use std::{net::SocketAddr, str::FromStr as _};

use broxy_core::filter::{BodyFilter, Filter};
use broxy_core::http_body_util::{BodyExt as _, Full};
use broxy_core::hyper::body::Bytes;
use broxy_core::server::Server;
use broxy_core::service::{Service, ServiceBundle};
use http::StatusCode;
use tracing::{debug, error, info, info_span, instrument};

mod logging;

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

    let load_balancer = broxy_core::load_balancer::LoadBalancer::new(vec![
        broxy_core::upstream::Upstream {
            address: SocketAddr::from_str("0.0.0.0:9944").unwrap(),
            use_ssl: false,
        },
        broxy_core::upstream::Upstream {
            address: SocketAddr::from_str("0.0.0.0:9945").unwrap(),
            use_ssl: false,
        },
        broxy_core::upstream::Upstream {
            address: SocketAddr::from_str("0.0.0.0:9946").unwrap(),
            use_ssl: false,
        },
        broxy_core::upstream::Upstream {
            address: SocketAddr::from_str("0.0.0.0:9947").unwrap(),
            use_ssl: false,
        },
        broxy_core::upstream::Upstream {
            address: SocketAddr::from_str("0.0.0.0:9948").unwrap(),
            use_ssl: false,
        },
    ]);

    let filters = vec![Filter::Method(broxy_core::hyper::Method::POST)];
    let body_filters = vec![BodyFilter::InternalFullBody(|_, body| {
        let serialized = serde_json::from_slice::<serde_json::Value>(body);
        if let Ok(serialized) = serialized {
            let method = serialized.get("method").and_then(|m| m.as_str());
            if let Some(method) = method {
                if method.eq("eth_sendTransaction") || method.eq("eth_sendRawTransaction") {
                    let id = serialized.get("id").and_then(|m| m.as_u64());
                    if let Some(id) = id {
                        Ok(Some(
                        http::response::Response::builder()
                            .status(StatusCode::FORBIDDEN)
                            .header("content-type", "application/json")
                            .body(
                                Full::<Bytes>::from(format!("{{{{\"jsonrpc\": \"2.0\",\"error\": {{\"code\": -32601,\"message\": \"Method not found\",\"data\": {{\"method\": \"unknownMethod\"}},\"id\": {}}}", id))
                                    .map_err(|never| match never {})
                                    .boxed(),
                            )
                            .unwrap(),
                    ))
                    } else {
                        Ok(Some(
                            http::response::Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(
                                    Full::<Bytes>::from(Vec::with_capacity(0))
                                        .map_err(|never| match never {})
                                        .boxed(),
                                )
                                .unwrap(),
                        ))
                    }
                } else {
                    Ok(None)
                }
            } else {
                Ok(Some(
                    http::response::Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(
                            Full::<Bytes>::from(Vec::with_capacity(0))
                                .map_err(|never| match never {})
                                .boxed(),
                        )
                        .unwrap(),
                ))
            }
        } else {
            Err(unsafe { serialized.unwrap_err_unchecked() }.into())
        }
    })];
    let middleware = broxy_core::middleware::Middleware::new(
        vec![],
        vec![
            broxy_core::middleware::MiddlewareOutgoingFunction::Internal(
                |from, upstream_addr, header| {
                    header.headers.insert(
                        http::HeaderName::from_str("X-Provided-For").unwrap(),
                        from.to_string().parse().unwrap(),
                    );
                    header.headers.insert(
                        http::HeaderName::from_str("X-backend").unwrap(),
                        upstream_addr.to_string().parse().unwrap(),
                    );
                    Ok(())
                },
            ),
        ],
    );
    let service = Service::new(filters, body_filters, Some(middleware), &load_balancer);

    let services = vec![service];
    let bundle = ServiceBundle::new(&services);

    let server_addr = SocketAddr::from_str("0.0.0.0:8546").unwrap();
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
