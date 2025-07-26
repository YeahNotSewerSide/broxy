//! Service definitions and request processing logic.
//!
//! This module contains the core service abstraction that handles HTTP request processing,
//! filtering, middleware application, and upstream forwarding. It provides both individual
//! service instances and service bundles for routing requests.

use std::{net::SocketAddr, pin::Pin, str::FromStr as _};

use http::{Request, Response, StatusCode, request::Parts};
use http_body_util::{BodyExt as _, Empty, Full, combinators::BoxBody};
use hyper::{
    body::{Body as _, Bytes, Incoming},
    client::conn::http1::Builder,
    service::Service as HyperService,
};
use rayon::iter::{IntoParallelRefIterator as _, ParallelIterator as _};
use tokio::net::TcpStream;
use tracing::{debug, error, info, warn};

use crate::{
    filter::{BodyFilter, BodyFilters, Filter},
    load_balancer::LoadBalancer,
    middleware::Middleware,
    server::HyperSocket,
    upstream::Upstream,
};

/// Function type for processing HTTP requests.
///
/// This type alias defines the signature for request processing functions
/// that take a service reference, upstream configuration, request parts,
/// and incoming body, returning a future that resolves to a response.
type ProcessFunction = fn(
    &Service,
    Upstream,
    &SocketAddr,
    http::request::Parts,
    Incoming,
) -> Pin<
    Box<dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>> + Send>,
>;

/// Function type for generating "not found" responses.
///
/// This type alias defines the signature for functions that generate
/// custom response bodies when no matching service is found.
type BodyNotFoundFunction = fn() -> Response<BoxBody<Bytes, hyper::Error>>;

/// A service that handles HTTP requests with filtering, middleware, and upstream forwarding.
///
/// Services are the core abstraction in Broxy that define how requests are processed.
/// Each service contains filters to match requests, optional middleware for processing,
/// and an upstream server configuration for forwarding requests.
#[derive(Debug, Clone)]
pub struct Service {
    /// Request header filters for matching requests
    filters: Vec<Filter>,
    /// Request body filters for content-based filtering
    body_filters: Vec<BodyFilter>,
    /// Optional middleware for request/response processing
    middleware: Option<Middleware>,
    /// Upstream server configuration
    load_balancer: *const LoadBalancer,
    /// Optional custom "not found" response generator
    not_found_body_response: Option<BodyNotFoundFunction>,
    /// Function pointer to the appropriate processing method
    _process: ProcessFunction,
    /// Function pointer to the appropriate filtering method
    _filter: fn(&Service, &SocketAddr, header: &Parts) -> anyhow::Result<bool>,
}

impl Service {
    /// Creates a new service with the specified configuration.
    ///
    /// The service automatically selects the most efficient processing and filtering
    /// strategies based on the provided configuration (e.g., parallel vs sequential
    /// filtering, body processing vs header-only processing).
    ///
    /// # Arguments
    ///
    /// * `filters` - Request header filters for matching requests
    /// * `body_filters` - Request body filters for content-based filtering
    /// * `middleware` - Optional middleware for request/response processing
    /// * `upstream` - Upstream server configuration
    /// * `not_found_body_response` - Optional custom "not found" response generator
    ///
    /// # Returns
    ///
    /// Returns a new `Service` instance configured with the specified parameters.
    pub fn new(
        filters: Vec<Filter>,
        body_filters: Vec<BodyFilter>,
        middleware: Option<Middleware>,
        load_balancer: *const LoadBalancer,
        not_found_body_response: Option<BodyNotFoundFunction>,
    ) -> Self {
        let amount_of_filters = filters.len();
        let has_body_filters = body_filters.len() > 0;
        let has_middleware = middleware.is_some();
        let needs_body = has_body_filters
            || (has_middleware && middleware.as_ref().unwrap().incoming_needs_body);

        debug!(
            "Creating service with {} filters, {} body filters, middleware: {}, needs_body: {}",
            amount_of_filters,
            body_filters.len(),
            has_middleware,
            needs_body
        );

        Self {
            filters,
            load_balancer,
            _process: if has_middleware {
                if needs_body {
                    Service::process_with_body
                } else {
                    Service::process_without_body_with_middleware
                }
            } else {
                Self::process_without_body_without_middleware
            },
            middleware,
            body_filters,
            not_found_body_response,
            _filter: if amount_of_filters > 5 {
                Service::filter_parallel_header
            } else {
                Service::filter_sequential_header
            },
        }
    }

    /// Returns a reference to the upstream configuration for this service.
    ///
    /// # Returns
    ///
    /// Returns a reference to the `Upstream` configuration.
    pub fn get_upstream(&self) -> &Upstream {
        unsafe { &*(*self.load_balancer).get_upstream() }
    }

