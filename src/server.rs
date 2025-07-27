use std::net::SocketAddr;

use anyhow::Result;
use hyper_util::{
    rt::{TokioExecutor, TokioIo as HyperSocket},
    server::conn::auto::Builder,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error};

use crate::service::ServiceBundle;

/// HTTP server that accepts connections and routes requests to services.
///
/// This struct manages the TCP listener, TLS configuration, and service bundle
/// for handling incoming HTTP connections.
pub struct Server {
    /// The TCP listener for accepting incoming connections
    connection: TcpListener,
    /// The service bundle that handles request routing
    services: ServiceBundle,
    tls_acceptor: Option<TlsAcceptor>,
    _accept: fn(&Server, ServiceBundle, TcpStream) -> (),
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
            _accept: if tls_acceptor.is_some() {
                debug!("Setting up tls acceptor");
                Self::_tls_acceptor
            } else {
                debug!("Setting up non-tls acceptor");
                Self::_non_tls_acceptor
            },
            connection: TcpListener::bind(&addr).await?,
            tls_acceptor,
            services,
        })
    }

    fn _non_tls_acceptor(_: &Self, bundle: ServiceBundle, conn: TcpStream) {
        let io = HyperSocket::new(conn);

        tokio::spawn(async move {
            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection(io, bundle)
                .await
            {
                error!("Error serving non tls connection: {:?}", e);
            }
        });
    }

    fn _tls_acceptor(server: &Self, bundle: ServiceBundle, conn: TcpStream) {
        // TODO: remove clone
        let acceptor = unsafe { server.tls_acceptor.as_ref().unwrap_unchecked() }.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(conn).await {
                Ok(tls_stream) => tls_stream,
                Err(err) => {
                    error!("failed to perform tls handshake: {err:#}");
                    return;
                }
            };
            let io = HyperSocket::new(tls_stream);
            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection(io, bundle)
                .await
            {
                error!("Error serving tls connection: {:?}", e);
            }
        });
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

        (self._accept)(self, bundle, conn);
        Ok(())
    }
}
