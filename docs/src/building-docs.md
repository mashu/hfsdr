# Building this book and API docs

Two outputs serve different purposes:

| Output | Audience | Command |
|--------|----------|---------|
| **This book** (HTML) | Operators + architects | `mdbook build docs` |
| **rustdoc** | Rust API lookup | `cargo doc --no-deps --features gui` |

---

## Build everything

```sh
./scripts/build-docs.sh
```

Produces:

- `docs/book/index.html` — narrative guide (start here)
- `target/doc/hfsdr/index.html` — type reference

Requires [mdBook](https://rust-lang.github.io/mdBook/) 0.4.x:

```sh
cargo install mdbook --locked
```

On Linux, API docs with Airspy need `libairspyhf-dev`. Without it, the script
builds Kiwi-only rustdoc automatically.

---

## Edit the book

1. Add or edit Markdown under `docs/src/`.
2. Link new pages in `docs/src/SUMMARY.md`.
3. Run `mdbook build docs` and fix broken links.

Prefer **explanations and diagrams** over module path lists. Put API details in
rustdoc.

---

## CI

The GitHub **Documentation** job runs `./scripts/build-docs.sh` and uploads
artifacts (`hfsdr-book`, `hfsdr-api-docs`) for each push/PR.

---

## mdBook config

`docs/book.toml` — title, theme, edit URL template pointing at GitHub.

Built HTML is in `.gitignore` (`docs/book/`); CI builds fresh each time.