    /// Filters a request by its header information.
    ///
    /// This method applies all configured header filters to determine if the request
    /// should be processed by this service. The filtering strategy (sequential vs parallel)
    /// is automatically selected based on the number of filters.
    ///
    /// # Arguments
    ///
    /// * `header` - The HTTP request header parts to filter
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the request matches all filters, `Ok(false)` if it doesn't match,
    /// or an error if filtering fails.
    #[inline]
    pub fn filter_request_by_header(
        &self,
        from: &SocketAddr,
        header: &Parts,
    ) -> anyhow::Result<bool> {
        let result = (self._filter)(self, from, header);
        match &result {
            Ok(matched) => debug!("Header filter result: {}", matched),
            Err(e) => error!("Header filter error: {}", e),
        }
        result
    }

    /// Creates a raw body filters structure for FFI integration.
    ///
    /// This method creates a `BodyFilters` struct that can be safely passed to external code.
    ///
    /// # Returns
    ///
    /// Returns a `BodyFilters` struct containing raw pointers to the body filters.
    ///
    /// # Panics
    ///
    /// Panics if called on a service with no body filters.
    fn get_body_filters_raw(&self) -> BodyFilters {
        BodyFilters {
            filters: self
                .body_filters
                .get(0)
                .expect("`get_body_filters_raw` was called with no body filters")
                as *const _,
            len: self.body_filters.len(),
        }
    }

    /// Filters a request body using the provided body filters.
    ///
    /// This method applies all body filters to determine if the request body
    /// should be processed. Currently only supports synchronous body filtering.
    ///
    /// # Arguments
    ///
    /// * `body_filters` - The body filters to apply
    /// * `body` - The request body as bytes
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the body passes all filters, `Ok(false)` if it's rejected,
    /// or an error if filtering fails.
    #[inline]
    // TODO: for now we assume that `BodyFilter::InternalIncoming` never used
    pub fn filter_request_by_body(
        body_filters: &[BodyFilter],
        from: &SocketAddr,
        body: &[u8],
    ) -> anyhow::Result<bool> {
        debug!(
            "Filtering request body with {} filters, body size: {} bytes",
            body_filters.len(),
            body.len()
        );

        for (i, filter) in body_filters.iter().enumerate() {
            match filter.filter(from, body) {
                Ok(passed) => {
                    debug!("Body filter {} result: {}", i, passed);
                    if !passed {
                        return Ok(false);
                    }
                }
                Err(e) => {
                    error!("Body filter {} error: {}", i, e);
                    return Err(e);
                }
            }
        }
        debug!("All body filters passed");
        Ok(true)
    }

    //pub fn filters_body(&self) -> bool {
    //    self.body_filters.len() > 0
    //}

    /// Filters requests sequentially using all configured header filters.
    ///
    /// This method processes filters one by one, stopping at the first filter that
    /// doesn't match. It's used when there are few filters (≤5) for better performance.
    ///
    /// # Arguments
    ///
    /// * `service` - Reference to the service containing the filters
    /// * `header` - The HTTP request header parts to filter
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if all filters pass, `Ok(false)` if any filter fails,
    /// or an error if filtering fails.
    fn filter_sequential_header(
        service: &Service,
        from: &SocketAddr,
        header: &Parts,
    ) -> anyhow::Result<bool> {
        debug!(
            "Running sequential header filtering with {} filters",
            service.filters.len()
        );

        for (i, filter) in service.filters.iter().enumerate() {
            match filter.filter(from, header) {
                Ok(passed) => {
                    debug!("Sequential filter {} result: {}", i, passed);
                    if !passed {
                        return Ok(false);
                    }
                }
                Err(e) => {
                    error!("Sequential filter {} error: {}", i, e);
                    return Err(e);
                }
            }
        }
        debug!("All sequential filters passed");
        Ok(true)
    }

    /// Filters requests in parallel using all configured header filters.
    ///
    /// This method processes filters in parallel using rayon, which is more efficient
    /// when there are many filters (>5). It stops at the first filter that doesn't match.
    ///
    /// # Arguments
    ///
    /// * `service` - Reference to the service containing the filters
    /// * `header` - The HTTP request header parts to filter
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if all filters pass, `Ok(false)` if any filter fails,
    /// or an error if filtering fails.
    fn filter_parallel_header(
        service: &Service,
        from: &SocketAddr,
        header: &Parts,
    ) -> anyhow::Result<bool> {
        debug!(
            "Running parallel header filtering with {} filters",
            service.filters.len()
        );

        let result = service
            .filters
            .par_iter()
            .find_map_any(|filter| match filter.filter(from, header) {
                Ok(f) => {
                    if f {
                        None
                    } else {
                        Some(())
                    }
                }
                Err(_) => Some(()),
            })
            .is_none();

        debug!("Parallel filter result: {}", result);
        Ok(result)
    }

