use std::pin::Pin;

use http::request::Parts;
use hyper::body::Incoming;

pub type FilterBody = unsafe extern "C" fn(*const u8, u64) -> bool;

#[derive(Debug, Clone)]
pub enum Filter {
    Method(hyper::Method),
    Host(regex::Regex),
    Path(regex::Regex),
    //Body(libloading::Symbol<'static, FilterBody>),
}

impl Filter {
    pub fn filter(&self, header: &Parts) -> anyhow::Result<bool> {
        Ok(match self {
            Filter::Method(method) => header.method.eq(method),
            Filter::Host(host_regex) => host_regex.is_match(
                header
                    .uri
                    .host()
                    .ok_or(anyhow::anyhow!("Host is empty: {:?}", header))?,
            ),
            Filter::Path(path_regex) => path_regex.is_match(header.uri.path()),
        })
    }
}

#[derive(Debug, Clone)]
pub enum BodyFilter {
    InternalIncoming(
        fn(Incoming) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<Vec<u8>>>> + Send>>,
    ),
    InternalFullBody(fn(&[u8]) -> anyhow::Result<bool>),
    External,
}
impl BodyFilter {
    pub fn filter(&self, body: &[u8]) -> anyhow::Result<bool> {
        match self {
            BodyFilter::InternalFullBody(func) => func(body),
            BodyFilter::External => unimplemented!(),
            BodyFilter::InternalIncoming(_) => {
                return Err(anyhow::anyhow!("Expected to be called by `filter_async`"));
            }
        }
    }

    pub async fn filter_async(&self, incoming: Incoming) -> anyhow::Result<Option<Vec<u8>>> {
        if let Self::InternalIncoming(filter_incoming) = self {
            filter_incoming(incoming).await
        } else {
            Err(anyhow::anyhow!("Expected to be called by `filter`"))
        }
    }

    #[inline]
    pub fn use_async(&self) -> bool {
        if let Self::InternalIncoming(_) = self {
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct BodyFilters {
    pub filters: *const BodyFilter,
    pub len: usize,
}
unsafe impl Send for BodyFilters {}
unsafe impl Sync for BodyFilters {}
