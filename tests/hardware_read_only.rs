//! Explicit-opt-in, read-only checks against an attached Codex Micro.

#![cfg(all(target_os = "linux", feature = "live-hardware-tests"))]

use std::sync::Mutex;

use libremicro::{Device, PRODUCT_ID, SUPPORTED_FIRMWARE, VENDOR_ID};

static HARDWARE_TEST: Mutex<()> = Mutex::new(());

fn opt_in() {
    assert_eq!(
        std::env::var("LIBREMICRO_HARDWARE_TESTS").as_deref(),
        Ok("1"),
        "set LIBREMICRO_HARDWARE_TESTS=1 as well as passing --ignored"
    );
}

#[test]
#[ignore = "requires an attached Codex Micro and explicit read-only opt-in"]
fn discover_supported_device() {
    let _guard = HARDWARE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    opt_in();

    let devices = Device::enumerate()
        .unwrap_or_else(|error| panic!("hardware discovery should succeed: {error}"));

    assert!(
        devices
            .iter()
            .any(|device| { device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID })
    );
}

#[test]
#[ignore = "requires an attached Codex Micro and explicit read-only opt-in"]
fn read_version_and_status() {
    let _guard = HARDWARE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    opt_in();

    let mut device = Device::open_first()
        .unwrap_or_else(|error| panic!("hardware open should succeed: {error}"));
    assert_eq!(device.firmware_version().as_str(), SUPPORTED_FIRMWARE);

    let status = device
        .status()
        .unwrap_or_else(|error| panic!("status read should succeed: {error}"));
    assert_eq!(status.firmware_version().as_str(), SUPPORTED_FIRMWARE);
}

#[test]
#[ignore = "requires an attached Codex Micro and explicit read-only opt-in"]
fn list_device_files() {
    let _guard = HARDWARE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    opt_in();

    let mut device = Device::open_first()
        .unwrap_or_else(|error| panic!("hardware open should succeed: {error}"));
    let files = device
        .list_files()
        .unwrap_or_else(|error| panic!("file listing should succeed: {error}"));

    assert!(
        files
            .iter()
            .any(|file| file.name().as_str() == "keymap.json")
    );
}

#[test]
#[ignore = "requires an attached Codex Micro and explicit read-only opt-in"]
fn read_keymap_snapshot() {
    let _guard = HARDWARE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    opt_in();

    let mut device = Device::open_first()
        .unwrap_or_else(|error| panic!("hardware open should succeed: {error}"));
    let snapshot = device
        .read_keymap()
        .unwrap_or_else(|error| panic!("keymap read should succeed: {error}"));

    assert!(!snapshot.document().profiles().is_empty());
}
