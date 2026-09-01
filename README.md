# libremicro

`libremicro` is a synchronous Rust library for talking directly to a stock Work Louder Codex Micro. It bypasses Work Louder Input and communicates with the device's vendor HID protocol through Linux `hidraw`.

The crate is intentionally device-only. A future daemon can use it to own process locking, reconnect policy, host key bindings, and virtual input injection.

## Compatibility

| Component | Supported in v1 |
| --- | --- |
| Platform | Linux |
| Connection | USB HID |
| USB identity | `303a:8360` |
| Firmware | `0.6.2` only |
| Runtime | Blocking/synchronous |

Other firmware versions are rejected when the device is opened.

## Quick start

```rust,no_run
use std::time::Duration;

use libremicro::Device;

fn main() -> Result<(), libremicro::Error> {
    let mut device = Device::open_first()?;
    println!("firmware: {}", device.firmware_version());
    println!("status: {:?}", device.status()?);

    if let Some(event) = device.poll_event(Duration::from_millis(250))? {
        println!("event: {event:?}");
    }
    Ok(())
}
```

## Linux permissions

Install a narrow udev rule such as:

```udev
SUBSYSTEM=="hidraw", KERNEL=="hidraw*", ATTRS{idVendor}=="303a", ATTRS{idProduct}=="8360", MODE="0660", GROUP="plugdev", TAG+="uaccess"
```

Reload udev rules and reconnect the device. `Device::open_first` reports permission failures distinctly from discovery failures.

## Capabilities

- Device discovery, strict firmware verification, and status.
- Agent/action/encoder notifications and raw forward-compatible notifications.
- Typed per-key and zone lighting commands.
- No `lights.preview` API: firmware `0.6.2` acknowledges that method without producing a visible effect; base-zone lighting uses the verified `v.oai.rgbcfg` path.
- Read-only device-file listing and text reads.
- Lossless keymap snapshots, warnings, validation, targeted edits, and write-plan generation.
- Optional keymap application with `--features persistent-writes`.
- Explicit-opt-in live hardware suites with `--features live-hardware-tests`.

The crate does not expose arbitrary RPC, firmware flashing, bootloader entry, filesystem deletion/formatting, daemon behavior, or host input injection.

## Persistent-write safety

`persistent-writes` enables only transactional `keymap.json` application. It does not enable generic filesystem writes. An apply checks the live revision, writes once, verifies by reading back, and attempts a best-effort rollback if verification fails.

This is not a complete crash-safety system. The caller must ensure exclusive device ownership and persist the plan's original keymap before applying it. Process locking, durable backup storage, signal recovery, and reconnection belong in the daemon.

## Verification policy

The transport, lighting encodings, and persistent-write state machine are exercised with scripted transports. Live implementation tests on the attached unit are read-only: lighting and device-file writes are intentionally not executed.

Hardware suites are feature-gated and ignored by default. The read-only suite requires three explicit opt-ins:

```sh
LIBREMICRO_HARDWARE_TESTS=1 cargo test --features live-hardware-tests --test hardware_read_only -- --ignored --test-threads=1
```

Run it only with no other process owning the device. It enumerates, reads version/status, lists files, and reads `keymap.json`; it contains no lighting or write calls.

The separate volatile suite tests input events and lighting. It is never run implicitly, requires a different environment opt-in, changes visible LED state, and cleans up by turning affected LEDs off rather than restoring their prior colors. Review [the complete live test plan](docs/LIVE_TESTS.md) before running it.

See [the implementation plan](docs/PLAN.md) for the complete contract.

## License

Licensed under either Apache License 2.0 or the MIT license, at your option.
