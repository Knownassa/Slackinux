use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AppError {
    #[error("window '{0}' not found")]
    WindowNotFound(String),

    #[error("failed to resolve path: {0}")]
    PathResolution(String),

    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("navigation failed: {0}")]
    NavigationFailed(String),

    #[error("Tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("plugin error: {0}")]
    Plugin(String),

    #[error("{0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;
