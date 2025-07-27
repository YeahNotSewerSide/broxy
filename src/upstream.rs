use std::net::SocketAddr;

use http::Uri;

/// Configuration for an upstream server that the proxy forwards requests to.
///
/// This struct defines the connection details and routing information for
/// a backend server that handles the actual request processing.
#[derive(Debug, Clone)]
pub struct Upstream {
    /// The network address (IP and port) of the upstream server
    pub address: SocketAddr,
    /// Whether to use SSL/TLS when connecting to the upstream server
    pub use_ssl: bool,
}
