//! Demonstrates the two verified volatile-lighting APIs with explicit opt-in.

use std::thread;
use std::time::Duration;

use libremicro::{
    AgentId, BaseLighting, Brightness, Device, LightingEffect, LightingZone, RgbColor,
    ThreadLighting,
};

const OPT_IN: &str = "LIBREMICRO_LIGHTING_DEMO";
const DISPLAY_TIME: Duration = Duration::from_secs(3);

fn main() -> Result<(), libremicro::Error> {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!(
            "refusing to change LEDs; run with {OPT_IN}=1 after stopping other device owners"
        );
        return Ok(());
    }

    let mut device = Device::open_first()?;
    eprintln!(
        "displaying volatile lighting for three seconds; cleanup turns affected LEDs off and cannot restore previous colors"
    );

    let operation = display_lighting(&mut device);
    let cleanup = clear_lighting(&mut device);
    if let Err(error) = &cleanup {
        eprintln!("lighting cleanup failed: {error}");
    }
    operation?;
    cleanup
}

fn display_lighting(device: &mut Device) -> Result<(), libremicro::Error> {
    device.set_base_lighting(
        BaseLighting::new()
            .with_keys(LightingZone::new(
                LightingEffect::Solid,
                RgbColor::from_rgb(0x00, 0xff, 0xff),
            ))
            .with_ambient(LightingZone::new(
                LightingEffect::Solid,
                RgbColor::from_rgb(0x80, 0x00, 0xff),
            )),
    )?;

    let agent = ThreadLighting::new(AgentId::new(0)?, RgbColor::from_rgb(0xff, 0x40, 0x00));
    device.set_thread_lighting(&[agent])?;
    thread::sleep(DISPLAY_TIME);
    Ok(())
}

fn clear_lighting(device: &mut Device) -> Result<(), libremicro::Error> {
    let brightness = Brightness::new(0.0)?;
    let agent = ThreadLighting::new(AgentId::new(0)?, RgbColor::BLACK)
        .with_effect(LightingEffect::Off)
        .with_brightness(brightness);
    let agent_cleanup = device.set_thread_lighting(&[agent]);

    let off = LightingZone::new(LightingEffect::Off, RgbColor::BLACK).with_brightness(brightness);
    let base_cleanup =
        device.set_base_lighting(BaseLighting::new().with_keys(off).with_ambient(off));

    agent_cleanup?;
    base_cleanup
}
