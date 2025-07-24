use std::pin::Pin;

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
    middleware::Middleware,
    server::HyperSocket,
    upstream::Upstream,
};

type ProcessFunction = fn(
    &Service,
    Upstream,
    http::request::Parts,
    Incoming,
) -> Pin<
    Box<dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>> + Send>,
>;

type BodyNotFoundFunction = fn() -> Response<BoxBody<Bytes, hyper::Error>>;

#[derive(Debug, Clone)]
pub struct Service {
    filters: Vec<Filter>,
    body_filters: Vec<BodyFilter>,
    middleware: Option<Middleware>,
    upstream: Upstream,
    not_found_body_response: Option<BodyNotFoundFunction>,
    _process: ProcessFunction,
    _filter: fn(&Service, header: &Parts) -> anyhow::Result<bool>,
}

impl Service {
    pub fn new(
        filters: Vec<Filter>,
        body_filters: Vec<BodyFilter>,
        middleware: Option<Middleware>,
        upstream: Upstream,
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
            upstream,
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

    pub fn get_upstream(&self) -> &Upstream {
        &self.upstream
    }

    #[inline]
    pub fn filter_request_by_header(&self, header: &Parts) -> anyhow::Result<bool> {
        let result = (self._filter)(self, header);
        match &result {
            Ok(matched) => debug!("Header filter result: {}", matched),
            Err(e) => error!("Header filter error: {}", e),
        }
        result
    }

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

    #[inline]
    // TODO: for now we assume that `BodyFilter::InternalIncoming` never used
    pub fn filter_request_by_body(
        body_filters: &[BodyFilter],
        body: &[u8],
    ) -> anyhow::Result<bool> {
        debug!(
            "Filtering request body with {} filters, body size: {} bytes",
            body_filters.len(),
            body.len()
        );

        for (i, filter) in body_filters.iter().enumerate() {
            match filter.filter(body) {
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

    /// filter request in sequence
    fn filter_sequential_header(service: &Service, header: &Parts) -> anyhow::Result<bool> {
        debug!(
            "Running sequential header filtering with {} filters",
            service.filters.len()
        );

        for (i, filter) in service.filters.iter().enumerate() {
            match filter.filter(header) {
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

    /// filter request in parallel, using rayon
    fn filter_parallel_header(service: &Service, header: &Parts) -> anyhow::Result<bool> {
        debug!(
            "Running parallel header filtering with {} filters",
            service.filters.len()
        );

        let result = service
            .filters
            .par_iter()
            .find_map_any(|filter| match filter.filter(header) {
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

    #[inline]
    pub fn process(
        &self,
        upstream: Upstream,
        header: http::request::Parts,
        body: Incoming,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Response<BoxBody<Bytes, hyper::Error>>, anyhow::Error>>
                + Send,
        >,
    > {
        (self._process)(self, upstream, header, body)
    }

    fn process_without_body_without_middleware(
        service: &Service,
        upstream: Upstream,
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
        if let Err(e) = middleware.process_incoming(&mut header, None) {
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

        Self::process_without_body_with_middleware_internal(middleware, upstream, header, body)
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
            if let Err(e) = middleware.process_outgoing(&mut header, None) {
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
            if !Service::filter_request_by_body(body_filters, &entire_body)? {
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
            if let Err(e) = middleware.process_incoming(&mut header, Some(&mut entire_body)) {
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
            if let Err(e) = middleware.process_outgoing(&mut header, Some(&mut entire_body)) {
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

#[derive(Debug, Clone)]
pub struct ServiceBundle {
    services: *const [Service],
}
unsafe impl Sync for ServiceBundle {}
unsafe impl Send for ServiceBundle {}

impl ServiceBundle {
    pub fn new(services: &[Service]) -> Self {
        info!("Creating service bundle with {} services", services.len());
        Self {
            services: services as *const _,
        }
    }
}

impl HyperService<hyper::Request<Incoming>> for ServiceBundle {
    type Response = Response<BoxBody<Bytes, hyper::Error>>;

    type Error = anyhow::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: hyper::Request<Incoming>) -> Self::Future {
        let (header, body) = req.into_parts();
        let uri = header.uri.clone();
        let method = header.method.clone();

        debug!("Processing request: {} {}", method, uri);

        for (i, service) in unsafe { &*self.services }.iter().enumerate() {
            debug!("Trying service {} for request", i);

            match service.filter_request_by_header(&header) {
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
            return service.process(upstream.clone(), header, body);
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
