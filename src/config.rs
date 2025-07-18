use std::{collections::HashMap, net::SocketAddr, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub entry_points: HashMap<String, EntryPoint>,
    pub http: HashMap<String, Http>,
    pub upstream: HashMap<String, Upstream>,
}

#[derive(Serialize, Deserialize)]
pub struct EntryPoint {
    pub address: SocketAddr,
    /// regex
    pub domain_name: Option<String>,
    pub ssl: Option<SSL>,
}

#[derive(Serialize, Deserialize)]
pub struct SSL {
    pub certificate: String,
    pub private_key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Http {
    pub entry_point: String,
    /// regex
    pub path: String,
    pub middleware: Option<Vec<PathBuf>>,
    pub pass_to: String,
}

#[derive(Serialize, Deserialize)]
pub struct Upstream {
    pub servers: Vec<String>,
    pub loadbalancer_strategy: Option<String>,
}
