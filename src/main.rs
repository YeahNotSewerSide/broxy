use std::{net::SocketAddr, str::FromStr as _};

use filter::{BodyFilter, Filter};
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
        address: SocketAddr::from_str("0.0.0.0:9944").unwrap(),
        use_ssl: false,
    };
    let load_balancer = load_balancer::LoadBalancer::new(vec![upstream]);

    let filters = vec![Filter::Method(hyper::Method::POST)];
    let body_filters = vec![BodyFilter::InternalFullBody(|_, body| {
        let serialized = serde_json::from_slice::<serde_json::Value>(body);
        if let Ok(serialized) = serialized {
            let method = serialized.get("method").and_then(|m| m.as_str());
            if let Some(method) = method {
                if method.eq("eth_sendTransaction") || method.eq("eth_sendRawTransaction") {
                    Ok(false)
                } else {
                    Ok(true)
                }
            } else {
                Ok(false)
            }
        } else {
            Err(unsafe { serialized.unwrap_err_unchecked() }.into())
        }
    })];
    let middleware = middleware::Middleware::new(
        vec![],
        vec![middleware::MiddlewareOutgoingFunction::Internal(
            |from, header| {
                header.headers.insert(
                    http::HeaderName::from_str("X-Provided-For").unwrap(),
                    from.to_string().parse().unwrap(),
                );
                Ok(())
            },
        )],
    );
    let service1 = Service::new(
        filters,
        body_filters,
        Some(middleware),
        &load_balancer,
        None,
    );

    let services = vec![service1];
    let bundle = ServiceBundle::new(&services);

    let server_addr = SocketAddr::from_str("0.0.0.0:8545").unwrap();
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
