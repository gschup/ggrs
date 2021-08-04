# ![GGRS LOGO](./ggrs_logo.png)

[![crates.io](https://img.shields.io/crates/v/ggrs?style=for-the-badge)](https://crates.io/crates/ggrs)
[![docs.rs](https://img.shields.io/docsrs/ggrs?style=for-the-badge)](https://docs.rs/ggrs/newest/ggrs/)
![GitHub Workflow Status](https://img.shields.io/github/workflow/status/gschup/ggrs/Rust?style=for-the-badge)
![GitHub top language](https://img.shields.io/github/languages/top/gschup/ggrs?style=for-the-badge)
[![license](https://img.shields.io/github/license/gschup/ggrs?style=for-the-badge)](./LICENSE)

## P2P Rollback Networking in Rust

GGRS (good game rollback system) is a reimagination of the [GGPO network SDK](https://www.ggpo.net/) written in 100% safe [Rust 🦀](https://www.rust-lang.org/). The callback-style API from the original library has been replaced with a much saner, simpler control flow. Instead of registering callback functions, GGRS returns a list of requests for the user to fulfill.

If you are interested in integrating rollback networking into your game or just want to chat with other rollback developers (not limited to Rust), check out the [GGPO Developers Discord](https://discord.com/invite/8FKKhCRCCE)!

## Getting Started

To get started with GGRS, check out the following resources:

- [Website](https://gschup.github.io/ggrs/)
- [Tutorial](https://gschup.github.io/ggrs/docs/getting-started/quick-start/)
- [Documentation](https://docs.rs/ggrs/newest/ggrs/)
- [Examples](./examples/README.md)

## Development Status

GGRS is in an early stage, but the main functionality for two players and spectators should be quite stable. See the Changelog for the latest changes, even those yet unreleased on crates.io! If you want to contribute, check out the Issues!

- [Changelog](./CHANGELOG.md)
- [Issues](https://github.com/gschup/ggrs/issues)
- [Contribution Guide](https://gschup.github.io/ggrs/docs/contributing/how-to-contribute/)

## Other Rollback Implementations in Rust

Also take a look at the awesome [backroll-rs](https://github.com/HouraiTeahouse/backroll-rs/)!

## Licensing

Just like the original GGPO, GGRS is available under The MIT License. This means GGRS is free for commercial and non-commercial use. Attribution is not required, but appreciated.
