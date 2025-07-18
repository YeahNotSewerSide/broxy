use std::{error::Error, net::SocketAddr, str::FromStr as _};

use filter::{BodyFilter, Filter};
use http::Uri;
use middleware::Middleware;
use regex::Regex;
use server::Server;
use service::{Service, ServiceBundle};
mod config;
mod filter;
mod load_balancer;
mod middleware;
mod server;
mod service;
mod upstream;
mod utils;

#[tokio::main]
async fn main() {
    let upstream = upstream::Upstream {
        address: SocketAddr::from_str("0.0.0.0:3006").unwrap(),
        root_path: Uri::from_str("/").unwrap(),
        use_ssl: false,
    };
    let filters = vec![
        Filter::Method(hyper::Method::POST),
        Filter::Path(Regex::new("/login").unwrap()),
    ];
    let body_filters = vec![BodyFilter::InternalFullBody(|body| {
        let serialized = serde_json::from_slice::<serde_json::Value>(body);
        if let Ok(serialized) = serialized {
            match serialized.get("email") {
                Some(email) => Ok(email.eq("test")),
                None => Err(anyhow::anyhow!("No `email` field specified")),
            }
        } else {
            Err(unsafe { serialized.unwrap_err_unchecked() }.into())
        }
    })];
    let service = Service::new(filters, body_filters, None, upstream);
    let services = vec![service];
    let bundle = ServiceBundle::new(&services);
    let server = Server::new(SocketAddr::from_str("0.0.0.0:8181").unwrap(), bundle, None)
        .await
        .unwrap();
    loop {
        server.accept().await.unwrap();
    }
}
