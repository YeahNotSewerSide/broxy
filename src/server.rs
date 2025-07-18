use std::{convert::Infallible, net::SocketAddr, task::Poll};

use anyhow::Result;
use http::Response;
use http_body_util::combinators::BoxBody;
use hyper::{
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::{rt::TokioExecutor, server::conn::auto::Builder};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use crate::service::{Service, ServiceBundle};

pub struct Server {
    connection: TcpListener,
    tls_acceptor: Option<TlsAcceptor>,
    services: ServiceBundle,
}

impl Server {
    pub async fn new(
        addr: SocketAddr,
        services: ServiceBundle,
        tls_acceptor: Option<TlsAcceptor>,
    ) -> Result<Self> {
        Ok(Self {
            connection: TcpListener::bind(&addr).await?,
            tls_acceptor,
            services,
        })
    }

    pub async fn accept(&self) -> Result<()> {
        let (conn, address) = self.connection.accept().await?;

        let bundle = self.services.clone();

        if let Some(tls_acceptor) = self.tls_acceptor.as_ref() {
        } else {
            let io = HyperSocket::from(conn);
            tokio::spawn(async move {
                let result = Builder::new(TokioExecutor::new())
                    .serve_connection(io, bundle)
                    .await;
                if let Err(e) = result {}
            });
        }
        Ok(())
    }
}

pub struct HyperSocket {
    stream: TcpStream,
}

impl From<TcpStream> for HyperSocket {
    fn from(stream: TcpStream) -> Self {
        Self { stream }
    }
}

impl hyper::rt::Read for HyperSocket {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        let n = unsafe {
            let mut tbuf = tokio::io::ReadBuf::uninit(buf.as_mut());
            match tokio::io::AsyncRead::poll_read(
                std::pin::Pin::new(&mut self.stream),
                cx,
                &mut tbuf,
            ) {
                Poll::Ready(Ok(())) => tbuf.filled().len(),
                other => return other,
            }
        };

        unsafe {
            buf.advance(n);
        }
        Poll::Ready(Ok(()))
    }
}

impl hyper::rt::Write for HyperSocket {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        tokio::io::AsyncWrite::poll_write(std::pin::Pin::new(&mut self.stream), cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        tokio::io::AsyncWrite::poll_flush(std::pin::Pin::new(&mut self.stream), cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        tokio::io::AsyncWrite::poll_shutdown(std::pin::Pin::new(&mut self.stream), cx)
    }

    fn is_write_vectored(&self) -> bool {
        tokio::io::AsyncWrite::is_write_vectored(&self.stream)
    }

    fn poll_write_vectored(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        tokio::io::AsyncWrite::poll_write_vectored(std::pin::Pin::new(&mut self.stream), cx, bufs)
    }
}
