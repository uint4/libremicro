//! Validated primitive types shared by the public API.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Result, SUPPORTED_FIRMWARE};

macro_rules! bounded_id {
    ($name:ident, $max:expr, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(u8);

        impl $name {
            /// Creates a validated identifier.
            ///
            /// # Errors
            ///
            /// Returns [`Error::Validation`] when `value` is outside the
            /// range supported by firmware 0.6.2.
            pub fn new(value: u8) -> Result<Self> {
                if value <= $max {
                    Ok(Self(value))
                } else {
                    Err(Error::validation(
                        stringify!($name),
                        format!("{value} is outside 0..={}", $max),
                    ))
                }
            }

            /// Returns the numeric identifier.
            #[must_use]
            pub const fn get(self) -> u8 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = u8::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

bounded_id!(
    AgentId,
    19,
    "Agent-key identifier in the firmware range 0 through 19."
);
bounded_id!(
    ActionId,
    20,
    "Action-key identifier in the firmware range 0 through 20."
);
bounded_id!(
    KeyPosition,
    12,
    "Firmware row-major physical key index in the range 0 through 12."
);

macro_rules! integer_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// Creates an identifier from its on-device numeric value.
            #[must_use]
            pub const fn new(value: u32) -> Self {
                Self(value)
            }

            /// Returns the on-device numeric value.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

integer_id!(
    ProfileId,
    "Stable profile identifier stored in `keymap.json`."
);
integer_id!(LayerId, "Stable layer identifier stored in `keymap.json`.");

/// A 24-bit RGB color encoded as `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RgbColor(u32);

impl RgbColor {
    /// Black (`#000000`).
    pub const BLACK: Self = Self(0);

    /// Creates a color from an integer.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] when bits above `0x00ff_ffff` are set.
    pub fn new(value: u32) -> Result<Self> {
        if value <= 0x00ff_ffff {
            Ok(Self(value))
        } else {
            Err(Error::validation(
                "RGB color",
                format!("0x{value:08x} exceeds 24 bits"),
            ))
        }
    }

    /// Creates a color from red, green, and blue components.
    #[must_use]
    pub const fn from_rgb(red: u8, green: u8, blue: u8) -> Self {
        Self(((red as u32) << 16) | ((green as u32) << 8) | blue as u32)
    }

    /// Returns the packed `0xRRGGBB` representation.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for RgbColor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for RgbColor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "#{:06x}", self.0)
    }
}

macro_rules! unit_interval {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(f32);

        impl $name {
            /// Creates a normalized value.
            ///
            /// # Errors
            ///
            /// Returns [`Error::Validation`] unless `value` is finite and in
            /// the inclusive range `0.0..=1.0`.
            pub fn new(value: f32) -> Result<Self> {
                if value.is_finite() && (0.0..=1.0).contains(&value) {
                    Ok(Self(value))
                } else {
                    Err(Error::validation(
                        stringify!($name),
                        format!("{value} is not a finite value in 0.0..=1.0"),
                    ))
                }
            }

            /// Returns the normalized floating-point value.
            #[must_use]
            pub const fn get(self) -> f32 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = f32::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

unit_interval!(
    Brightness,
    "Normalized LED brightness in the inclusive range 0.0 through 1.0."
);
unit_interval!(
    Speed,
    "Normalized LED animation speed in the inclusive range 0.0 through 1.0."
);

impl Default for Brightness {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Default for Speed {
    fn default() -> Self {
        Self(0.5)
    }
}

/// Firmware version reported by `sys.version`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FirmwareVersion(String);

impl FirmwareVersion {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let normalized = value.strip_prefix('v').unwrap_or(value);
        if normalized == SUPPORTED_FIRMWARE {
            Ok(Self(normalized.to_owned()))
        } else {
            Err(Error::UnsupportedFirmware {
                found: value.to_owned(),
                supported: SUPPORTED_FIRMWARE,
            })
        }
    }

    /// Returns the normalized semantic version without a leading `v`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FirmwareVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Validated root-level firmware file name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct DeviceFileName(String);

impl DeviceFileName {
    /// Creates a device file name.
    ///
    /// # Errors
    ///
    /// Rejects empty names, path separators, traversal components, control
    /// characters, and names longer than 255 bytes.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let invalid = value.is_empty()
            || value.len() > 255
            || value == "."
            || value == ".."
            || value.contains('/')
            || value.contains('\\')
            || value.chars().any(char::is_control);
        if invalid {
            Err(Error::validation(
                "device file name",
                format!("{value:?} is not a safe root-level file name"),
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated file name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn keymap() -> Self {
        Self("keymap.json".to_owned())
    }
}

impl<'de> Deserialize<'de> for DeviceFileName {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for DeviceFileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Keycode string stored in a keymap binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct KeyCode(String);

impl KeyCode {
    /// Creates a validated keycode while allowing firmware-defined values that
    /// are not yet known to this crate.
    ///
    /// # Errors
    ///
    /// Rejects empty strings, control characters, whitespace, and values over
    /// 128 bytes.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let invalid = value.is_empty()
            || value.len() > 128
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace());
        if invalid {
            Err(Error::validation(
                "keycode",
                format!("{value:?} is not a valid firmware keycode"),
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Creates an agent-key keycode such as `KV_OAI_AG03`.
    #[must_use]
    pub fn agent(id: AgentId) -> Self {
        Self(format!("KV_OAI_AG{:02}", id.get()))
    }

    /// Creates an action-key keycode such as `KV_OAI_ACT06`.
    #[must_use]
    pub fn action(id: ActionId) -> Self {
        Self(format!("KV_OAI_ACT{:02}", id.get()))
    }

    /// Returns the firmware keycode text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for KeyCode {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for KeyCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for FirmwareVersion {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}
