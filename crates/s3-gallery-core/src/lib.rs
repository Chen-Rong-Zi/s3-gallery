#![forbid(unsafe_code)]
#![deny(unreachable_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::let_underscore_must_use)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]

pub mod classify;
pub mod config;
pub mod db;
pub mod error;
pub mod extractor;
pub mod s3;
pub mod scan;
pub mod thumbnail;
pub mod types;
pub mod util;
pub mod view;
