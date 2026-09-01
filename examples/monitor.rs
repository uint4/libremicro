//! Prints device-originated events until interrupted.

use std::time::Duration;

use libremicro::Device;

fn main() -> Result<(), libremicro::Error> {
    let mut device = Device::open_first()?;
    eprintln!(
        "monitoring Codex Micro firmware {}; press Ctrl+C to stop",
        device.firmware_version()
    );

    loop {
        if let Some(event) = device.poll_event(Duration::from_millis(250))? {
            println!("{event:?}");
        }
    }
}
