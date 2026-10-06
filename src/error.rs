use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("documents folder not found")]
    NoDocuments,
    #[error("profile response did not include the user")]
    EmptyProfile,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}
