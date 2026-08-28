//! Report whether an Airspy HF+ is attached and reachable over plain USB.
//!
//! Run with: `cargo run --example usb_probe --features airspyhf-usb`
//!
//! The failure that matters is permissions, not absence: on Linux the device
//! enumerates for everyone but only opens for a user the udev rules allow, and
//! on Windows it must be bound to WinUSB. Both surface as "access denied" from
//! the OS, so this prints the reason alongside it.

use hfsdr::usb::airspyhf;
use hfsdr::usb::nusb_transport::NusbTransport;

fn main() {
    let transport = match NusbTransport::open(airspyhf::VENDOR_ID, airspyhf::PRODUCT_ID) {
        Ok(t) => t,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    println!(
        "opened {:04x}:{:04x}",
        airspyhf::VENDOR_ID,
        airspyhf::PRODUCT_ID
    );

    match airspyhf::open_sequence(&transport) {
        Ok(rates) => {
            println!("sample rates: {rates:?}");
            println!(
                "highest is {} Sa/s; a bulk transfer is {} samples ({} bytes)",
                rates.iter().max().copied().unwrap_or(0),
                airspyhf::TRANSFER_SAMPLES,
                airspyhf::TRANSFER_BYTES,
            );
        }
        Err(e) => println!("device opened but did not answer: {e}"),
    }
}
