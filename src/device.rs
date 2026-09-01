//! High-level Codex Micro device API.

use std::path::{Path, PathBuf};
#[cfg(feature = "persistent-writes")]
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::event::DeviceEvent;
use crate::keymap::KeymapSnapshot;
#[cfg(feature = "persistent-writes")]
use crate::keymap::{KeymapApplyOutcome, KeymapWritePlan, Revision, RollbackFailure};
use crate::lighting::{BaseLighting, ThreadLighting};
use crate::protocol::Protocol;
use crate::transport::Transport;
#[cfg(target_os = "linux")]
use crate::transport::linux::{LinuxHidraw, enumerate as enumerate_linux};
use crate::{DeviceFileName, Error, FirmwareVersion, PRODUCT_ID, Result, VENDOR_ID};

const KEYMAP_RELOAD_DELAY: Duration = Duration::from_millis(2_500);

/// Enumerated Codex Micro HID endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    path: PathBuf,
    vendor_id: u16,
    product_id: u16,
    product_name: Option<String>,
    serial_number: Option<String>,
}

impl DeviceInfo {
    pub(crate) fn new(
        path: PathBuf,
        vendor_id: u16,
        product_id: u16,
        product_name: Option<String>,
        serial_number: Option<String>,
    ) -> Self {
        Self {
            path,
            vendor_id,
            product_id,
            product_name,
            serial_number,
        }
    }

    /// Linux hidraw device node.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// USB vendor identifier.
    #[must_use]
    pub const fn vendor_id(&self) -> u16 {
        self.vendor_id
    }

    /// USB product identifier.
    #[must_use]
    pub const fn product_id(&self) -> u16 {
        self.product_id
    }

    /// Product name reported through sysfs, when available.
    #[must_use]
    pub fn product_name(&self) -> Option<&str> {
        self.product_name.as_deref()
    }

    /// Device serial number reported through sysfs, when available.
    #[must_use]
    pub fn serial_number(&self) -> Option<&str> {
        self.serial_number.as_deref()
    }
}

/// Live device status returned by firmware 0.6.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStatus {
    firmware_version: FirmwareVersion,
    profile_index: u8,
    layer_index: u8,
    battery_percent: u8,
    charging: bool,
}

impl DeviceStatus {
    /// Firmware version included in the status response.
    #[must_use]
    pub const fn firmware_version(&self) -> &FirmwareVersion {
        &self.firmware_version
    }

    /// One-based active profile index reported by the firmware.
    #[must_use]
    pub const fn profile_index(&self) -> u8 {
        self.profile_index
    }

    /// One-based active layer index reported by the firmware.
    #[must_use]
    pub const fn layer_index(&self) -> u8 {
        self.layer_index
    }

    /// Battery charge percentage in the inclusive range 0 through 100.
    #[must_use]
    pub const fn battery_percent(&self) -> u8 {
        self.battery_percent
    }

    /// Returns whether the firmware reports active charging.
    #[must_use]
    pub const fn is_charging(&self) -> bool {
        self.charging
    }
}

/// Root-level file exposed by the firmware's read-only filesystem API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceFileInfo {
    name: DeviceFileName,
    size: u64,
}

impl DeviceFileInfo {
    /// Validated root-level file name.
    #[must_use]
    pub const fn name(&self) -> &DeviceFileName {
        &self.name
    }

    /// File size reported by the firmware, in bytes.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }
}

/// Open, firmware-verified Codex Micro connection.
///
/// `Device` is intentionally single-owner and non-cloneable. Every operation
/// takes `&mut self`, making the firmware's one-request-at-a-time requirement
/// explicit at compile time.
#[cfg(target_os = "linux")]
pub struct Device {
    info: DeviceInfo,
    client: Client<LinuxHidraw>,
}

#[cfg(target_os = "linux")]
impl Device {
    /// Enumerates Codex Micro endpoints matching USB identity `303a:8360`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Discovery`] if Linux sysfs cannot be inspected.
    pub fn enumerate() -> Result<Vec<DeviceInfo>> {
        enumerate_linux()
    }

    /// Opens the first enumerated Codex Micro and verifies firmware `0.6.2`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DeviceNotFound`] if no matching device exists, or a
    /// transport/protocol/firmware error when opening or probing fails.
    pub fn open_first() -> Result<Self> {
        let info = Self::enumerate()?
            .into_iter()
            .next()
            .ok_or(Error::DeviceNotFound {
                vendor_id: VENDOR_ID,
                product_id: PRODUCT_ID,
            })?;
        Self::open(&info)
    }