    /// Processes an HTTP request through this service.
    ///
    /// This method handles the complete request processing pipeline, including
    /// filtering, middleware application, and upstream forwarding. The specific
    /// processing strategy is automatically selected based on the service configuration.
    ///
    /// # Arguments
    ///
    /// * `upstream` - The upstream server configuration to forward requests to
    /// * `header` - The HTTP request header parts
    /// * `body` - The incoming HTTP body stream
    ///
    /// # Returns
    ///
    /// Returns a future that resolves to the HTTP response from the upstream server.
    #[inline]
    pub fn process(
        &self,
        upstream: Upstream,
        from: &SocketAddr,
        header: http::request::Parts,
        body: Incoming,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>>
                + Send,
        >,
    > {
        (self._process)(self, upstream, from, header, body)
    }

    fn process_without_body_without_middleware(
        service: &Service,
        upstream: Upstream,
        from: &SocketAddr,
        header: http::request::Parts,
        body: Incoming,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>>
                + Send,
        >,
    > {
        debug!(
            "Processing request without body and without middleware to upstream: {:?}",
            upstream
        );

        Self::process_without_body_internal(upstream, header, body)
    }

    fn process_without_body_with_middleware(
        service: &Service,
        upstream: Upstream,
        from: &SocketAddr,
        mut header: http::request::Parts,
        body: Incoming,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>>
                + Send,
        >,
    > {
        debug!(
            "Processing request without body and without middleware to upstream: {:?}",
            upstream
        );

        let middleware = unsafe { service.middleware.clone().unwrap_unchecked() };
        debug!("Applying middleware to request");
        if let Err(e) = middleware.process_incoming(from, &mut header, None) {
            error!("Middleware processing error: {}", e);
            return Box::pin(async {
                let mut response = Response::new(
                    Empty::<Bytes>::new()
                        .map_err(|never| match never {})
                        .boxed(),
                );
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                Ok(response)
            });
        }
        debug!("Middleware processing completed successfully");

        Self::process_without_body_with_middleware_internal(
            middleware, upstream, from, header, body,
        )
    }

    #[inline(always)]
    fn process_without_body_internal(
        upstream: Upstream,
        header: http::request::Parts,
        body: Incoming,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>>
                + Send,
        >,
    > {
        Box::pin(async move {
            debug!("Connecting to upstream: {}", upstream.address);
            let stream = match TcpStream::connect(upstream.address).await {
                Ok(stream) => {
                    debug!("Successfully connected to upstream");
                    stream
                }
                Err(e) => {
                    error!("Failed to connect to upstream {}: {}", upstream.address, e);
                    return Err(e.into());
                }
            };

            let io = HyperSocket::from(stream);

            debug!("Performing HTTP handshake");
            let (mut sender, conn) = match Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .handshake(io)
                .await
            {
                Ok(result) => {
                    debug!("HTTP handshake successful");
                    result
                }
                Err(e) => {
                    error!("HTTP handshake failed: {}", e);
                    return Err(e.into());
                }
            };

            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    error!("Connection error: {}", err);
                }
            });

            let request = Request::from_parts(header, body);
            debug!("Sending request to upstream");

            let (header, body) = match sender.send_request(request).await {
                Ok(response) => {
                    debug!("Request sent successfully, received response");
                    response.into_parts()
                }
                Err(e) => {
                    error!("Failed to send request: {}", e);
                    return Err(e.into());
                }
            };

            let response = Response::from_parts(header, body.boxed());
            debug!("Response created successfully");
            return Ok(response);
        })
    }

    #[inline(always)]
    fn process_without_body_with_middleware_internal(
        middleware: Middleware,
        upstream: Upstream,
        from: &SocketAddr,
        header: http::request::Parts,
        body: Incoming,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>>
                + Send,
        >,
    > {
        let from = from.clone();
        Box::pin(async move {
            debug!("Connecting to upstream: {}", upstream.address);
            let stream = match TcpStream::connect(upstream.address).await {
                Ok(stream) => {
                    debug!("Successfully connected to upstream");
                    stream
                }
                Err(e) => {
                    error!("Failed to connect to upstream {}: {}", upstream.address, e);
                    return Err(e.into());
                }
            };

            let io = HyperSocket::from(stream);

            debug!("Performing HTTP handshake");
            let (mut sender, conn) = match Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .handshake(io)
                .await
            {
                Ok(result) => {
                    debug!("HTTP handshake successful");
                    result
                }
                Err(e) => {
                    error!("HTTP handshake failed: {}", e);
                    return Err(e.into());
                }
            };

            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    error!("Connection error: {}", err);
                }
            });

            let request = Request::from_parts(header, body);
            debug!("Sending request to upstream");

            let (mut header, body) = match sender.send_request(request).await {
                Ok(response) => {
                    debug!("Request sent successfully, received response");
                    response.into_parts()
                }
                Err(e) => {
                    error!("Failed to send request: {}", e);
                    return Err(e.into());
                }
            };

            debug!("Applying middleware to response");
            if let Err(e) = middleware.process_outgoing(&from, &mut header, None) {
                error!("Middleware processing error: {}", e);
                let mut response = Response::new(
                    Empty::<Bytes>::new()
                        .map_err(|never| match never {})
                        .boxed(),
                );
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                return Ok(response);
            }
            debug!("Middleware processing completed successfully");

            let response = Response::from_parts(header, body.boxed());
            debug!("Response created successfully");
            return Ok(response);
        })
    }

    fn process_with_body(
        service: &Service,
        upstream: Upstream,
        from: &SocketAddr,
        mut header: http::request::Parts,
        body: Incoming,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>>
                + Send,
        >,
    > {
        debug!("Processing request with body to upstream: {:?}", upstream);

        let middleware = service.middleware.clone();
        let body_filters = service.get_body_filters_raw();
        let not_found_body_response = service.not_found_body_response.clone();
        let from = from.clone();
        Box::pin(async move {
            let middleware = unsafe { middleware.unwrap_unchecked() };
            let body_filters = body_filters;
            let body_filters =
                unsafe { std::slice::from_raw_parts(body_filters.filters, body_filters.len) };

            debug!("Collecting request body");
            // NOTE: we won't be always recieving full body here
            let mut entire_body = match body.collect().await {
                Ok(collected) => {
                    let bytes = collected.to_bytes().to_vec();
                    debug!("Collected body of {} bytes", bytes.len());
                    bytes
                }
                Err(e) => {
                    error!("Failed to collect request body: {}", e);
                    return Err(e.into());
                }
            };

            debug!("Applying body filters");
            if !Service::filter_request_by_body(body_filters, &from, &entire_body)? {
                if let Some(not_found_body_response) = not_found_body_response {
                    warn!("Request body not filtered, returning specified response");
                    return Ok(not_found_body_response());
                } else {
                    warn!("Request body not filtered, returning FORBIDDEN");
                    let mut response = Response::new(
                        Empty::<Bytes>::new()
                            .map_err(|never| match never {})
                            .boxed(),
                    );
                    *response.status_mut() = StatusCode::FORBIDDEN;
                    return Ok(response);
                }
            }

            debug!("Connecting to upstream: {}", upstream.address);
            let stream = match TcpStream::connect(upstream.address).await {
                Ok(stream) => {
                    debug!("Successfully connected to upstream");
                    stream
                }
                Err(e) => {
                    error!("Failed to connect to upstream {}: {}", upstream.address, e);
                    return Err(e.into());
                }
            };

            let io = HyperSocket::from(stream);

            debug!("Performing HTTP handshake");
            let (mut sender, conn) = match Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .handshake(io)
                .await
            {
                Ok(result) => {
                    debug!("HTTP handshake successful");
                    result
                }
                Err(e) => {
                    error!("HTTP handshake failed: {}", e);
                    return Err(e.into());
                }
            };

            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    error!("Connection error: {}", err);
                }
            });

            debug!("Applying middleware to request with body");
            if let Err(e) = middleware.process_incoming(&from, &mut header, Some(&mut entire_body))
            {
                error!("Middleware processing error: {}", e);
                let mut response = Response::new(
                    Empty::<Bytes>::new()
                        .map_err(|never| match never {})
                        .boxed(),
                );
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                return Ok(response);
            };
            debug!("Middleware processing completed successfully");

            let request = Request::from_parts(header, Full::<Bytes>::from(entire_body));
            debug!("Sending request with body to upstream");

            let (mut header, body) = match sender.send_request(request).await {
                Ok(response) => {
                    debug!("Request sent successfully, received response");
                    response.into_parts()
                }
                Err(e) => {
                    error!("Failed to send request: {}", e);
                    return Err(e.into());
                }
            };

            // NOTE: we won't be always recieving full body here
            let mut entire_body = match body.collect().await {
                Ok(collected) => {
                    let bytes = collected.to_bytes().to_vec();
                    debug!("Collected body of {} bytes", bytes.len());
                    bytes
                }
                Err(e) => {
                    error!("Failed to collect response body: {}", e);
                    return Err(e.into());
                }
            };

            debug!("Applying middleware to response with body");
            if let Err(e) = middleware.process_outgoing(&from, &mut header, Some(&mut entire_body))
            {
                error!("Middleware processing error: {}", e);
                let mut response = Response::new(
                    Empty::<Bytes>::new()
                        .map_err(|never| match never {})
                        .boxed(),
                );
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                return Ok(response);
            };
            debug!("Middleware processing completed successfully");

            let response = Response::from_parts(
                header,
                Full::<Bytes>::from(entire_body)
                    .map_err(|never| match never {})
                    .boxed(),
            );
            debug!("Response created successfully");
            return Ok(response);
        })
    }
}

