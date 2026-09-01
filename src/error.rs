//! Public error types.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use crate::keymap::Revision;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by Codex Micro operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Device enumeration failed.
    #[error("device discovery failed: {message}")]
    Discovery {
        /// Human-readable discovery failure.
        message: String,
    },

    /// No supported Codex Micro was connected.
    #[error("no Codex Micro ({vendor_id:04x}:{product_id:04x}) was found")]
    DeviceNotFound {
        /// Expected USB vendor identifier.
        vendor_id: u16,
        /// Expected USB product identifier.
        product_id: u16,
    },

    /// The process does not have permission to open the hidraw node.
    #[error("permission denied opening {path}; check the Codex Micro udev rule")]
    PermissionDenied {
        /// Device node that could not be opened.
        path: PathBuf,
        /// Operating-system error.
        #[source]
        source: io::Error,
    },

    /// An operating-system I/O operation failed.
    #[error("{operation} failed: {source}")]
    Io {
        /// Operation being attempted.
        operation: &'static str,
        /// Operating-system error.
        #[source]
        source: io::Error,
    },

    /// The HID device disconnected or became unusable.
    #[error("Codex Micro disconnected")]
    Disconnected,

    /// A device operation did not complete before its deadline.
    #[error("{operation} timed out after {timeout:?}")]
    Timeout {
        /// Operation that timed out.
        operation: String,
        /// Configured timeout.
        timeout: Duration,
    },

    /// A HID report or JSON message violated the protocol.
    #[error("protocol error: {message}")]
    Protocol {
        /// Description of the malformed input.
        message: String,
    },

    /// The device returned a JSON-RPC error.
    #[error("RPC method {method} failed: {message}")]
    Rpc {
        /// RPC method that failed.
        method: String,
        /// Optional device error code.
        code: Option<i64>,
        /// Device error message.
        message: String,
        /// Optional structured error data.
        data: Option<Box<serde_json::Value>>,
    },

    /// The connected device runs firmware outside the strict v1 boundary.
    #[error("unsupported firmware {found}; libremicro v1 requires {supported}")]
    UnsupportedFirmware {
        /// Version reported by the device.
        found: String,
        /// Version required by this crate.
        supported: &'static str,
    },

    /// Caller input or device configuration failed validation.
    #[error("invalid {context}: {message}")]
    Validation {
        /// Value or document being validated.
        context: &'static str,
        /// Validation failure.
        message: String,
    },

    /// A keymap write plan no longer matches the live device configuration.
    #[error("stale keymap plan: expected revision {expected}, found {actual}")]
    StaleConfiguration {
        /// Revision on which the plan was based.
        expected: Revision,
        /// Current live revision.
        actual: Revision,
    },

    /// A persistent write may have reached the device, but its final state
    /// could not be established.
    #[error("keymap write outcome is indeterminate: {message}")]
    IndeterminateWrite {
        /// Description of the uncertain outcome.
        message: String,
    },
}

impl Error {
    pub(crate) fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol {
            message: message.into(),
        }
    }

    pub(crate) fn validation(context: &'static str, message: impl Into<String>) -> Self {
        Self::Validation {
            context,
            message: message.into(),
        }
    }
}
