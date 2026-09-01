//! Crate-private HID transport boundary.

use std::time::Duration;

use crate::Result;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

pub(crate) trait Transport {
    fn write_report(&mut self, report: &[u8; 64]) -> Result<()>;
    fn read_report(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>>;
}
