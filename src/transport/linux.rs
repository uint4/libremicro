//! Linux `/dev/hidraw*` transport and sysfs enumeration.

use std::fs;
use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{Mode, OFlags, open};

use super::Transport;
use crate::device::DeviceInfo;
use crate::{Error, PRODUCT_ID, Result, VENDOR_ID};

const SYS_HIDRAW: &str = "/sys/class/hidraw";
const DEV_ROOT: &str = "/dev";

pub(crate) struct LinuxHidraw {
    fd: OwnedFd,
}

impl LinuxHidraw {
    pub(crate) fn open(info: &DeviceInfo) -> Result<Self> {
        match open(
            info.path(),
            OFlags::RDWR | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => Ok(Self { fd }),
            Err(error) if matches!(error, rustix::io::Errno::ACCESS | rustix::io::Errno::PERM) => {
                Err(Error::PermissionDenied {
                    path: info.path().to_path_buf(),
                    source: io_error(error),
                })
            }
            Err(rustix::io::Errno::NODEV | rustix::io::Errno::NOENT) => Err(Error::Disconnected),
            Err(error) => Err(Error::Io {
                operation: "open hidraw device",
                source: io_error(error),
            }),
        }
    }

    fn wait(&self, flags: PollFlags, timeout: Duration) -> Result<PollFlags> {
        let timespec = Timespec::try_from(timeout)
            .map_err(|_| Error::validation("poll timeout", format!("{timeout:?} is too large")))?;
        let mut descriptors = [PollFd::new(&self.fd, flags)];
        let ready = poll(&mut descriptors, Some(&timespec)).map_err(|error| Error::Io {
            operation: "poll hidraw device",
            source: io_error(error),
        })?;
        if ready == 0 {
            return Ok(PollFlags::empty());
        }
        let events = descriptors[0].revents();
        if events.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
            return Err(Error::Disconnected);
        }
        Ok(events)
    }
}

impl Transport for LinuxHidraw {
    fn write_report(&mut self, report: &[u8; 64]) -> Result<()> {
        loop {
            match rustix::io::write(&self.fd, report) {
                Ok(0) => return Err(Error::Disconnected),
                Ok(count) if count == report.len() => return Ok(()),
                Ok(count) => {
                    return Err(Error::protocol(format!(
                        "hidraw accepted a partial report ({count}/{} bytes)",
                        report.len()
                    )));
                }
                Err(error) if error == rustix::io::Errno::AGAIN => {
                    let events = self.wait(PollFlags::OUT, Duration::from_secs(10))?;
                    if !events.contains(PollFlags::OUT) {
                        return Err(Error::Timeout {
                            operation: "write HID report".to_owned(),
                            timeout: Duration::from_secs(10),
                        });
                    }
                }
                Err(
                    rustix::io::Errno::NODEV | rustix::io::Errno::NOENT | rustix::io::Errno::PIPE,
                ) => {
                    return Err(Error::Disconnected);
                }
                Err(error) => {
                    return Err(Error::Io {
                        operation: "write HID report",
                        source: io_error(error),
                    });
                }
            }
        }
    }

    fn read_report(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let events = self.wait(PollFlags::IN, timeout)?;
        if !events.contains(PollFlags::IN) {
            return Ok(None);
        }
        let mut report = [0_u8; 64];
        match rustix::io::read(&self.fd, &mut report) {
            Ok(0) => Err(Error::Disconnected),
            Ok(count) => Ok(Some(report[..count].to_vec())),
            Err(error) if error == rustix::io::Errno::AGAIN => Ok(None),
            Err(rustix::io::Errno::NODEV | rustix::io::Errno::NOENT | rustix::io::Errno::PIPE) => {
                Err(Error::Disconnected)
            }
            Err(error) => Err(Error::Io {
                operation: "read HID report",
                source: io_error(error),
            }),
        }
    }
}

pub(crate) fn enumerate() -> Result<Vec<DeviceInfo>> {
    let entries = match fs::read_dir(SYS_HIDRAW) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::Discovery {
                message: format!("could not read {SYS_HIDRAW}: {error}"),
            });
        }
    };

    let mut devices = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Error::Discovery {
            message: format!("could not inspect hidraw sysfs entry: {error}"),
        })?;
        let uevent_path = entry.path().join("device/uevent");
        let Ok(uevent) = fs::read_to_string(&uevent_path) else {
            continue;
        };
        let metadata = Uevent::parse(&uevent);
        if metadata.vendor_id != Some(VENDOR_ID) || metadata.product_id != Some(PRODUCT_ID) {
            continue;
        }
        let path = Path::new(DEV_ROOT).join(entry.file_name());
        devices.push(DeviceInfo::new(
            path,
            VENDOR_ID,
            PRODUCT_ID,
            metadata.name,
            metadata.serial,
        ));
    }
    devices.sort_by(|left, right| left.path().cmp(right.path()));
    Ok(devices)
}

#[derive(Default)]
struct Uevent {
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    name: Option<String>,
    serial: Option<String>,
}

impl Uevent {
    fn parse(contents: &str) -> Self {
        let mut result = Self::default();
        for line in contents.lines() {
            if let Some(value) = line.strip_prefix("HID_ID=") {
                let mut components = value.split(':');
                let _bus = components.next();
                result.vendor_id = components.next().and_then(parse_hex_u16);
                result.product_id = components.next().and_then(parse_hex_u16);
            } else if let Some(value) = line.strip_prefix("HID_NAME=") {
                result.name = Some(value.to_owned());
            } else if let Some(value) = line
                .strip_prefix("HID_UNIQ=")
                .filter(|value| !value.is_empty())
            {
                result.serial = Some(value.to_owned());
            }
        }
        result
    }
}

fn parse_hex_u16(value: &str) -> Option<u16> {
    u32::from_str_radix(value, 16)
        .ok()
        .and_then(|number| u16::try_from(number).ok())
}

fn io_error(error: rustix::io::Errno) -> io::Error {
    io::Error::from_raw_os_error(error.raw_os_error())
}

#[cfg(test)]
mod tests {
    use super::Uevent;

    #[test]
    fn uevent_parse_should_extract_codex_micro_identity() {
        let parsed = Uevent::parse(
            "DRIVER=hid-generic\nHID_ID=0003:0000303A:00008360\nHID_NAME=Work Louder Codex Micro\nHID_UNIQ=SERIAL\n",
        );

        assert_eq!(
            (
                parsed.vendor_id,
                parsed.product_id,
                parsed.name,
                parsed.serial
            ),
            (
                Some(0x303a),
                Some(0x8360),
                Some("Work Louder Codex Micro".to_owned()),
                Some("SERIAL".to_owned())
            )
        );
    }
}
