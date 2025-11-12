use thiserror::Error;

use crate::engine::EngineError;

#[derive(Debug, Error)]
pub enum RouteError {
    #[error("failed to build GPX document: {0}")]
    Gpx(#[from] gpx::errors::GpxError),
    #[error("routing engine error: {0}")]
    Engine(#[from] EngineError),
}
