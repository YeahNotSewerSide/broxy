//! Broxy - A high-performance reverse HTTP proxy server
//!
//! This library provides a flexible and extensible reverse HTTP proxy server with support for:
//! - Request/response filtering
//! - Middleware processing
//! - Load balancing
//! - SSL/TLS
//! - Custom routing rules
//!
//! The main components are organized into the following modules:
//! - `config`: Configuration structures for the proxy
//! - `filter`: Request and response filtering capabilities
//! - `load_balancer`: Load balancing strategies
//! - `logging`: Logging system initialization and configuration
//! - `middleware`: Request/response processing middleware
//! - `server`: HTTP server implementation
//! - `service`: Service definitions and processing logic
//! - `upstream`: Upstream server configuration

pub mod filter;
pub mod load_balancer;
pub mod middleware;
pub mod server;
pub mod service;
pub mod upstream;
pub mod utils;
pub use hyper;
