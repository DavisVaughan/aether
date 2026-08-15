//! The markdown corpus harness
//!
//! Library code rather than a test module, because `xtask accept` needs
//! [`Document`] and [`check()`] too and cannot reach into another crate's
//! `tests/` directory.
//!
//! This harness is for the parser only. The lexer keeps its own `#[cfg(test)]`
//! suite inside `aether_r_parser`, where the tests can read private fields and
//! a token stream is small enough to assert exactly.

pub mod check;
pub mod corpus;
pub mod document;
pub mod manifest;
pub mod runner;

pub use crate::check::check;
pub use crate::document::{Block, Cell, Document, DocumentError, Element, Markers, TestCase};
pub use crate::manifest::Manifest;
pub use crate::runner::run_file;
