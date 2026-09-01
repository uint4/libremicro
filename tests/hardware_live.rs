//! Explicit-opt-in acceptance tests that change volatile device state.

#![cfg(all(target_os = "linux", feature = "live-hardware-tests"))]

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use libremicro::{
    AgentId, BaseLighting, Brightness, Device, DeviceEvent, Error, InputControl, LightingEffect,
    LightingZone, RgbColor, ThreadLighting,
};

const OBSERVATION_TIME: Duration = Duration::from_secs(30);
const DISPLAY_TIME: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

static HARDWARE_TEST: Mutex<()> = Mutex::new(());

fn opt_in() {
    assert_eq!(
        std::env::var("LIBREMICRO_LIVE_TESTS").as_deref(),
        Ok("1"),
        "set LIBREMICRO_LIVE_TESTS=1 as well as enabling the feature and passing --ignored"
    );
}

fn open_device() -> Device {
    opt_in();
    Device::open_first().unwrap_or_else(|error| panic!("hardware open should succeed: {error}"))
}

fn zero_brightness() -> Brightness {
    Brightness::new(0.0).unwrap_or_else(|error| panic!("zero brightness should be valid: {error}"))
}

fn off_zone() -> LightingZone {
    LightingZone::new(LightingEffect::Off, RgbColor::BLACK).with_brightness(zero_brightness())
}

fn finish_with_cleanup(
    operation: Result<(), Error>,
    cleanup: Result<(), Error>,
) -> Result<(), String> {
    match (operation, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(operation), Ok(())) => Err(format!("live operation failed: {operation}")),
        (Ok(()), Err(cleanup)) => Err(format!(
            "live operation passed, but cleanup failed: {cleanup}"
        )),
        (Err(operation), Err(cleanup)) => Err(format!(
            "live operation failed: {operation}; cleanup also failed: {cleanup}"
        )),
    }
}

fn agent_palette(effect: LightingEffect) -> Vec<ThreadLighting> {
    const COLORS: [RgbColor; 6] = [
        RgbColor::from_rgb(0xff, 0x00, 0x00),
        RgbColor::from_rgb(0xff, 0x80, 0x00),
        RgbColor::from_rgb(0xff, 0xff, 0x00),
        RgbColor::from_rgb(0x00, 0xff, 0x00),
        RgbColor::from_rgb(0x00, 0x40, 0xff),
        RgbColor::from_rgb(0x80, 0x00, 0xff),
    ];

    COLORS
        .iter()
        .copied()
        .enumerate()
        .map(|(index, color)| {
            let id = AgentId::new(index as u8)
                .unwrap_or_else(|error| panic!("agent id should be valid: {error}"));
            ThreadLighting::new(id, color).with_effect(effect)
        })
        .collect()
}

fn agent_cleanup() -> Vec<ThreadLighting> {
    (0_u8..6)
        .map(|index| {
            let id = AgentId::new(index)
                .unwrap_or_else(|error| panic!("agent id should be valid: {error}"));
            ThreadLighting::new(id, RgbColor::BLACK)
                .with_effect(LightingEffect::Off)
                .with_brightness(zero_brightness())
        })
        .collect()
}

#[derive(Debug, Default)]
struct ObservedInputs {
    agent: bool,
    action: bool,
    encoder: bool,
    radial: bool,
}

impl ObservedInputs {
    const fn is_complete(&self) -> bool {
        self.agent && self.action && self.encoder && self.radial
    }

    fn observe(&mut self, event: &DeviceEvent) {
        match event {
            DeviceEvent::Input(input) => match input.control() {
                InputControl::Agent(_) => self.agent = true,
                InputControl::Action(_) => self.action = true,
                InputControl::Encoder(_) => self.encoder = true,
                _ => {}
            },
            DeviceEvent::Radial(_) => self.radial = true,
            _ => {}
        }
    }
}

#[test]
#[ignore = "requires an operator and explicit live-hardware opt-in"]
fn input_stream_should_report_agent_action_encoder_and_radial_events() {
    let _guard = HARDWARE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut device = open_device();
    let mut observed = ObservedInputs::default();
    let deadline = Instant::now() + OBSERVATION_TIME;

    eprintln!(
        "Within 30 seconds: press an Agent key, press an Action key, rotate or press the encoder, and move the joystick."
    );
    while Instant::now() < deadline && !observed.is_complete() {
        if let Some(event) = device
            .poll_event(POLL_INTERVAL)
            .unwrap_or_else(|error| panic!("event polling should succeed: {error}"))
        {
            eprintln!("observed {event:?}");
            observed.observe(&event);
        }
    }

    assert!(
        observed.is_complete(),
        "expected Agent, Action, Encoder, and Radial events; observed {observed:?}"
    );
}

#[test]
#[ignore = "changes Agent-key LEDs and requires explicit live-hardware opt-in"]
fn agent_lighting_should_acknowledge_palette_and_cleanup() {
    let _guard = HARDWARE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut device = open_device();
    let palette = agent_palette(LightingEffect::Solid);

    eprintln!("Agent keys should display red, orange, yellow, green, blue, and purple.");
    let operation = device.set_thread_lighting(&palette);
    if operation.is_ok() {
        thread::sleep(DISPLAY_TIME);
    }
    let cleanup = device.set_thread_lighting(&agent_cleanup());

    finish_with_cleanup(operation, cleanup)
        .unwrap_or_else(|error| panic!("Agent-lighting acceptance test failed: {error}"));
}

#[test]
#[ignore = "changes base LEDs and requires explicit live-hardware opt-in"]
fn base_lighting_should_acknowledge_both_zones_and_cleanup() {
    let _guard = HARDWARE_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut device = open_device();
    let lighting = BaseLighting::new()
        .with_keys(LightingZone::new(
            LightingEffect::Solid,
            RgbColor::from_rgb(0x00, 0xff, 0xff),
        ))
        .with_ambient(LightingZone::new(
            LightingEffect::Solid,
            RgbColor::from_rgb(0x80, 0x00, 0xff),
        ));

    eprintln!("The key backlight should be cyan and the ambient zone purple.");
    let operation = device.set_base_lighting(lighting);
    if operation.is_ok() {
        thread::sleep(DISPLAY_TIME);
    }
    let cleanup = device.set_base_lighting(
        BaseLighting::new()
            .with_keys(off_zone())
            .with_ambient(off_zone()),
    );

    finish_with_cleanup(operation, cleanup)
        .unwrap_or_else(|error| panic!("base-lighting acceptance test failed: {error}"));
}
