//! Shared types, traits, and error types for docq.

pub mod error;
pub mod models;
pub mod traits;

pub use error::{DocqError, Result};
pub use models::*;
pub use traits::*;
