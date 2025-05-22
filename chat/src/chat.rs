use async_trait::async_trait;
use futures::AsyncReadExt;
use futures::AsyncWriteExt;
use futures::{io, AsyncRead, AsyncWrite};
use libp2p::request_response::Codec as RequestResponseCodec;
use std::io::ErrorKind;

#[derive(Clone)]
pub struct ChatProtocol();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRequest(pub Vec<u8>);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatResponse(pub ChatRequest);

impl AsRef<str> for ChatProtocol {
    fn as_ref(&self) -> &str {
        std::str::from_utf8(b"/chat/1.0.0").unwrap()
    }
}

#[derive(Clone, Default)]
pub struct ChatCodec();

#[async_trait]
impl RequestResponseCodec for ChatCodec {
    type Protocol = ChatProtocol;
    type Request = ChatRequest;
    type Response = ChatResponse;

    async fn read_request<T>(&mut self, _: &ChatProtocol, io: &mut T) -> io::Result<ChatRequest>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();
        loop {
            let mut tmp = [0u8; 1024];
            match io.read(&mut tmp).await {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(ChatRequest(buf))
    }

    async fn read_response<T>(
        &mut self,
        proto: &ChatProtocol,
        io: &mut T,
    ) -> io::Result<ChatResponse>
    where
        T: AsyncRead + Unpin + Send,
    {
        // same framing as request
        self.read_request(proto, io).await.map(ChatResponse)
    }

    async fn write_request<T>(
        &mut self,
        _: &ChatProtocol,
        io: &mut T,
        ChatRequest(data): ChatRequest,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.write_all(&data).await?;
        io.close().await
    }

    async fn write_response<T>(
        &mut self,
        _: &ChatProtocol,
        io: &mut T,
        ChatResponse(data): ChatResponse,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.write_all(&data.0).await?;
        io.close().await
    }
}