    /// Opens an enumerated device and verifies firmware `0.6.2`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] for a mismatched identity and a typed
    /// permission, I/O, protocol, timeout, or firmware error otherwise.
    pub fn open(info: &DeviceInfo) -> Result<Self> {
        if info.vendor_id != VENDOR_ID || info.product_id != PRODUCT_ID {
            return Err(Error::validation(
                "device identity",
                format!(
                    "expected {VENDOR_ID:04x}:{PRODUCT_ID:04x}, found {:04x}:{:04x}",
                    info.vendor_id, info.product_id
                ),
            ));
        }
        let transport = LinuxHidraw::open(info)?;
        let client = Client::connect(transport, KEYMAP_RELOAD_DELAY)?;
        Ok(Self {
            info: info.clone(),
            client,
        })
    }

    /// Enumeration record used to open this connection.
    #[must_use]
    pub const fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// Cached, verified firmware version obtained during open.
    #[must_use]
    pub const fn firmware_version(&self) -> &FirmwareVersion {
        self.client.firmware_version()
    }

    /// Reads battery, charging, profile, layer, and firmware status.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, timeout, RPC, protocol, or validation error.
    pub fn status(&mut self) -> Result<DeviceStatus> {
        self.client.status()
    }

    /// Waits up to `timeout` for one device-originated event.
    ///
    /// Events received while another RPC is in progress are queued and
    /// returned first.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, protocol, or validation error.
    pub fn poll_event(&mut self, timeout: Duration) -> Result<Option<DeviceEvent>> {
        self.client.poll_event(timeout)
    }

    /// Sends one or more validated per-agent-key lighting updates.
    ///
    /// An empty slice is a no-op. Firmware acknowledgement confirms receipt,
    /// not visual correctness.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, timeout, RPC, or protocol error.
    pub fn set_thread_lighting(&mut self, updates: &[ThreadLighting]) -> Result<()> {
        self.client.set_thread_lighting(updates)
    }

    /// Updates one or both base lighting zones.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] for an empty update, or a typed device
    /// communication error.
    pub fn set_base_lighting(&mut self, lighting: BaseLighting) -> Result<()> {
        self.client.set_base_lighting(lighting)
    }

    /// Lists root-level firmware-managed files without reading their contents.
    ///
    /// # Errors
    ///
    /// Returns a typed device communication or filename-validation error.
    pub fn list_files(&mut self) -> Result<Vec<DeviceFileInfo>> {
        self.client.list_files()
    }

    /// Reads one validated root-level text file.
    ///
    /// # Errors
    ///
    /// Returns a typed device communication or protocol error.
    pub fn read_text_file(&mut self, name: &DeviceFileName) -> Result<String> {
        self.client.read_text_file(name)
    }

    /// Reads and parses `keymap.json`, preserving unknown fields.
    ///
    /// # Errors
    ///
    /// Returns a typed device communication or keymap parse error.
    pub fn read_keymap(&mut self) -> Result<KeymapSnapshot> {
        self.client.read_keymap()
    }

    /// Applies a validated keymap plan with compare-before-write, exact
    /// readback, and one best-effort rollback attempt.
    ///
    /// The caller must hold exclusive device ownership and durably save
    /// [`KeymapWritePlan::original_json`] before calling this method.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleConfiguration`] before writing if the live
    /// revision changed. Communication failures that prevent the final device
    /// state from being established return [`Error::IndeterminateWrite`].
    #[cfg(feature = "persistent-writes")]
    pub fn apply_keymap(&mut self, plan: &KeymapWritePlan) -> Result<KeymapApplyOutcome> {
        self.client.apply_keymap(plan)
    }
}

struct Client<T> {
    protocol: Protocol<T>,
    firmware_version: FirmwareVersion,
    #[cfg(feature = "persistent-writes")]
    keymap_reload_delay: Duration,
}

impl<T: Transport> Client<T> {
    fn connect(transport: T, keymap_reload_delay: Duration) -> Result<Self> {
        Self::finish_connect(Protocol::new(transport), keymap_reload_delay)
    }

    #[cfg(test)]
    fn connect_for_test(transport: T) -> Result<Self> {
        use crate::protocol::Timing;

        Self::finish_connect(
            Protocol::with_test_timing(
                transport,
                Timing {
                    request_timeout: Duration::from_millis(100),
                    cooldown: Duration::ZERO,
                },
            ),
            Duration::ZERO,
        )
    }

