//! buildscope-core: pure, IO-free analysis of a Buildroot output tree.
//!
//! The core consumes a [`snapshot::Snapshot`] (built by whichever frontend
//! has filesystem access: the native CLI, a browser file list, a test) and
//! produces a [`report::Report`]. Nothing in this crate reads files, so the
//! same code runs natively and under WASM, and every analysis is trivially
//! testable with synthetic snapshots.

pub mod analyze;
pub mod carve;
pub mod crc;
pub mod diff;
pub mod inputs;
pub mod parsers;
pub mod report;
pub mod snapshot;

pub const GENERATOR_NAME: &str = "buildscope";
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");
