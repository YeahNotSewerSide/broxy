use std::{error::Error, net::SocketAddr, str::FromStr as _};

use filter::{BodyFilter, Filter};
use http::Uri;
use middleware::{Middleware, MiddlewareIncomingFunction};
use regex::Regex;
use server::Server;
use service::{Service, ServiceBundle};
use tracing::{debug, error, info, info_span, instrument};
mod config;
mod filter;
mod load_balancer;
mod logging;
mod middleware;
mod server;
mod service;
mod upstream;
mod utils;

#[tokio::main]
async fn main() {
    // Initialize logging system
    if let Err(e) = logging::init_logging_from_env() {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    let _span = info_span!("broxy_startup");
    let _enter = _span.enter();

    info!("Starting Broxy proxy server");

    let upstream = upstream::Upstream {
        address: SocketAddr::from_str("0.0.0.0:3006").unwrap(),
        root_path: Uri::from_str("/").unwrap(),
        use_ssl: false,
    };

    debug!("Configured upstream: {:?}", upstream);

    let filters = vec![
        Filter::Method(hyper::Method::POST),
        Filter::Path(Regex::new("/login").unwrap()),
    ];
    let body_filters = vec![BodyFilter::InternalFullBody(|body| {
        let serialized = serde_json::from_slice::<serde_json::Value>(body);
        if let Ok(serialized) = serialized {
            match serialized.get("email") {
                Some(email) => Ok(email.eq("email@email.com")),
                None => Err(anyhow::anyhow!("No `email` field specified")),
            }
        } else {
            Err(unsafe { serialized.unwrap_err_unchecked() }.into())
        }
    })];
    let service1 = Service::new(filters, body_filters, None, upstream.clone());

    let filters = vec![
        Filter::Method(hyper::Method::GET),
        Filter::Path(Regex::new("/api/user").unwrap()),
    ];
    let middleware = Middleware::new(
        vec![MiddlewareIncomingFunction::Internal(|header| {
            header.uri = Uri::from_str("/user")?;
            Ok(())
        })],
        vec![],
    );
    let service2 = Service::new(filters, Vec::new(), Some(middleware), upstream);

    let services = vec![service1, service2];
    let bundle = ServiceBundle::new(&services);

    let server_addr = SocketAddr::from_str("0.0.0.0:8181").unwrap();
    info!("Starting server on {}", server_addr);

    let server = Server::new(server_addr, bundle, None).await.unwrap();

    info!("Server started successfully, accepting connections");

    // Drop the span before entering the main loop
    drop(_enter);
    drop(_span);

    run_server(server).await;
}

#[instrument(skip(server))]
async fn run_server(server: Server) {
    let _span = info_span!("server_loop");
    let _enter = _span.enter();

    loop {
        match server.accept().await {
            Ok(_) => debug!("Accepted new connection"),
            Err(e) => error!("Failed to accept connection: {}", e),
        }
    }
}
