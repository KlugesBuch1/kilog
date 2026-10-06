use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("documents folder not found")]
    NoDocuments,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
