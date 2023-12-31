# cargo-hoist 

[![CI Build Status]][actions]
[![Release]][actions]
[![Tag Build Status]][actions]
[![License]][mit-license]
[![Docs]][Docs-rs]
[![Latest Version]][crates.io]
[![rustc 1.70+]][Rust 1.70]

[CI Build Status]: https://img.shields.io/github/actions/workflow/status/refcell/cargo-hoist/ci.yml?branch=main&label=build
[Tag Build Status]: https://img.shields.io/github/actions/workflow/status/refcell/cargo-hoist/tag.yml?branch=main&label=tag
[Release]: https://img.shields.io/github/actions/workflow/status/refcell/cargo-hoist/release.yml?branch=main&label=release
[actions]: https://github.com/refcell/cargo-hoist/actions?query=branch%3Amain
[Latest Version]: https://img.shields.io/crates/v/cargo-hoist.svg
[crates.io]: https://crates.io/crates/cargo-hoist
[rustc 1.70+]: https://img.shields.io/badge/rustc_1.70+-lightgray.svg?label=msrv
[Rust 1.70]: https://blog.rust-lang.org/2023/06/01/Rust-1.70.0.html
[License]: https://img.shields.io/badge/license-MIT-7795AF.svg
[mit-license]: https://github.com/refcell/cargo-hoist/blob/main/LICENSE.md
[Docs-rs]: https://docs.rs/cargo-hoist/
[Docs]: https://img.shields.io/docsrs/cargo-hoist.svg?color=319e8c&label=docs.rs

**Dead simple cargo subcommand to hoist cargo-built binaries into scope.** https://github.com/refcell/cargo-hoist/labels/stable

![](./etc/banner.png)

**[Install](#usage)**
| [User Docs](#what-is-cargo-hoist)
| [Crate Docs][crates.io]
| [Reference][Docs-rs]
| [Contributing](#contributing)
| [License](#license)

## What is cargo-hoist?

`cargo-hoist` is an ultra lightweight, dead simple cargo subcommand that memoizes cargo-built binaries using
a global toml cache file. Since the global toml file contains a memoized list of the built binary paths, the
`hoist` subcommand can then be used to manipulate cargo-built binaries in a whole variety of ways.

Primarily, binaries can be pulled into the current working directory using `cargo hoist <bin name>` (the default,
flagless `hoist` command). To load the binary into path, you can run `cargo hoist <bin name> --path`.

Often, it's added overhead to remember where your binary is built within the `target/..` directories.
`cargo-hoist` makes it easy to find locally built binaries using the `--list` (or `-l` shorthand) flag.

## Usage

Install `cargo-hoist` using cargo.

```text
cargo install cargo-hoist
```

## CLI Flags

Below is a manual output for `v0.1.11`.
To generate a more up-to-date output, run `cargo hoist --help`. 

```text
Dead simple, memoized cargo subcommand to hoist cargo-built binaries into scope.

Usage: cargo hoist [OPTIONS] [COMMAND]

Commands:
  hoist     Hoist dependencies
  list      List registered dependencies
  search    Search for a binary in the hoist toml registry
  nuke      Nuke wipes the hoist toml registry
  register  Registers a binary in the global hoist toml registry
  help      Print this message or the help of the given subcommand(s)

Options:
  -v, --verbosity...  Verbosity level (0-4). Default: 0 (ERROR)
  -q, --quiet         Suppresses standard output
  -h, --help          Print help
  -V, --version       Print version
```

## Contributing

Contributions of all forms are welcome and encouraged!

Please check [existing issues][issues] for similar feature requests or bug reports.

Otherwise, feel free to [open an issue][oissue] if no relevant issue already exists.

[issues]: https://github.com/refcell/cargo-hoist/issues
[oissue]: https://github.com/refcell/cargo-hoist/issues/new


## License

This project is licensed under the [MIT License][mit-license].

Free and open-source, forever. *All our rust are belong to you.*