/// A collection of services that can be used to route HTTP requests.
///
/// Service bundles are used by the HTTP server to determine which service
/// should handle an incoming request. They iterate through all services
/// and use the first one that matches the request criteria.
#[derive(Debug, Clone)]
pub struct ServiceBundle {
    /// Raw pointer to the array of services for FFI safety
    services: *const [Service],

    pub from: SocketAddr,
}

// SAFETY: This is safe because Service is Send and Sync
unsafe impl Sync for ServiceBundle {}
// SAFETY: This is safe because Service is Send and Sync
unsafe impl Send for ServiceBundle {}

impl ServiceBundle {
    /// Creates a new service bundle from an array of services.
    ///
    /// This method initializes a new `ServiceBundle` that can be used to route
    /// requests to multiple services. It logs the number of services being bundled.
    ///
    /// # Arguments
    ///
    /// * `services` - An array of `Service` instances to bundle
    ///
    /// # Returns
    ///
    /// Returns a new `ServiceBundle` instance.
    pub fn new(services: &[Service]) -> Self {
        info!("Creating service bundle with {} services", services.len());
        Self {
            services: services as *const _,
            from: unsafe { SocketAddr::from_str("0.0.0.0:1").unwrap_unchecked() },
        }
    }
}

impl HyperService<hyper::Request<Incoming>> for ServiceBundle {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;

