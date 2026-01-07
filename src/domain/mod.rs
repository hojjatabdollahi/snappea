//! Pure domain types with minimal dependencies
//!
//! This module contains core types used throughout the application.
//! Types here should have no framework dependencies (cosmic, iced, etc.)
//! to avoid circular dependencies.

pub mod annotation;
pub mod geometry;
pub mod selection;

pub use annotation::*;
pub use geometry::*;
pub use selection::*;
