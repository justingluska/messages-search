//! Messages Search core: the local index, query parser and hybrid search.
//! No Tauri and no chat.db access here (see ms-source).

pub mod embed;
pub mod insights;
pub mod query;
pub mod search;
pub mod storage;
pub mod store;
pub mod types;
pub mod tz;

pub use store::Store;
pub use types::*;
pub use tz::Tz;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests;
