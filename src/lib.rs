#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/refcell/cargo-hoist/main/etc/logo.png",
    html_favicon_url = "https://raw.githubusercontent.com/refcell/cargo-hoist/main/etc/favicon.ico",
    issue_tracker_base_url = "https://github.com/refcell/cargo-hoist/issues/"
)]
#![warn(
    missing_debug_implementations,
    missing_docs,
    unreachable_pub,
    rustdoc::all
)]
#![deny(unused_must_use, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

pub mod binaries;
pub mod cli;
pub mod executables;
pub mod project;
pub mod registry;
pub mod shell;
pub mod telemetry;
pub mod utils;

#[doc(inline)]
pub use cli::run;
#[doc(inline)]
pub use cli::Args;
#[doc(inline)]
pub use cli::Command;
