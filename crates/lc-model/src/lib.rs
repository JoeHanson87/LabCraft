//! Core data model for LabCraft: types, values, documents and structural validation.
//!
//! The canonical on-disk representation is JSON (one file per VI). Everything an
//! agent needs to author a VI is reachable from [`ViDocument`]; node behavior lives
//! in `lc-nodes`, type checking and execution in `lc-runtime`.

pub mod document;
pub mod types;
pub mod validate;
pub mod value;

pub use document::*;
pub use types::*;
pub use validate::{validate_structure, Diagnostic, Severity};
pub use value::*;

/// Crate-wide error type.
#[derive(Debug, thiserror::Error)]
pub enum LcError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LcError>;

/// Generate a new stable, sortable identifier with the given prefix.
pub fn new_id(prefix: &str) -> String {
    format!(
        "{}_{}",
        prefix,
        ulid::Ulid::new().to_string().to_lowercase()
    )
}
