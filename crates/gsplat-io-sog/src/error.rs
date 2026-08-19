//! PlayCanvas SOG errors.

use gsplat_core::ErrorCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SogError {
    #[error("I/O error while reading SOG: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse SOG JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported SOG version {0}")]
    UnsupportedVersion(u32),
    #[error("malformed SOG metadata: {0}")]
    Malformed(&'static str),
    #[error("failed to decode a SOG image: {0}")]
    Image(#[from] image::ImageError),
    #[error("SOG resource limit exceeded for {resource}: requested {requested}, limit {limit}")]
    ResourceLimit {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    #[error("Streamed SOG requires the streaming session API, not whole-scene import")]
    StreamingRequired,
    #[error("SOG chunk decode failed in a worker thread")]
    DecodeJoin,
}

impl PartialEq for SogError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::UnsupportedVersion(a), Self::UnsupportedVersion(b)) => a == b,
            (Self::Malformed(a), Self::Malformed(b)) => a == b,
            (
                Self::ResourceLimit {
                    resource: ra,
                    requested: a,
                    limit: la,
                },
                Self::ResourceLimit {
                    resource: rb,
                    requested: b,
                    limit: lb,
                },
            ) => ra == rb && a == b && la == lb,
            (Self::StreamingRequired, Self::StreamingRequired) => true,
            (Self::DecodeJoin, Self::DecodeJoin) => true,
            _ => false,
        }
    }
}

impl SogError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Io(_) => ErrorCode::NotFound,
            Self::UnsupportedVersion(_) | Self::StreamingRequired | Self::ResourceLimit { .. } => {
                ErrorCode::Unsupported
            }
            Self::Json(_) | Self::Malformed(_) | Self::Image(_) => ErrorCode::ParseFailed,
            Self::DecodeJoin => ErrorCode::Internal,
        }
    }
}
