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

use crate::{
    filter::{BodyFilter, Filter},
    middleware::Middleware,
    server::HyperSocket,
    upstream::Upstream,
    utils::combine_uris,
};

#[derive(Debug, Clone)]
pub struct Service {
    filters: Vec<Filter>,
    body_filters: Vec<BodyFilter>,
    middleware: Option<Middleware>,
    upstream: Upstream,
    _filter: fn(&Service, header: &Parts) -> anyhow::Result<bool>,
}

impl Service {
    pub fn new(
        filters: Vec<Filter>,
        body_filters: Vec<BodyFilter>,
        middleware: Option<Middleware>,
        upstream: Upstream,
    ) -> Self {
        let amount_of_filters = filters.len();
        Self {
            filters,
            body_filters,
            middleware,
            upstream,
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
        (self._filter)(self, header)
    }

    #[inline]
    // TODO: for now we assume that `BodyFilter::InternalIncoming` never used
    pub fn filter_request_by_body(&self, body: &[u8]) -> anyhow::Result<bool> {
        if self.filters_body() {
            for filter in self.body_filters.iter() {
                if !filter.filter(body)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    pub fn filters_body(&self) -> bool {
        self.body_filters.len() > 0
    }

    /// filter request in sequence
    fn filter_sequential_header(service: &Service, header: &Parts) -> anyhow::Result<bool> {
        for filter in service.filters.iter() {
            if !filter.filter(header)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// filter request in parallel, using rayon
    fn filter_parallel_header(service: &Service, header: &Parts) -> anyhow::Result<bool> {
        Ok(service
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
            .is_none())
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
        let (mut header, mut body) = req.into_parts();

        for service in unsafe { &*self.services }.iter() {
            match service.filter_request_by_header(&header) {
                Ok(found) => {
                    if !found {
                        continue;
                    }
                }
                Err(_) => {
                    // TODO: log error
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
            if max > 1024 * 64 {
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
            return Box::pin(async {
                if !service.filters_body() {
                    let upstream = service.get_upstream();
                    let stream = TcpStream::connect(upstream.address).await.unwrap();
                    let io = HyperSocket::from(stream);

                    let (mut sender, conn) = Builder::new()
                        .preserve_header_case(true)
                        .title_case_headers(true)
                        .handshake(io)
                        .await?;
                    tokio::task::spawn(async move { if let Err(err) = conn.await {} });

                    // TODO: apply middleware

                    let request = Request::from_parts(header, body);

                    let (header, body) = sender.send_request(request).await?.into_parts();

                    let response = Response::from_parts(header, body.boxed());
                    return Ok(response);
                }

                // NOTE: we won't be always recieving full body here
                let mut entire_body = body.collect().await?.to_bytes().to_vec();
                if !service.filter_request_by_body(&entire_body)? {
                    let mut response = Response::new(
                        Empty::<Bytes>::new()
                            .map_err(|never| match never {})
                            .boxed(),
                    );
                    *response.status_mut() = StatusCode::FORBIDDEN;
                    return Ok(response);
                }

                let upstream = service.get_upstream();
                let stream = TcpStream::connect(upstream.address).await.unwrap();
                let io = HyperSocket::from(stream);

                let (mut sender, conn) = Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .handshake(io)
                    .await?;
                tokio::task::spawn(async move { if let Err(err) = conn.await {} });

                let request = Request::from_parts(header, Full::<Bytes>::from(entire_body));

                // TODO: apply middleware

                let (header, body) = sender.send_request(request).await?.into_parts();

                let response = Response::from_parts(header, body.boxed());
                return Ok(response);
            });
        }

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
