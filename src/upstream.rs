use std::net::SocketAddr;

use http::Uri;

#[derive(Debug, Clone)]
pub struct Upstream {
    pub address: SocketAddr,
    pub root_path: Uri,
    pub use_ssl: bool,
}
