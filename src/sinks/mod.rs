//! Ready-made [`TagSink`](crate::TagSink) implementations.
//!
//! Enough to run a collector without writing one. Implement the trait, or pass
//! a closure, for anything else.

mod log_sink;

#[cfg(feature = "jsonl-sink")]
mod jsonl;

pub use log_sink::LogSink;

#[cfg(feature = "jsonl-sink")]
pub use jsonl::JsonlSink;
