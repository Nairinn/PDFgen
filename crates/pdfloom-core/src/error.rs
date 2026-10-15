//! Errors for the core crate.

use thiserror::Error;

/// Something went wrong at the object-model or serialization level.
#[derive(Debug, Error)]
pub enum CoreError {
    /// An object was allocated but never given a value.
    #[error("object {id} was allocated but never assigned")]
    Unassigned {
        /// Object number.
        id: u32,
    },
    /// A stream object was placed inside another object (must be indirect).
    #[error("stream objects must be indirect (found inside object {id})")]
    StreamInObject {
        /// Containing object number.
        id: u32,
    },
}
