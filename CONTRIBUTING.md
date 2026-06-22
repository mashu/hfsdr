# Contributing to hfsdr

[![Documentation](https://img.shields.io/badge/docs-mdBook-blue)](docs/src/contributing.md)
[![CI](https://github.com/mashu/hfsdr/actions/workflows/ci.yml/badge.svg)](https://github.com/mashu/hfsdr/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

See the **[full contributing guide](docs/src/contributing.md)** and
**[architecture documentation](docs/src/architecture/code-layout.md)** in the mdBook.

```sh
cargo test --features gui
cargo clippy --features gui -- -D warnings
./scripts/build-docs.sh   # if you changed docs or public API
```

Layer dependencies: `source` → `dsp` / `skimmer` → GUI binary. Do not import egui from library code.
