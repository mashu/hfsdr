# Building hfsdr

Requires **Rust 1.85+**. The GUI binary is `hfsdr` (`cargo build --features gui --bin hfsdr`).

| Feature | Purpose |
|---------|---------|
| `gui` | Full desktop app (eframe, cpal, RTL-SDR) |
| `gui-core` | GUI without RTL-SDR link (Kiwi + QMX only) |
| `airspy` | Airspy HF+ (default feature) |
| `rtlsdr` | RTL-SDR (included in `gui`) |
| `qmx` | QRP Labs QMX/QMX+ (included in `gui-core`) |

---

## Linux (Debian / Ubuntu)

```sh
sudo apt install build-essential pkg-config \
  libairspyhf-dev librtlsdr-dev \
  libasound2-dev libxkbcommon-dev libwayland-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev

cargo build --release --features gui --bin hfsdr
```

---

## macOS

```sh
brew install airspyhf librtlsdr pkg-config
cargo build --release --features gui --bin hfsdr
```

---

## Windows

**KiwiSDR** and **QMX** need no native USB SDR libraries:

```powershell
cargo build --release --no-default-features --features gui-core --bin hfsdr
```

**Airspy HF+** and **RTL-SDR** need `libairspyhf` and `librtlsdr` (not shipped with
the repo). Prerequisites: Rust (MSVC toolchain), CMake, Git, Python 3.

```powershell
pwsh scripts/install-windows-sdr-deps.ps1
cargo build --release --features gui --bin hfsdr
```

The install script uses [vcpkg](https://github.com/microsoft/vcpkg) for RTL-SDR
(libusb, pthreads) and builds [libairspyhf](https://github.com/airspy/airspyhf)
into a local prefix. It sets `PKG_CONFIG_PATH`, `VCPKG_ROOT`, and
`HFSDR_DEPS_PREFIX` for the current shell — re-run it in new terminals before
building.

[Release builds](https://github.com/mashu/hfsdr/releases) include `hfsdr.exe` and
the required `.dll` files in the zip.

---

## Documentation

```sh
cargo install mdbook --locked
./scripts/build-docs.sh
```

Book output: `docs/book/index.html`. See [Building this book](./building-docs.md).
