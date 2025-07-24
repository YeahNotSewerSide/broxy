use std::{collections::HashMap, net::SocketAddr, path::PathBuf};

use serde::{Deserialize, Serialize};

/// Main configuration structure for the Broxy proxy server.
/// 
/// This struct contains all the configuration options for the proxy,
/// including entry points, HTTP routing rules, and upstream server definitions.
#[derive(Serialize, Deserialize)]
pub struct Config {
    /// Entry points define the network interfaces and ports the proxy listens on
    pub entry_points: HashMap<String, EntryPoint>,
    /// HTTP routing rules that determine how requests are processed
    pub http: HashMap<String, Http>,
    /// Upstream server definitions for load balancing and routing
    pub upstream: HashMap<String, Upstream>,
}

/// Configuration for a network entry point where the proxy accepts connections.
/// 
/// Entry points define the listening address, optional domain name matching,
/// and SSL/TLS configuration.
#[derive(Serialize, Deserialize)]
pub struct EntryPoint {
    /// The network address (IP and port) to listen on
    pub address: SocketAddr,
    /// Optional regex pattern for matching domain names
    pub domain_name: Option<String>,
    /// SSL/TLS configuration for secure connections
    pub ssl: Option<SSL>,
}

/// SSL/TLS configuration for secure entry points.
/// 
/// This struct defines the certificate and private key files
/// needed for SSL/TLS termination.
#[derive(Serialize, Deserialize)]
pub struct SSL {
    /// Path to the SSL certificate file
    pub certificate: String,
    /// Path to the SSL private key file
    pub private_key: String,
}

/// HTTP routing rule configuration.
/// 
/// This struct defines how HTTP requests are routed to upstream servers,
/// including path matching, middleware processing, and load balancing.
#[derive(Serialize, Deserialize, Debug)]
pub struct Http {
    /// The entry point name this rule applies to
    pub entry_point: String,
    /// Regex pattern for matching request paths
    pub path: String,
    /// Optional list of middleware modules to apply
    pub middleware: Option<Vec<PathBuf>>,
    /// The upstream server group name to forward requests to
    pub pass_to: String,
}

/// Upstream server group configuration.
/// 
/// This struct defines a group of backend servers that can handle requests,
/// along with the load balancing strategy to use.
#[derive(Serialize, Deserialize)]
pub struct Upstream {
    /// List of server addresses in this upstream group
    pub servers: Vec<String>,
    /// Optional load balancing strategy name
    pub loadbalancer_strategy: Option<String>,
}
