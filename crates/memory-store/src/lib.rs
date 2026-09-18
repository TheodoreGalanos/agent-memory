//! Domain repositories and governed artifact/source access.
mod access;
pub mod artifacts;
pub mod coordinator;
mod database;
mod entities;
mod formation;
mod judgements;
mod memories;
pub mod migrate;
mod policies;
mod relations;
pub mod sources;
mod temporal;

pub use database::Store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Resource is unavailable in the assigned scope")]
    NotFound,
    #[error("Write exceeds the assigned scope")]
    Forbidden,
    #[error("Expected revision or state no longer matches")]
    Conflict,
    #[error("Invalid data: {0}")]
    Invalid(String),
    #[error("Source or artifact bytes are unavailable")]
    Unavailable,
    #[error("Operation exceeds its configured limit")]
    LimitExceeded,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Object(#[from] object_store::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

pub mod activation;

pub mod consolidation;

pub mod intentions;
pub mod maintenance;

pub mod retention;

pub mod interaction;