    fn finish_connect(mut protocol: Protocol<T>, keymap_reload_delay: Duration) -> Result<Self> {
        let version: VersionWire = protocol.call("sys.version", Value::Null)?;
        let firmware_version = FirmwareVersion::parse(&version.version)?;
        #[cfg(not(feature = "persistent-writes"))]
        let _ = keymap_reload_delay;
        Ok(Self {
            protocol,
            firmware_version,
            #[cfg(feature = "persistent-writes")]
            keymap_reload_delay,
        })
    }

    const fn firmware_version(&self) -> &FirmwareVersion {
        &self.firmware_version
    }

    fn status(&mut self) -> Result<DeviceStatus> {
        let status: StatusWire = self.protocol.call("device.status", Value::Null)?;
        let firmware_version = FirmwareVersion::parse(&status.version)?;
        if status.battery > 100 {
            return Err(Error::validation(
                "battery percentage",
                format!("{} is outside 0..=100", status.battery),
            ));
        }
        Ok(DeviceStatus {
            firmware_version,
            profile_index: status.profile_index,
            layer_index: status.layer_index,
            battery_percent: status.battery,
            charging: status.is_charging,
        })
    }

    fn poll_event(&mut self, timeout: Duration) -> Result<Option<DeviceEvent>> {
        self.protocol.poll_event(timeout)
    }

