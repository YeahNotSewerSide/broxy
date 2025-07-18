use http::{request, response};
use http_body_util::BodyExt as _;
use hyper::body::Incoming;

#[derive(Debug, Clone)]
pub enum MiddlewareIncomingFunction {
    External,
    InternalWithBody(fn(&mut request::Parts, &mut [u8]) -> anyhow::Result<()>),
    Internal(fn(&mut request::Parts) -> anyhow::Result<()>),
}
impl MiddlewareIncomingFunction {
    #[inline]
    pub fn process(
        &self,
        parts: &mut request::Parts,
        body: &mut Option<&mut [u8]>,
    ) -> anyhow::Result<()> {
        match self {
            MiddlewareIncomingFunction::External => todo!(),
            MiddlewareIncomingFunction::InternalWithBody(func) => {
                if let Some(body) = body {
                    func(parts, body)
                } else {
                    Err(anyhow::anyhow!("No body provided"))
                }
            }
            MiddlewareIncomingFunction::Internal(func) => func(parts),
        }
    }

    pub fn needs_body(&self) -> bool {
        match self {
            MiddlewareIncomingFunction::InternalWithBody(_) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MiddlewareOutgoingFunction {
    External,
    InternalWithBody(fn(&mut response::Parts, &mut [u8]) -> anyhow::Result<()>),
    Internal(fn(&mut response::Parts) -> anyhow::Result<()>),
}
impl MiddlewareOutgoingFunction {
    #[inline]
    pub fn process(
        &self,
        parts: &mut response::Parts,
        body: &mut Option<&mut [u8]>,
    ) -> anyhow::Result<()> {
        match self {
            MiddlewareOutgoingFunction::External => todo!(),
            MiddlewareOutgoingFunction::InternalWithBody(func) => {
                if let Some(body) = body {
                    func(parts, body)
                } else {
                    Err(anyhow::anyhow!("No body provided"))
                }
            }
            MiddlewareOutgoingFunction::Internal(func) => func(parts),
        }
    }

    pub fn needs_body(&self) -> bool {
        match self {
            Self::InternalWithBody(_) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Middleware {
    process_incoming: Vec<MiddlewareIncomingFunction>,
    pub incoming_needs_body: bool,
    process_out: Vec<MiddlewareOutgoingFunction>,
    pub out_needs_body: bool,
}

impl Middleware {
    pub fn new(
        incoming: Vec<MiddlewareIncomingFunction>,
        outgoing: Vec<MiddlewareOutgoingFunction>,
    ) -> Self {
        let incoming_needs_body = !incoming.iter().all(|proc| !proc.needs_body());
        let out_needs_body = !outgoing.iter().all(|proc| !proc.needs_body());
        Self {
            process_incoming: Vec::new(),
            process_out: Vec::new(),
            incoming_needs_body,
            out_needs_body,
        }
    }

    pub async fn process_incoming(
        &self,
        parts: &mut request::Parts,
        incoming: Option<Incoming>,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let mut body: Option<Vec<u8>> = if let Some(incoming) = incoming {
            Some(incoming.collect().await?.to_bytes().to_vec())
        } else {
            None
        };
        let mut body_ref = body.as_mut().map(|body| body.as_mut_slice());
        for proc in &self.process_incoming {
            proc.process(parts, &mut body_ref)?;
        }
        Ok(body)
    }

    pub fn process_outgoing(
        &self,
        parts: &mut response::Parts,
        body: &mut Option<&mut [u8]>,
    ) -> anyhow::Result<()> {
        for proc in &self.process_out {
            proc.process(parts, body)?;
        }
        Ok(())
    }
}
