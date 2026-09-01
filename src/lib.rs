//! Direct, typed access to the stock Work Louder Codex Micro HID interface.
//!
//! `libremicro` provides a blocking, single-owner device API. It deliberately
//! excludes daemon policy, host input injection, application integration, and
//! firmware management.
//!
//! # Example
//!
//! ```no_run
//! use libremicro::Device;
//!
//! # fn main() -> Result<(), libremicro::Error> {
//! let mut device = Device::open_first()?;
//! let status = device.status()?;
//! println!("battery: {}%", status.battery_percent());
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod device;
mod error;
mod event;
mod keymap;
mod lighting;
mod protocol;
mod transport;
mod types;

#[cfg(target_os = "linux")]
pub use device::Device;
pub use device::{DeviceFileInfo, DeviceInfo, DeviceStatus};
pub use error::{Error, Result};
pub use event::{ButtonState, DeviceEvent, EncoderInput, InputControl, InputEvent};
pub use keymap::{
    EncoderControl, JoystickMode, KeymapDocument, KeymapDraft, KeymapIssue, KeymapJoystick,
    KeymapLayer, KeymapLayout, KeymapProfile, KeymapSnapshot, KeymapValidation, KeymapWarning,
    KeymapWritePlan, Revision,
};
#[cfg(feature = "persistent-writes")]
pub use keymap::{KeymapApplyOutcome, RollbackFailure};
pub use lighting::{BaseLighting, LightingEffect, LightingZone, ThreadLighting};
pub use types::{
    ActionId, AgentId, Brightness, DeviceFileName, FirmwareVersion, KeyCode, KeyPosition, LayerId,
    ProfileId, RgbColor, Speed,
};

/// USB vendor identifier used by Work Louder.
pub const VENDOR_ID: u16 = 0x303a;

/// USB product identifier for the Codex Micro supported by this crate.
pub const PRODUCT_ID: u16 = 0x8360;

/// The only firmware version accepted by v1.
pub const SUPPORTED_FIRMWARE: &str = "0.6.2";