    fn set_thread_lighting(&mut self, updates: &[ThreadLighting]) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }
        let params = Value::Array(
            updates
                .iter()
                .copied()
                .map(ThreadLighting::to_json)
                .collect(),
        );
        let _: Value = self.protocol.call("v.oai.thstatus", params)?;
        Ok(())
    }

    fn set_base_lighting(&mut self, lighting: BaseLighting) -> Result<()> {
        if lighting.is_empty() {
            return Err(Error::validation(
                "base lighting",
                "at least one zone must be present",
            ));
        }
        let _: Value = self.protocol.call("v.oai.rgbcfg", lighting.to_json())?;
        Ok(())
    }

    fn list_files(&mut self) -> Result<Vec<DeviceFileInfo>> {
        let files: Vec<FileInfoWire> = self.protocol.call(
            "fs.list",
            json!({"checksum": false, "rec": true, "path": ""}),
        )?;
        files
            .into_iter()
            .map(|file| {
                Ok(DeviceFileInfo {
                    name: DeviceFileName::new(file.name)?,
                    size: file.size,
                })
            })
            .collect()
    }

    fn read_text_file(&mut self, name: &DeviceFileName) -> Result<String> {
        let result: ReadTextWire = self
            .protocol
            .call("fs.read", json!({"file": name.as_str()}))?;
        Ok(match result {
            ReadTextWire::Text(text) | ReadTextWire::Data { data: text } => text,
        })
    }

    fn read_keymap(&mut self) -> Result<KeymapSnapshot> {
        let text = self.read_keymap_text()?;
        KeymapSnapshot::from_json(text)
    }

    fn read_keymap_text(&mut self) -> Result<String> {
        self.read_text_file(&DeviceFileName::keymap())
    }

    #[cfg(feature = "persistent-writes")]
    fn apply_keymap(&mut self, plan: &KeymapWritePlan) -> Result<KeymapApplyOutcome> {
        let live = self.read_keymap_text()?;
        let actual_revision = Revision::from_text(&live);
        if actual_revision != plan.expected_revision() {
            return Err(Error::StaleConfiguration {
                expected: plan.expected_revision(),
                actual: actual_revision,
            });
        }
        if plan.is_unchanged() {
            return Ok(KeymapApplyOutcome::Unchanged);
        }

        let write_result = self.write_keymap(plan.candidate_json());
        if let Err(write_error) = write_result {
            return self.resolve_failed_write(plan, write_error);
        }
        thread::sleep(self.keymap_reload_delay);
        match self.read_keymap_text() {
            Ok(text) if text == plan.candidate_json() => Ok(KeymapApplyOutcome::Applied {
                revision: plan.candidate_revision(),
            }),
            Ok(text) => self.rollback(
                plan,
                format!(
                    "candidate readback mismatch (revision {})",
                    Revision::from_text(&text)
                ),
            ),
            Err(error) => self.rollback(plan, format!("candidate readback failed: {error}")),
        }
    }

    #[cfg(feature = "persistent-writes")]
    fn resolve_failed_write(
        &mut self,
        plan: &KeymapWritePlan,
        write_error: Error,
    ) -> Result<KeymapApplyOutcome> {
        thread::sleep(self.keymap_reload_delay);
        match self.read_keymap_text() {
            Ok(text) if text == plan.candidate_json() => Ok(KeymapApplyOutcome::Applied {
                revision: plan.candidate_revision(),
            }),
            Ok(text) if text == plan.original_json() => Err(write_error),
            Ok(text) => self.rollback(
                plan,
                format!(
                    "write returned {write_error}; live revision became {}",
                    Revision::from_text(&text)
                ),
            ),
            Err(read_error) => Err(Error::IndeterminateWrite {
                message: format!(
                    "write returned {write_error}; subsequent read failed: {read_error}"
                ),
            }),
        }
    }

    #[cfg(feature = "persistent-writes")]
    fn rollback(&mut self, plan: &KeymapWritePlan, cause: String) -> Result<KeymapApplyOutcome> {
        if let Err(error) = self.write_keymap(plan.original_json()) {
            return Ok(KeymapApplyOutcome::RollbackFailed {
                cause,
                rollback: RollbackFailure::new(format!("restore write failed: {error}")),
            });
        }
        thread::sleep(self.keymap_reload_delay);
        match self.read_keymap_text() {
            Ok(text) if text == plan.original_json() => {
                Ok(KeymapApplyOutcome::RolledBack { cause })
            }
            Ok(text) => Ok(KeymapApplyOutcome::RollbackFailed {
                cause,
                rollback: RollbackFailure::new(format!(
                    "restore readback revision {} did not match {}",
                    Revision::from_text(&text),
                    plan.expected_revision()
                )),
            }),
            Err(error) => Ok(KeymapApplyOutcome::RollbackFailed {
                cause,
                rollback: RollbackFailure::new(format!("restore readback failed: {error}")),
            }),
        }
    }

    #[cfg(feature = "persistent-writes")]
    fn write_keymap(&mut self, text: &str) -> Result<()> {
        let _: Value = self
            .protocol
            .call("fs.write", json!({"file": "keymap.json", "data": text}))?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct VersionWire {
    version: String,
}

#[derive(Deserialize)]
struct StatusWire {
    version: String,
    profile_index: u8,
    layer_index: u8,
    battery: u8,
    is_charging: bool,
}

#[derive(Deserialize)]
struct FileInfoWire {
    name: String,
    size: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReadTextWire {
    Text(String),
    Data { data: String },
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    #[cfg(all(test, feature = "persistent-writes"))]
    use crate::protocol::rpc_error_response;
    use crate::protocol::{REPORT_SIZE, rpc_notification, rpc_response};
    use crate::{AgentId, InputControl, LightingEffect, LightingZone, RgbColor};
    #[cfg(feature = "persistent-writes")]
    use crate::{KeyCode, KeyPosition, LayerId, ProfileId};

    use super::*;

    const KEYMAP: &str = include_str!("../tests/fixtures/keymap-0.6.2.json");

    #[derive(Clone, Default)]
    struct SharedTransport {
        state: Rc<RefCell<FakeState>>,
    }

    #[derive(Default)]
    struct FakeState {
        reads: VecDeque<Vec<u8>>,
        writes: Vec<[u8; REPORT_SIZE]>,
    }

    impl SharedTransport {
        fn with_response_groups(groups: Vec<Vec<[u8; REPORT_SIZE]>>) -> Self {
            let transport = Self::default();
            transport
                .state
                .borrow_mut()
                .reads
                .extend(groups.into_iter().flatten().map(|report| report.to_vec()));
            transport
        }

        fn written_requests(&self) -> Vec<Value> {
            let state = self.state.borrow();
            let bytes: Vec<u8> = state
                .writes
                .iter()
                .flat_map(|report| {
                    let length = usize::from(report[2]);
                    report[3..3 + length].iter().copied()
                })
                .collect();
            serde_json::Deserializer::from_slice(&bytes)
                .into_iter::<Value>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap_or_else(|error| panic!("written requests should be JSON: {error}"))
        }
    }

    impl Transport for SharedTransport {
        fn write_report(&mut self, report: &[u8; REPORT_SIZE]) -> Result<()> {
            self.state.borrow_mut().writes.push(*report);
            Ok(())
        }

        fn read_report(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>> {
            Ok(self.state.borrow_mut().reads.pop_front())
        }
    }

    fn version_response(id: u16, version: &str) -> Vec<[u8; REPORT_SIZE]> {
        rpc_response(id, json!({"version": version}))
    }

    fn read_response(id: u16, text: &str) -> Vec<[u8; REPORT_SIZE]> {
        rpc_response(id, json!({"data": text}))
    }

    #[cfg(feature = "persistent-writes")]
    fn changed_plan() -> KeymapWritePlan {
        let snapshot = KeymapSnapshot::from_json(KEYMAP)
            .unwrap_or_else(|error| panic!("fixture should parse: {error}"));
        let mut draft = snapshot.draft();
        draft
            .set_key_binding(
                ProfileId::new(0),
                LayerId::new(0),
                KeyPosition::new(3)
                    .unwrap_or_else(|error| panic!("position should be valid: {error}")),
                KeyCode::new("KC_B")
                    .unwrap_or_else(|error| panic!("keycode should be valid: {error}")),
            )
            .unwrap_or_else(|error| panic!("edit should succeed: {error}"));
        draft
            .into_write_plan()
            .unwrap_or_else(|error| panic!("plan should build: {error}"))
    }

    #[test]
    fn connect_should_reject_unverified_firmware() {
        let transport = SharedTransport::with_response_groups(vec![version_response(1, "0.7.0")]);

        let error = Client::connect_for_test(transport)
            .err()
            .unwrap_or_else(|| panic!("unsupported firmware should fail"));

        assert!(matches!(error, Error::UnsupportedFirmware { .. }));
    }

    #[test]
    fn status_should_return_validated_typed_fields() {
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            rpc_response(
                2,
                json!({
                    "version": "0.6.2",
                    "profile_index": 1,
                    "layer_index": 2,
                    "battery": 99,
                    "is_charging": true
                }),
            ),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let status = client
            .status()
            .unwrap_or_else(|error| panic!("status should succeed: {error}"));

        assert_eq!(
            (
                status.firmware_version().as_str(),
                status.profile_index(),
                status.layer_index(),
                status.battery_percent(),
                status.is_charging()
            ),
            ("0.6.2", 1, 2, 99, true)
        );
    }

    #[test]
    fn thread_lighting_should_encode_validated_short_field_payload() {
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            rpc_response(2, json!({"ok": 1})),
        ]);
        let observer = transport.clone();
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));
        let update = ThreadLighting::new(
            AgentId::new(3).unwrap_or_else(|error| panic!("id should be valid: {error}")),
            RgbColor::from_rgb(0x12, 0x34, 0x56),
        )
        .with_effect(LightingEffect::Breath);

        client
            .set_thread_lighting(&[update])
            .unwrap_or_else(|error| panic!("lighting call should succeed: {error}"));
        let requests = observer.written_requests();

        assert_eq!(
            (
                requests[1]["method"].as_str(),
                requests[1]["params"][0]["e"].as_u64()
            ),
            (Some("v.oai.thstatus"), Some(4))
        );
    }

    #[test]
    fn base_lighting_should_use_the_compact_wire_shape() {
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            rpc_response(2, json!({"ok": 1})),
        ]);
        let observer = transport.clone();
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));
        let zone = LightingZone::new(
            LightingEffect::Gradient,
            RgbColor::from_rgb(0x12, 0x34, 0x56),
        );

        client
            .set_base_lighting(BaseLighting::new().with_keys(zone).with_ambient(zone))
            .unwrap_or_else(|error| panic!("base lighting should succeed: {error}"));
        let requests = observer.written_requests();

        assert_eq!(requests[1]["params"]["keys"]["e"], json!(5));
        assert_eq!(requests[1]["params"]["ambient"]["e"], json!(5));
        assert!(requests[1]["params"].get("backlight").is_none());
    }

    #[test]
    fn list_files_should_validate_names_and_sizes() {
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            rpc_response(
                2,
                json!([
                    {"name": "keymap.json", "size": 1816},
                    {"name": "smart_actions.json", "size": 41}
                ]),
            ),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let files = client
            .list_files()
            .unwrap_or_else(|error| panic!("file listing should succeed: {error}"));

        assert_eq!(
            (files[0].name().as_str(), files[1].size()),
            ("keymap.json", 41)
        );
    }

    #[test]
    fn read_keymap_should_parse_the_lossless_document() {
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            read_response(2, KEYMAP),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let snapshot = client
            .read_keymap()
            .unwrap_or_else(|error| panic!("keymap read should succeed: {error}"));

        assert_eq!(snapshot.document().profiles().len(), 1);
    }

    #[test]
    fn input_notification_should_be_exposed_while_polling() {
        let notification = rpc_response(1, json!({"version": "0.6.2"}));
        let input = rpc_notification("v.oai.hid", json!({"k": "AG03", "act": 1}));
        let transport = SharedTransport::with_response_groups(vec![notification, input]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let event = client
            .poll_event(Duration::from_millis(10))
            .unwrap_or_else(|error| panic!("poll should succeed: {error}"));

        assert!(matches!(
            event,
            Some(DeviceEvent::Input(ref input))
                if matches!(input.control(), InputControl::Agent(_))
        ));
    }

    #[cfg(feature = "persistent-writes")]
    #[test]
    fn apply_keymap_should_write_and_verify_candidate() {
        let plan = changed_plan();
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            read_response(2, KEYMAP),
            rpc_response(3, json!({"ok": 1})),
            read_response(4, plan.candidate_json()),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let outcome = client
            .apply_keymap(&plan)
            .unwrap_or_else(|error| panic!("apply should succeed: {error}"));

        assert!(matches!(outcome, KeymapApplyOutcome::Applied { .. }));
    }

    #[cfg(feature = "persistent-writes")]
    #[test]
    fn apply_keymap_should_reject_stale_revision_before_writing() {
        let plan = changed_plan();
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            read_response(2, "{\"different\":true}"),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let error = client
            .apply_keymap(&plan)
            .expect_err("stale plan should fail");

        assert!(matches!(error, Error::StaleConfiguration { .. }));
    }

    #[cfg(feature = "persistent-writes")]
    #[test]
    fn apply_keymap_should_skip_write_for_unchanged_plan() {
        let snapshot = KeymapSnapshot::from_json(KEYMAP)
            .unwrap_or_else(|error| panic!("fixture should parse: {error}"));
        let plan = snapshot
            .draft()
            .into_write_plan()
            .unwrap_or_else(|error| panic!("plan should build: {error}"));
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            read_response(2, KEYMAP),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let outcome = client
            .apply_keymap(&plan)
            .unwrap_or_else(|error| panic!("no-op apply should succeed: {error}"));

        assert!(matches!(outcome, KeymapApplyOutcome::Unchanged));
    }

    #[cfg(feature = "persistent-writes")]
    #[test]
    fn apply_keymap_should_restore_original_after_readback_mismatch() {
        let plan = changed_plan();
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            read_response(2, KEYMAP),
            rpc_response(3, json!({"ok": 1})),
            read_response(4, "{\"unexpected\":true}"),
            rpc_response(5, json!({"ok": 1})),
            read_response(6, KEYMAP),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let outcome = client
            .apply_keymap(&plan)
            .unwrap_or_else(|error| panic!("rollback should be reported: {error}"));

        assert!(matches!(outcome, KeymapApplyOutcome::RolledBack { .. }));
    }

    #[cfg(feature = "persistent-writes")]
    #[test]
    fn apply_keymap_should_report_rollback_write_failure() {
        let plan = changed_plan();
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            read_response(2, KEYMAP),
            rpc_response(3, json!({"ok": 1})),
            read_response(4, "{\"unexpected\":true}"),
            rpc_error_response(5, "restore rejected"),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let outcome = client
            .apply_keymap(&plan)
            .unwrap_or_else(|error| panic!("rollback failure should be an outcome: {error}"));

        assert!(matches!(outcome, KeymapApplyOutcome::RollbackFailed { .. }));
    }

    #[cfg(feature = "persistent-writes")]
    #[test]
    fn apply_keymap_should_report_an_indeterminate_failed_write() {
        let plan = changed_plan();
        let transport = SharedTransport::with_response_groups(vec![
            version_response(1, "0.6.2"),
            read_response(2, KEYMAP),
            rpc_error_response(3, "write outcome unknown"),
        ]);
        let mut client = Client::connect_for_test(transport)
            .unwrap_or_else(|error| panic!("connect should succeed: {error}"));

        let error = client
            .apply_keymap(&plan)
            .expect_err("unreadable state after a failed write must be indeterminate");

        assert!(matches!(error, Error::IndeterminateWrite { .. }));
    }
}
