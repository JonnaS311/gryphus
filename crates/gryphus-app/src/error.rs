use gryphus_core::ports::PortError;
use gryphus_core::{DomainError, NameError, PathError};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
}

impl From<NameError> for AppError {
    fn from(e: NameError) -> Self {
        AppError::Domain(e.into())
    }
}

impl From<PathError> for AppError {
    fn from(e: PathError) -> Self {
        AppError::Domain(e.into())
    }
}

impl AppError {
    /// Código estable para la UI (`FEATURE_NOT_FOUND`, `CONFLICT`, …).
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Domain(e) => e.code(),
            AppError::Port(e) => e.code(),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
