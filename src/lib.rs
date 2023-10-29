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

#[doc(inline)]
pub use cli::run;

/// The CLI module.
pub mod cli;
