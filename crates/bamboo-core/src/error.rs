use crate::bus::BusError;

/// Top-level error type for the Bamboo Elf system.
#[derive(Debug, thiserror::Error)]
pub enum BambooError {
    #[error("config: {0}")]
    Config(String),

    #[error("bus: {0}")]
    Bus(#[from] BusError),

    #[error("feed: {0}")]
    Feed(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Convenience result type alias.
pub type BambooResult<T> = Result<T, BambooError>;
