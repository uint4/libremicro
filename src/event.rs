//! Device-originated events.

use serde_json::Value;

use crate::{ActionId, AgentId};

/// Press/release state reported by a physical control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ButtonState {
    /// The control was pressed.
    Pressed,
    /// The control was released.
    Released,
    /// An action value whose semantics are not established. Firmware `0.6.2`
    /// emits value `2` for encoder-rotation notifications.
    Unknown(u8),
}

/// Encoder input reported by the stock agent key bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncoderInput {
    /// Counter-clockwise rotation.
    CounterClockwise,
    /// Clockwise rotation.
    Clockwise,
    /// Encoder press.
    Press,
}

/// Physical or logical control named by a `v.oai.hid` notification.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputControl {
    /// Agent key `AG00` through `AG19`.
    Agent(AgentId),
    /// Action key `ACT00` through `ACT20`.
    Action(ActionId),
    /// Rotary encoder direction or click.
    Encoder(EncoderInput),
    /// An identifier introduced by another firmware version.
    Unknown(String),
}

/// Parsed `v.oai.hid` notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputEvent {
    control: InputControl,
    state: ButtonState,
}

impl InputEvent {
    pub(crate) const fn new(control: InputControl, state: ButtonState) -> Self {
        Self { control, state }
    }

    /// Returns the control that changed.
    #[must_use]
    pub fn control(&self) -> &InputControl {
        &self.control
    }

    /// Returns whether the control was pressed or released.
    #[must_use]
    pub const fn state(&self) -> ButtonState {
        self.state
    }
}

/// Unsolicited message read from the Codex Micro.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DeviceEvent {
    /// Parsed agent/action/encoder input.
    Input(InputEvent),
    /// Raw joystick/radial notification. Its firmware 0.6.2 payload has not
    /// been established sufficiently to expose a stronger type.
    Radial(Value),
    /// Line emitted on the firmware debug channel.
    Debug(String),
    /// Forward-compatible notification whose method is not modeled yet.
    UnknownNotification {
        /// Firmware notification method.
        method: String,
        /// Unmodified notification parameters.
        params: Value,
    },
}
