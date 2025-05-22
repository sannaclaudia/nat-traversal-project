//! A libp2p chat over DCUtR + relay example.

use clap::Parser;
use futures::StreamExt;
use libp2p::{
    core::multiaddr::{Multiaddr, Protocol},
    dcutr, identify, identity, noise, ping, relay,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, PeerId,
};
use rand;
use std::{error::Error, iter};
use tokio::{
    io::{self, AsyncBufReadExt, BufReader},
    sync::mpsc,
};

mod chat;
use chat::{ChatCodec, ChatProtocol, ChatRequest, ChatResponse};

#[derive(Debug, Parser)]
#[command(name = "libp2p DCUtR chat")]
struct Opts {
    /// Mode: "dial" or "listen"
    #[arg(long)]
    mode: Mode,

    /// Address of your relay server
    #[arg(long)]
    relay_address: Multiaddr,

    /// Remote PeerId (only in dial mode)
    #[arg(long)]
    remote_peer_id: Option<PeerId>,

    // secret key seed (optional)
    #[arg(long)]
    secret_key_seed: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Parser)]
enum Mode {
    Dial,
    Listen,
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dial" => Ok(Mode::Dial),
            "listen" => Ok(Mode::Listen),
            _ => Err("Expected 'dial' or 'listen'".into()),
        }
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event")]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
    chat: request_response::Behaviour<ChatCodec>,
}

#[derive(Debug)]
enum Event {
    Relay(relay::client::Event),
    Ping(ping::Event),
    Identify(identify::Event),
    Dcutr(dcutr::Event),
    Chat(request_response::Event<ChatRequest, ChatResponse>),
}

impl From<relay::client::Event> for Event {
    fn from(e: relay::client::Event) -> Self {
        Event::Relay(e)
    }
}
impl From<ping::Event> for Event {
    fn from(e: ping::Event) -> Self {
        Event::Ping(e)
    }
}
impl From<identify::Event> for Event {
    fn from(e: identify::Event) -> Self {
        Event::Identify(e)
    }
}
impl From<dcutr::Event> for Event {
    fn from(e: dcutr::Event) -> Self {
        Event::Dcutr(e)
    }
}
impl From<request_response::Event<ChatRequest, ChatResponse>> for Event {
    fn from(e: request_response::Event<ChatRequest, ChatResponse>) -> Self {
        Event::Chat(e)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let opts = Opts::parse();

    //
    // === Build the Swarm with a fresh new identity ===
    //
    let transport =
        libp2p::SwarmBuilder::with_existing_identity(generate_ed25519(opts.secret_key_seed))
            // use Tokio executor
            .with_tokio()
            // TCP + Noise + Yamux
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )?
            // QUIC v1
            .with_quic()
            // DNS
            .with_dns()?
            // Relay client behaviour
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            // Finally, our custom protocol
            .with_behaviour(|keypair, relay_behaviour| {
                // Set up RequestResponse for /chat/1.0.0
                let cfg = request_response::Config::default()
                    .with_request_timeout(std::time::Duration::from_secs(30));
                let protocols = iter::once((ChatProtocol(), ProtocolSupport::Full));
                let chat = request_response::Behaviour::new(protocols, cfg);

                Behaviour {
                    relay_client: relay_behaviour,
                    ping: ping::Behaviour::new(
                        ping::Config::new()
                            .with_interval(std::time::Duration::from_secs(5))
                            .with_timeout(std::time::Duration::from_secs(30)),
                    ),
                    identify: identify::Behaviour::new(identify::Config::new(
                        "/chat/1.0.0".into(),
                        keypair.public(),
                    )),
                    dcutr: dcutr::Behaviour::new(keypair.public().to_peer_id()),
                    chat,
                }
            })?
            .build();

    let mut swarm = transport;

    // Listen on QUIC and TCP
    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // Give a moment for listen addresses to appear
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Connect to relay just to register ourselves
    swarm.dial(opts.relay_address.clone())?;

    // Brief pause for DCUtR handshake to complete
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    println!("Local PeerId: {:?}", swarm.local_peer_id());

    // Reserve or dial on the relay circuit
    match opts.mode {
        Mode::Dial => {
            let peer = opts
                .remote_peer_id
                .expect("dial mode needs --remote-peer-id");
            swarm.dial(
                opts.relay_address
                    .clone()
                    .with(Protocol::P2pCircuit)
                    .with(Protocol::P2p(peer)),
            )?;
        }
        Mode::Listen => {
            swarm.listen_on(opts.relay_address.clone().with(Protocol::P2pCircuit))?;
        }
    }

    // Spawn a task to read stdin lines
    let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut reader = BufReader::new(io::stdin()).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = stdin_tx.send(line);
        }
    });

    println!("Chat ready. Type messages and press enter to send.");

    let mut remote = opts.remote_peer_id;
    // Main event loop
    loop {
        tokio::select! {
            // Handle libp2p events
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(Event::Chat(e)) => match e {
                    request_response::Event::Message { peer, message, .. } => {
                        // capture remote on first incoming
                        remote.get_or_insert(peer);
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                let txt = String::from_utf8_lossy(&request.0);
                                println!("[{} ▶︎ us] {}", peer, txt);
                                swarm.behaviour_mut().chat
                                    .send_response(channel, ChatResponse(request))
                                    .unwrap();
                            }
                            request_response::Message::Response { response, .. } => {
                                let txt = String::from_utf8_lossy(&response.0.0);
                                println!("[us ▶︎ {}] {}", peer, txt);
                            }
                        }
                    }
                    evt => {
                        // failures, responses sent, etc.
                        tracing::debug!("Chat event: {:?}", evt);
                    }
                },
                other => {
                    // Handle other events
                    //println!("Swarm event: {:?}", other);
                }
            },

            // Handle user input
            Some(line) = stdin_rx.recv() => {
                if let Some(peer) = remote {
                    swarm.behaviour_mut().chat
                        .send_request(&peer, ChatRequest(line.into_bytes()));
                } else {
                    println!("…still waiting for a peer to connect");
                }
            }
        }
    }
}

fn generate_ed25519(secret_key_seed: Option<u8>) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed.unwrap_or(rand::random::<u8>());

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}
