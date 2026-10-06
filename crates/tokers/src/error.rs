use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("http: {0}")]
    Http(#[from] wreq::Error),

    /// HTTP 200 with no body: the edge silently dropped the request (bad
    /// signature, rejected identity, or an intermittent drop worth a retry).
    #[error("empty response (HTTP {status}): edge dropped the request")]
    EmptyBody { status: u16 },

    #[error("invalid JSON (HTTP {status}): {source}; body starts with {head:?}")]
    Json {
        status: u16,
        head: String,
        #[source]
        source: serde_json::Error,
    },

    /// The body parsed but did not fit the endpoint's response type.
    #[error("unexpected response shape: {0}")]
    Shape(#[source] serde_json::Error),

    /// `status_code != 0` in the API envelope.
    #[error("api error {code}: {message}")]
    Api { code: i64, message: String },

    /// This build has no request signer (see [`crate::signer::NoSigner`]).
    #[error("this build has no request signer")]
    NoSigner,

    #[error("template: {0}")]
    Template(String),

    #[error("proxy: {0}")]
    Proxy(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Worth re-signing and sending again.
    pub fn is_transient(&self) -> bool {
        matches!(self, Error::Http(_) | Error::EmptyBody { .. } | Error::Json { .. })
    }
}
