# Native-Rust USB, and USB in the browser

Notes from the spike behind `src/usb/`.

## Why not statically link SoapySDR

SoapySDR is itself a plugin loader, so linking it statically does not remove
dlopen — it removes the drivers:

```
$ nm -D /lib/x86_64-linux-gnu/libSoapySDR.so.0.8 | grep dlopen
                 U dlopen@GLIBC_2.34
```

`libSoapySDR` contains no device support at all. Every device lives in a
separate module it opens at runtime (13 of them on a stock Debian install:
`librtlsdrSupport.so`, `libairspySupport.so`, `libuhdSupport.so`, …). Linking
the modules statically as well is possible — `--whole-archive` so the registrar
constructors survive — but each one drags in its vendor library:

```
$ ldd modules0.8/librtlsdrSupport.so
    librtlsdr.so.2 → libusb-1.0.so.0 → libudev.so.1 → libcap.so.2
```

That means cross-compiling that C stack for three platforms in CI, and a
licence question for each library. It also can never target wasm32.

## What this does instead

`nusb` is a pure-Rust USB implementation, so a driver written on it links
statically with nothing underneath:

```
$ ldd target/debug/examples/usb_probe
    libgcc_s.so.1
    libc.so.6
```

No libusb, no libudev, no vendor library, nothing to install beside the binary.

## The shape

The protocol is written once as values, and only the transport is written
twice. This is the same split as `kiwi::session`, for the same reason.

```
                 usb::ControlRequest        ← described, not performed
                          │
        ┌─────────────────┴─────────────────┐
   usb::nusb_transport              WebUSB (browser)
   (native, static)                 web-sys, not yet written
        └─────────────────┬─────────────────┘
                          │
                  usb::airspyhf              ← commands, replies, samples
                  (no USB API at all)           tested with no radio present
```

`usb::airspyhf` names no USB crate and opens no device, so it compiles for
wasm32 unchanged and its tests run on the host. The parts most likely to be
wrong — command numbers, the byte order of the serial, I/Q order, the fact
that a sample rate is selected *by index* because the field is 16 bits and the
rates are not, and the order of the open sequence — are all in there.

## WebUSB in a worker

The browser engine shares the render thread (wgpu and wasm atomics are mutually
exclusive upstream — see `engine/link.rs`), so a USB stream would compete with
rendering unless it runs in a worker. Measured in Chromium 143 over https:

| context | `navigator.usb` | `getDevices()` | `requestDevice()` |
|---|---|---|---|
| Window | present | present | present |
| DedicatedWorker | present | present | **undefined** |

So the split is forced, and it is the right one anyway: the window calls
`requestDevice()` behind a user gesture and gets the chooser; the worker calls
`getDevices()` and does the streaming. The permission is per-origin and
persists, so the chooser appears once rather than every visit.

Not yet verified, and needing hardware: that a worker can *open* a device it
got from `getDevices()` rather than merely see it.

## Known constraints

- **Chromium only.** Firefox and Safari have both filed negative standards
  positions on WebUSB; neither is going to ship it.
- **Secure context required** — the opposite of the KiwiSDR case, where https
  is what blocks a plain-ws socket. Here https is what enables WebUSB.
- **OS setup is unchanged from native**: a udev rule on Linux, WinUSB on
  Windows. WebUSB does not avoid either.

## Not done

Streaming. The bulk endpoint, transfer geometry and sample decoding are
defined and tested (`BULK_IN_ENDPOINT`, `TRANSFER_BYTES`, `decode_samples`),
but the queue of in-flight transfers feeding an `IqSource` ring is not written.
It cannot be verified here — this container has no USB subsystem at all:

```
$ cargo run --example usb_probe --features airspyhf-usb
could not list USB devices: /sys/bus/usb/devices/ not found (errno 2)
```

That is the point at which real hardware is needed.