    type Error = anyhow::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    /// Calls the service bundle to process an incoming HTTP request.
    ///
    /// This method iterates through all configured services and attempts to find
    /// the first service that matches the request. It filters the request by header,
    /// checks for large payloads, and then forwards the request to the selected service.
    ///
    /// # Arguments
    ///
    /// * `req` - The incoming HTTP request
    ///
    /// # Returns
    ///
    /// Returns a future that resolves to the HTTP response from the selected service.
    fn call(&self, req: hyper::Request<Incoming>) -> Self::Future {
        let (header, body) = req.into_parts();
        let uri = header.uri.clone();
        let method = header.method.clone();

        debug!("Processing request: {} {}", method, uri);

        for (i, service) in unsafe { &*self.services }.iter().enumerate() {
            debug!("Trying service {} for request", i);

            match service.filter_request_by_header(&self.from, &header) {
                Ok(found) => {
                    if !found {
                        debug!("Service {} did not match request", i);
                        continue;
                    }
                    debug!("Service {} matched request", i);
                }
                Err(e) => {
                    error!("Service {} header filter error: {}", i, e);
                    return Box::pin(async {
                        let mut response = Response::new(
                            Empty::<Bytes>::new()
                                .map_err(|never| match never {})
                                .boxed(),
                        );
                        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                        Ok(response)
                    });
                }
            };

            let max = body.size_hint().upper().unwrap_or(u64::MAX);
            debug!("Request body size hint: {} bytes", max);

            if max > 1024 * 64 {
                warn!(
                    "Request body too large ({} bytes), returning PAYLOAD_TOO_LARGE",
                    max
                );
                return Box::pin(async {
                    let mut response = Response::new(
                        Empty::<Bytes>::new()
                            .map_err(|never| match never {})
                            .boxed(),
                    );
                    *response.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
                    Ok(response)
                });
            }

            let upstream = service.get_upstream();
            debug!("Selected service {} with upstream: {:?}", i, upstream);

            // TODO: REMOVE CLONE
            return service.process(upstream.clone(), &self.from, header, body);
        }

        warn!("No matching service found for request: {} {}", method, uri);
        Box::pin(async {
            let mut response = Response::new(
                Empty::<Bytes>::new()
                    .map_err(|never| match never {})
                    .boxed(),
            );
            *response.status_mut() = StatusCode::NOT_FOUND;
            Ok(response)
        })
    }
}
