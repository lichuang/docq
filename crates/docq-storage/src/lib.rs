//! SQLite-backed [`Storage`] implementation.
//!
//! [`Storage`]: docq_core::traits::Storage

mod error;
mod sqlite;

pub use sqlite::SqliteStorage;
