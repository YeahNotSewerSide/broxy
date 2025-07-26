use std::{convert::Infallible, net::SocketAddr, task::Poll};

use anyhow::Result;
use http::{Request, Response};
use http_body_util::combinators::BoxBody;
use hyper::{
    body::{self, Bytes, Incoming},
    service::{Service as _, service_fn},
};
use hyper_util::{rt::TokioExecutor, server::conn::auto::Builder};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use crate::service::{Service, ServiceBundle};

/// HTTP server that accepts connections and routes requests to services.
///
/// This struct manages the TCP listener, TLS configuration, and service bundle
/// for handling incoming HTTP connections.
pub struct Server {
    /// The TCP listener for accepting incoming connections
    connection: TcpListener,
    /// Optional TLS acceptor for secure connections
    tls_acceptor: Option<TlsAcceptor>,
    /// The service bundle that handles request routing
    services: ServiceBundle,
}

impl Server {
    /// Creates a new server instance bound to the specified address.
    ///
    /// # Arguments
    ///
    /// * `addr` - The network address to bind to
    /// * `services` - The service bundle for handling requests
    /// * `tls_acceptor` - Optional TLS acceptor for secure connections
    ///
    /// # Returns
    ///
    /// Returns a `Result<Server>` containing the new server instance or an error.
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

    /// Accepts a new connection and spawns a task to handle it.
    ///
    /// This method accepts a TCP connection and spawns an asynchronous task
    /// to process the HTTP request using the service bundle.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when a connection is successfully accepted and handled,
    /// or an error if the connection fails.
    pub async fn accept(&self) -> Result<()> {
        let (conn, address) = self.connection.accept().await?;

        let mut bundle = self.services.clone();
        bundle.from = address;

        if let Some(tls_acceptor) = self.tls_acceptor.as_ref() {
            // TODO: Implement TLS connection handling
        } else {
            let io = HyperSocket::from(conn);

            tokio::spawn(async move {
                let result = Builder::new(TokioExecutor::new())
                    .serve_connection(io, bundle)
                    .await;
                if let Err(e) = result {
                    // TODO: Handle connection errors
                }
            });
        }
        Ok(())
    }
}

/// Wrapper around `TcpStream` that implements Hyper's I/O traits.
///
/// This struct provides the necessary trait implementations to use Tokio's
/// `TcpStream` with Hyper's HTTP server.
pub struct HyperSocket {
    /// The underlying TCP stream
    stream: TcpStream,
}

impl From<TcpStream> for HyperSocket {
    /// Creates a `HyperSocket` from a `TcpStream`.
    fn from(stream: TcpStream) -> Self {
        Self { stream }
    }
}

impl hyper::rt::Read for HyperSocket {
    /// Implements asynchronous reading for Hyper.
    ///
    /// This method polls the underlying TCP stream for data and advances
    /// the read buffer cursor accordingly.
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
    /// Implements asynchronous writing for Hyper.
    ///
    /// This method polls the underlying TCP stream to write data.
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        tokio::io::AsyncWrite::poll_write(std::pin::Pin::new(&mut self.stream), cx, buf)
    }

    /// Implements asynchronous flushing for Hyper.
    ///
    /// This method polls the underlying TCP stream to flush any buffered data.
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        tokio::io::AsyncWrite::poll_flush(std::pin::Pin::new(&mut self.stream), cx)
    }

    /// Implements asynchronous shutdown for Hyper.
    ///
    /// This method polls the underlying TCP stream to initiate a graceful shutdown.
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        tokio::io::AsyncWrite::poll_shutdown(std::pin::Pin::new(&mut self.stream), cx)
    }

    /// Checks if the underlying stream supports vectored writes.
    fn is_write_vectored(&self) -> bool {
        tokio::io::AsyncWrite::is_write_vectored(&self.stream)
    }

    /// Implements asynchronous vectored writing for Hyper.
    ///
    /// This method polls the underlying TCP stream to write multiple buffers efficiently.
    fn poll_write_vectored(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        tokio::io::AsyncWrite::poll_write_vectored(std::pin::Pin::new(&mut self.stream), cx, bufs)
    }
}
