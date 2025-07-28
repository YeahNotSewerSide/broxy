use std::net::SocketAddr;

use http::{request, response};

/// Incoming request middleware function types.
///
/// These functions are called before forwarding requests to upstream servers
/// and can modify request headers and bodies.
#[derive(Debug, Clone)]
pub enum MiddlewareIncomingFunction {
    /// External middleware (not yet implemented)
    External,
    /// Internal middleware that processes both headers and body
    InternalWithBody(fn(&SocketAddr, &mut request::Parts, &mut Vec<u8>) -> anyhow::Result<()>),
    /// Internal middleware that processes only headers
    Internal(fn(&SocketAddr, &mut request::Parts) -> anyhow::Result<()>),
}

impl MiddlewareIncomingFunction {
    /// Processes the incoming request with this middleware function.
    ///
    /// # Arguments
    ///
    /// * `parts` - The HTTP request header parts to modify
    /// * `body` - Optional mutable reference to the request body
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success or an error if processing fails.
    #[inline]
    pub fn process(
        &self,
        from: &SocketAddr,
        parts: &mut request::Parts,
        body: &mut Option<&mut Vec<u8>>,
    ) -> anyhow::Result<()> {
        match self {
            MiddlewareIncomingFunction::External => todo!(),
            MiddlewareIncomingFunction::InternalWithBody(func) => {
                if let Some(body) = body {
                    func(from, parts, body)
                } else {
                    Err(anyhow::anyhow!("No body provided"))
                }
            }
            MiddlewareIncomingFunction::Internal(func) => func(from, parts),
        }
    }

    /// Checks if this middleware function requires access to the request body.
    ///
    /// # Returns
    ///
    /// Returns `true` if the middleware needs the body, `false` otherwise.
    pub fn needs_body(&self) -> bool {
        match self {
            MiddlewareIncomingFunction::InternalWithBody(_) => true,
            _ => false,
        }
    }
}

/// Outgoing response middleware function types.
///
/// These functions are called after receiving responses from upstream servers
/// and can modify response headers and bodies.
#[derive(Debug, Clone)]
pub enum MiddlewareOutgoingFunction {
    /// External middleware (not yet implemented)
    External,
    /// Internal middleware that processes both headers and body
    InternalWithBody(
        fn(&SocketAddr, &SocketAddr, &mut response::Parts, &mut Vec<u8>) -> anyhow::Result<()>,
    ),
    /// Internal middleware that processes only headers
    Internal(fn(&SocketAddr, &SocketAddr, &mut response::Parts) -> anyhow::Result<()>),
}

impl MiddlewareOutgoingFunction {
    /// Processes the outgoing response with this middleware function.
    ///
    /// # Arguments
    ///
    /// * `parts` - The HTTP response header parts to modify
    /// * `body` - Optional mutable reference to the response body
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success or an error if processing fails.
    #[inline]
    pub fn process(
        &self,
        from: &SocketAddr,
        upstream_addr: &SocketAddr,
        parts: &mut response::Parts,
        body: &mut Option<&mut Vec<u8>>,
    ) -> anyhow::Result<()> {
        match self {
            MiddlewareOutgoingFunction::External => todo!(),
            MiddlewareOutgoingFunction::InternalWithBody(func) => {
                if let Some(body) = body {
                    func(from, upstream_addr, parts, body)
                } else {
                    Err(anyhow::anyhow!("No body provided"))
                }
            }
            MiddlewareOutgoingFunction::Internal(func) => func(from, upstream_addr, parts),
        }
    }

    /// Checks if this middleware function requires access to the response body.
    ///
    /// # Returns
    ///
    /// Returns `true` if the middleware needs the body, `false` otherwise.
    pub fn needs_body(&self) -> bool {
        match self {
            Self::InternalWithBody(_) => true,
            _ => false,
        }
    }
}

/// Middleware chain for processing requests and responses.
///
/// This struct contains collections of incoming and outgoing middleware functions
/// that are applied to requests and responses respectively.
#[derive(Debug, Clone)]
pub struct Middleware {
    /// Collection of incoming request middleware functions
    process_incoming: Vec<MiddlewareIncomingFunction>,
    /// Whether any incoming middleware requires the request body
    pub incoming_needs_body: bool,
    /// Collection of outgoing response middleware functions
    process_out: Vec<MiddlewareOutgoingFunction>,
    /// Whether any outgoing middleware requires the response body
    pub out_needs_body: bool,
}

impl Middleware {
    /// Creates a new middleware chain with the specified incoming and outgoing functions.
    ///
    /// # Arguments
    ///
    /// * `incoming` - Vector of incoming request middleware functions
    /// * `outgoing` - Vector of outgoing response middleware functions
    ///
    /// # Returns
    ///
    /// Returns a new `Middleware` instance with the specified functions.
    pub fn new(
        incoming: Vec<MiddlewareIncomingFunction>,
        outgoing: Vec<MiddlewareOutgoingFunction>,
    ) -> Self {
        let incoming_needs_body = !incoming.iter().all(|proc| !proc.needs_body());
        let out_needs_body = !outgoing.iter().all(|proc| !proc.needs_body());
        Self {
            process_incoming: incoming,
            process_out: outgoing,
            incoming_needs_body,
            out_needs_body,
        }
    }

    /// Processes incoming request headers and optionally the body through all middleware.
    ///
    /// # Arguments
    ///
    /// * `parts` - The HTTP request header parts to process
    /// * `body` - Optional mutable reference to the request body
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success or an error if any middleware fails.
    pub fn process_incoming(
        &self,
        from: &SocketAddr,
        parts: &mut request::Parts,
        mut body: Option<&mut Vec<u8>>,
    ) -> anyhow::Result<()> {
        for proc in &self.process_incoming {
            proc.process(from, parts, &mut body)?;
        }
        Ok(())
    }

    /// Processes outgoing response headers and optionally the body through all middleware.
    ///
    /// # Arguments
    ///
    /// * `parts` - The HTTP response header parts to process
    /// * `body` - Optional mutable reference to the response body
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success or an error if any middleware fails.
    pub fn process_outgoing(
        &self,
        from: &SocketAddr,
        upstream_addr: &SocketAddr,
        parts: &mut response::Parts,
        mut body: Option<&mut Vec<u8>>,
    ) -> anyhow::Result<()> {
        for proc in &self.process_out {
            proc.process(from, upstream_addr, parts, &mut body)?;
        }
        Ok(())
    }
}
