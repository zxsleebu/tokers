//! Request signing is pluggable: this crate only defines the interfaces. A build
//! without a real implementation uses [`NoSigner`], and every API call fails with
//! [`Error::NoSigner`].

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::template::{PreparedRequest, RequestTemplate};

/// Signature headers (X-Argus, X-Ladon, ...) for one mobile request.
pub trait Signer: Send + Sync {
    fn sign(&self, req: &PreparedRequest, body: &[u8]) -> Result<Vec<(String, String)>>;
}

/// Web API query signing: returns `query` with its signature parameter appended.
pub trait WebSigner: Send + Sync {
    fn sign_query(&self, query: &str, user_agent: &str, body: &str, timestamp: u32) -> Result<String>;
}

/// Placeholder for builds that ship without a signer.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSigner;

impl Signer for NoSigner {
    fn sign(&self, _: &PreparedRequest, _: &[u8]) -> Result<Vec<(String, String)>> {
        Err(Error::NoSigner)
    }
}

impl WebSigner for NoSigner {
    fn sign_query(&self, _: &str, _: &str, _: &str, _: u32) -> Result<String> {
        Err(Error::NoSigner)
    }
}

/// Everything a front end needs to talk to the API: the request template and
/// the signers. Built by whoever owns the signing implementation and handed to
/// the UI.
#[derive(Clone)]
pub struct Backend {
    pub template: RequestTemplate,
    pub signer: Arc<dyn Signer>,
    pub web_signer: Arc<dyn WebSigner>,
}

impl Backend {
    pub fn new(template: RequestTemplate, signer: Arc<dyn Signer>, web_signer: Arc<dyn WebSigner>) -> Self {
        Backend { template, signer, web_signer }
    }

    /// A backend that builds requests but cannot sign them.
    pub fn unsigned() -> Self {
        Backend {
            template: RequestTemplate::placeholder(),
            signer: Arc::new(NoSigner),
            web_signer: Arc::new(NoSigner),
        }
    }

    /// A mobile client builder preset with this backend's template and signer.
    pub fn tiktok(&self) -> crate::TikTokBuilder {
        crate::TikTok::builder(self.template.clone(), self.signer.clone())
    }
}
