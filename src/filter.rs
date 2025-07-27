use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    pin::Pin,
};

use http::request::Parts;
use hyper::body::Incoming;

/// Type alias for external C function filters that operate on request bodies.
///
/// This function type is used for integrating with external filtering libraries
/// written in C or other languages that can be called via FFI.
pub type FilterBody = unsafe extern "C" fn(*const u8, u64) -> bool;

/// Request filtering criteria for matching HTTP requests.
///
/// Filters are used to determine whether a request should be processed
/// by a particular service based on various criteria like HTTP method,
/// host header, or request path.
#[derive(Debug, Clone)]
pub enum Filter {
    /// Filter by HTTP method (GET, POST, PUT, etc.)
    Method(hyper::Method),
    /// Filter by host header using regex pattern matching
    Host(regex::Regex),
    /// Filter by request path using regex pattern matching
    Path(regex::Regex),

    BlackList(HashSet<IpAddr>),
    WhiteList(HashSet<IpAddr>),

    CustomFunction(fn(&SocketAddr, &Parts) -> anyhow::Result<bool>), //Body(libloading::Symbol<'static, FilterBody>),
}

impl Filter {
    /// Applies the filter to a request header to determine if it matches.
    ///
    /// # Arguments
    ///
    /// * `header` - The HTTP request header parts to filter
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the request matches the filter criteria,
    /// `Ok(false)` if it doesn't match, or an error if filtering fails.
    pub fn filter(&self, from: &SocketAddr, header: &Parts) -> anyhow::Result<bool> {
        Ok(match self {
            Filter::Method(method) => header.method.eq(method),
            Filter::Host(host_regex) => host_regex.is_match(
                header
                    .uri
                    .host()
                    .ok_or(anyhow::anyhow!("Host is empty: {:?}", header))?,
            ),
            Filter::Path(path_regex) => path_regex.is_match(header.uri.path()),
            Filter::BlackList(ip_addrs) => ip_addrs.get(&from.ip()).is_none(),
            Filter::WhiteList(ip_addrs) => ip_addrs.get(&from.ip()).is_some(),
            Filter::CustomFunction(function) => function(from, header)?,
        })
    }
}

/// Body filtering strategies for processing request bodies.
///
/// Body filters can operate on incoming request bodies to determine
/// whether a request should be processed or rejected based on content.
#[derive(Debug, Clone)]
pub enum BodyFilter {
    /// Asynchronous body filter that processes the full incoming body stream
    InternalIncoming(
        fn(Incoming) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<Vec<u8>>>> + Send>>,
    ),
    /// Synchronous body filter that processes the complete body as bytes
    InternalFullBody(fn(&SocketAddr, &[u8]) -> anyhow::Result<bool>),
    /// External body filter (not yet implemented)
    External,
}

impl BodyFilter {
    /// Applies the body filter to a request body.
    ///
    /// This method is used for synchronous body filtering where the complete
    /// body is available as bytes.
    ///
    /// # Arguments
    ///
    /// * `body` - The complete request body as bytes
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the body passes the filter, `Ok(false)` if it's rejected,
    /// or an error if filtering fails.
    pub fn filter(&self, from: &SocketAddr, body: &[u8]) -> anyhow::Result<bool> {
        match self {
            BodyFilter::InternalFullBody(func) => func(from, body),
            BodyFilter::External => unimplemented!(),
            BodyFilter::InternalIncoming(_) => {
                Err(anyhow::anyhow!("Expected to be called by `filter_async`"))
            }
        }
    }

    /// Applies the body filter asynchronously to an incoming body stream.
    ///
    /// This method is used for asynchronous body filtering where the body
    /// is still being received as a stream.
    ///
    /// # Arguments
    ///
    /// * `incoming` - The incoming HTTP body stream
    ///
    /// # Returns
    ///
    /// Returns a future that resolves to `Ok(Some(body_bytes))` if the body passes
    /// the filter, `Ok(None)` if the body should be rejected, or an error if filtering fails.
    pub async fn filter_async(&self, incoming: Incoming) -> anyhow::Result<Option<Vec<u8>>> {
        if let Self::InternalIncoming(filter_incoming) = self {
            filter_incoming(incoming).await
        } else {
            Err(anyhow::anyhow!("Expected to be called by `filter`"))
        }
    }

    /// Checks if this body filter requires asynchronous processing.
    ///
    /// # Returns
    ///
    /// Returns `true` if the filter uses asynchronous processing,
    /// `false` if it uses synchronous processing.
    #[inline]
    pub fn use_async(&self) -> bool {
        if let Self::InternalIncoming(_) = self {
            true
        } else {
            false
        }
    }
}

/// Raw pointer wrapper for body filters to enable FFI integration.
///
/// This struct provides a safe way to pass body filters to external code
/// while maintaining thread safety guarantees.
#[derive(Debug, Clone)]
pub struct BodyFilters {
    /// Raw pointer to an array of body filters
    pub filters: *const BodyFilter,
    /// Number of filters in the array
    pub len: usize,
}

// SAFETY: This is safe because BodyFilter is Send and Sync
unsafe impl Send for BodyFilters {}
// SAFETY: This is safe because BodyFilter is Send and Sync
unsafe impl Sync for BodyFilters {}
