//! HID framing, JSON stream decoding, and serialized RPC handling.

use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;

use crate::event::{ButtonState, DeviceEvent, EncoderInput, InputControl, InputEvent};
use crate::transport::Transport;
use crate::{ActionId, AgentId, Error, Result};

pub(crate) const REPORT_ID: u8 = 0x06;
pub(crate) const REPORT_SIZE: usize = 64;
pub(crate) const MAX_CHUNK: usize = REPORT_SIZE - 3;
const CHANNEL_DEBUG: u8 = 1;
const CHANNEL_RPC: u8 = 2;
const MAX_RPC_ID: u16 = 998;
const MAX_RECEIVE_BUFFER: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    pub(crate) request_timeout: Duration,
    pub(crate) cooldown: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(10),
            cooldown: Duration::from_millis(50),
        }
    }
}

pub(crate) struct Protocol<T> {
    transport: T,
    decoder: Decoder,
    events: VecDeque<DeviceEvent>,
    next_id: u16,
    last_call_finished: Option<Instant>,
    timing: Timing,
}

impl<T: Transport> Protocol<T> {
    pub(crate) fn new(transport: T) -> Self {
        Self::with_timing(transport, Timing::default())
    }

    #[cfg(test)]
    pub(crate) fn with_test_timing(transport: T, timing: Timing) -> Self {
        Self::with_timing(transport, timing)
    }

    fn with_timing(transport: T, timing: Timing) -> Self {
        Self {
            transport,
            decoder: Decoder::default(),
            events: VecDeque::new(),
            next_id: 1,
            last_call_finished: None,
            timing,
        }
    }

    pub(crate) fn call<R>(&mut self, method: &str, params: Value) -> Result<R>
    where
        R: DeserializeOwned,
    {
        self.wait_for_cooldown();
        let result = self.call_inner(method, params);
        self.last_call_finished = Some(Instant::now());
        result
    }

    pub(crate) fn poll_event(&mut self, timeout: Duration) -> Result<Option<DeviceEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| Error::validation("event timeout", "duration overflows Instant"))?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(report) = self.transport.read_report(remaining)? else {
                return Ok(None);
            };
            self.process_report(&report, None, None, None)?;
            if let Some(event) = self.events.pop_front() {
                return Ok(Some(event));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    fn call_inner<R>(&mut self, method: &str, params: Value) -> Result<R>
    where
        R: DeserializeOwned,
    {
        let id = self.allocate_id();
        let request = encode_request(method, params, id)?;
        for report in frame_message(CHANNEL_RPC, request.as_bytes())? {
            self.transport.write_report(&report)?;
        }

        let deadline = Instant::now()
            .checked_add(self.timing.request_timeout)
            .ok_or_else(|| Error::validation("request timeout", "duration overflows Instant"))?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(report) = self.transport.read_report(remaining)? else {
                return Err(Error::Timeout {
                    operation: method.to_owned(),
                    timeout: self.timing.request_timeout,
                });
            };
            let mut response = None;
            self.process_report(&report, Some(id), Some(method), Some(&mut response))?;
            if let Some(value) = response {
                return serde_json::from_value(value)
                    .map_err(|error| Error::protocol(format!("invalid {method} result: {error}")));
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout {
                    operation: method.to_owned(),
                    timeout: self.timing.request_timeout,
                });
            }
        }
    }

    fn process_report(
        &mut self,
        report: &[u8],
        expected_id: Option<u16>,
        expected_method: Option<&str>,
        mut response: Option<&mut Option<Value>>,
    ) -> Result<()> {
        for incoming in self.decoder.push(report)? {
            match incoming {
                Incoming::Debug(line) => self.events.push_back(DeviceEvent::Debug(line)),
                Incoming::Json(message) => {
                    if let Some(id) = response_id(&message) {
                        if Some(id) == expected_id {
                            let method = expected_method
                                .or_else(|| message.get("method").and_then(Value::as_str))
                                .unwrap_or("unknown")
                                .to_owned();
                            let value = decode_response(method, message)?;
                            if let Some(slot) = response.as_deref_mut() {
                                *slot = Some(value);
                            }
                        }
                    } else if let Some(event) = parse_notification(message) {
                        self.events.push_back(event);
                    }
                }
            }
        }
        Ok(())
    }

    fn allocate_id(&mut self) -> u16 {
        let id = self.next_id;
        self.next_id = if id == MAX_RPC_ID { 1 } else { id + 1 };
        id
    }

    fn wait_for_cooldown(&self) {
        if let Some(finished) = self.last_call_finished {
            let elapsed = finished.elapsed();
            if elapsed < self.timing.cooldown {
                thread::sleep(self.timing.cooldown - elapsed);
            }
        }
    }
}

fn encode_request(method: &str, params: Value, id: u16) -> Result<String> {
    #[derive(Serialize)]
    struct Request<'a> {
        method: &'a str,
        params: Value,
        id: u16,
    }

    let json = serde_json::to_string(&Request { method, params, id })
        .map_err(|error| Error::protocol(format!("could not serialize request: {error}")))?;
    Ok(escape_non_ascii(&json))
}

fn escape_non_ascii(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii() {
            escaped.push(character);
        } else {
            let mut units = [0_u16; 2];
            for unit in character.encode_utf16(&mut units) {
                use std::fmt::Write;
                let _ = write!(escaped, "\\u{unit:04x}");
            }
        }
    }
    escaped
}

fn frame_message(channel: u8, payload: &[u8]) -> Result<Vec<[u8; REPORT_SIZE]>> {
    if payload.is_empty() {
        return Err(Error::protocol("cannot frame an empty message"));
    }
    let mut reports = Vec::with_capacity(payload.len().div_ceil(MAX_CHUNK));
    for chunk in payload.chunks(MAX_CHUNK) {
        let length = u8::try_from(chunk.len())
            .map_err(|_| Error::protocol("HID payload chunk exceeds one-byte length"))?;
        let mut report = [0_u8; REPORT_SIZE];
        report[0] = REPORT_ID;
        report[1] = channel;
        report[2] = length;
        report[3..3 + chunk.len()].copy_from_slice(chunk);
        reports.push(report);
    }
    Ok(reports)
}

#[derive(Default)]
struct Decoder {
    debug: Vec<u8>,
    rpc: Vec<u8>,
}

impl Decoder {
    fn push(&mut self, report: &[u8]) -> Result<Vec<Incoming>> {
        if report.len() < 3 {
            return Err(Error::protocol(format!(
                "report is {} bytes; expected at least 3",
                report.len()
            )));
        }
        if report[0] != REPORT_ID {
            return Err(Error::protocol(format!(
                "unexpected report id 0x{:02x}",
                report[0]
            )));
        }
        let length = usize::from(report[2]);
        if length > MAX_CHUNK || report.len() < 3 + length {
            return Err(Error::protocol(format!(
                "invalid report payload length {length} for {}-byte report",
                report.len()
            )));
        }
        let payload = &report[3..3 + length];
        match report[1] {
            CHANNEL_DEBUG => Self::push_debug(&mut self.debug, payload),
            CHANNEL_RPC => Self::push_rpc(&mut self.rpc, payload),
            channel => Err(Error::protocol(format!(
                "unsupported HID channel {channel}"
            ))),
        }
    }

    fn push_debug(buffer: &mut Vec<u8>, payload: &[u8]) -> Result<Vec<Incoming>> {
        extend_bounded(buffer, payload)?;
        let mut lines = Vec::new();
        while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
            let bytes: Vec<u8> = buffer.drain(..=position).collect();
            let line = String::from_utf8_lossy(&bytes[..bytes.len().saturating_sub(1)])
                .trim_end_matches('\r')
                .to_owned();
            lines.push(Incoming::Debug(line));
        }
        Ok(lines)
    }

    fn push_rpc(buffer: &mut Vec<u8>, payload: &[u8]) -> Result<Vec<Incoming>> {
        extend_bounded(buffer, payload)?;
        let mut values = Vec::new();
        loop {
            if buffer.iter().all(u8::is_ascii_whitespace) {
                buffer.clear();
                break;
            }
            let (next, consumed) = {
                let mut stream = serde_json::Deserializer::from_slice(buffer).into_iter::<Value>();
                let next = stream.next();
                (next, stream.byte_offset())
            };
            match next {
                Some(Ok(value)) => {
                    buffer.drain(..consumed);
                    values.push(Incoming::Json(value));
                }
                Some(Err(error)) if error.is_eof() => break,
                Some(Err(error)) => {
                    buffer.clear();
                    return Err(Error::protocol(format!("invalid JSON message: {error}")));
                }
                None => {
                    buffer.clear();
                    break;
                }
            }
        }
        Ok(values)
    }
}

fn extend_bounded(buffer: &mut Vec<u8>, payload: &[u8]) -> Result<()> {
    if buffer.len().saturating_add(payload.len()) > MAX_RECEIVE_BUFFER {
        buffer.clear();
        return Err(Error::protocol(format!(
            "receive buffer exceeded {MAX_RECEIVE_BUFFER} bytes"
        )));
    }
    buffer.extend_from_slice(payload);
    Ok(())
}

#[derive(Debug)]
enum Incoming {
    Debug(String),
    Json(Value),
}

fn response_id(message: &Value) -> Option<u16> {
    let id = message.get("id")?;
    if let Some(number) = id.as_u64() {
        return u16::try_from(number).ok();
    }
    id.as_str()?.parse().ok()
}

fn decode_response(method: String, mut message: Value) -> Result<Value> {
    if let Some(error) = message.get_mut("error").map(Value::take) {
        let code = error.get("code").and_then(Value::as_i64);
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("device returned an unspecified RPC error")
            .to_owned();
        let data = error.get("data").cloned().map(Box::new);
        return Err(Error::Rpc {
            method,
            code,
            message: text,
            data,
        });
    }
    message
        .get_mut("result")
        .map(Value::take)
        .ok_or_else(|| Error::protocol("RPC response contains neither result nor error"))
}

fn parse_notification(message: Value) -> Option<DeviceEvent> {
    let method = message.get("m").and_then(Value::as_str)?.to_owned();
    let params = message.get("p").cloned().unwrap_or(Value::Null);
    match method.as_str() {
        "v.oai.hid" => parse_input_event(&params)
            .map(DeviceEvent::Input)
            .or_else(|| Some(DeviceEvent::UnknownNotification { method, params })),
        "v.oai.rad" => Some(DeviceEvent::Radial(params)),
        _ => Some(DeviceEvent::UnknownNotification { method, params }),
    }
}

fn parse_input_event(params: &Value) -> Option<InputEvent> {
    let key = params.get("k")?.as_str()?;
    let action = u8::try_from(params.get("act")?.as_u64()?).ok()?;
    let state = match action {
        0 => ButtonState::Released,
        1 => ButtonState::Pressed,
        other => ButtonState::Unknown(other),
    };
    Some(InputEvent::new(parse_control(key), state))
}

fn parse_control(key: &str) -> InputControl {
    if let Some(id) = key
        .strip_prefix("AG")
        .and_then(|value| value.parse::<u8>().ok())
        .and_then(|number| AgentId::new(number).ok())
    {
        return InputControl::Agent(id);
    }
    if let Some(id) = key
        .strip_prefix("ACT")
        .and_then(|value| value.parse::<u8>().ok())
        .and_then(|number| ActionId::new(number).ok())
    {
        return InputControl::Action(id);
    }
    match key {
        "ENC_CC" => InputControl::Encoder(EncoderInput::CounterClockwise),
        "ENC_CW" => InputControl::Encoder(EncoderInput::Clockwise),
        "ENC_CLK" => InputControl::Encoder(EncoderInput::Press),
        _ => InputControl::Unknown(key.to_owned()),
    }
}

#[cfg(test)]
pub(crate) fn rpc_response(id: u16, result: Value) -> Vec<[u8; REPORT_SIZE]> {
    let payload = serde_json::to_vec(&json!({"id": id, "result": result}))
        .unwrap_or_else(|error| panic!("test response must serialize: {error}"));
    frame_message(CHANNEL_RPC, &payload)
        .unwrap_or_else(|error| panic!("test response must frame: {error}"))
}

#[cfg(all(test, feature = "persistent-writes"))]
pub(crate) fn rpc_error_response(id: u16, message: &str) -> Vec<[u8; REPORT_SIZE]> {
    let payload = serde_json::to_vec(&json!({"id": id, "error": {"message": message}}))
        .unwrap_or_else(|error| panic!("test error response must serialize: {error}"));
    frame_message(CHANNEL_RPC, &payload)
        .unwrap_or_else(|error| panic!("test error response must frame: {error}"))
}

#[cfg(test)]
pub(crate) fn rpc_notification(method: &str, params: Value) -> Vec<[u8; REPORT_SIZE]> {
    let payload = serde_json::to_vec(&json!({"m": method, "p": params}))
        .unwrap_or_else(|error| panic!("test notification must serialize: {error}"));
    frame_message(CHANNEL_RPC, &payload)
        .unwrap_or_else(|error| panic!("test notification must frame: {error}"))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use proptest::prelude::*;

    use super::*;

    #[derive(Clone, Default)]
    struct SharedTransport {
        state: Rc<RefCell<FakeState>>,
    }

    #[derive(Default)]
    struct FakeState {
        reads: VecDeque<ReadStep>,
        writes: Vec<[u8; REPORT_SIZE]>,
    }

    enum ReadStep {
        Report(Vec<u8>),
        Timeout,
        Disconnected,
    }

    impl SharedTransport {
        fn with_reports(reports: impl IntoIterator<Item = [u8; REPORT_SIZE]>) -> Self {
            let transport = Self::default();
            transport.state.borrow_mut().reads.extend(
                reports
                    .into_iter()
                    .map(|report| ReadStep::Report(report.to_vec())),
            );
            transport
        }

        fn push_reports(&self, reports: impl IntoIterator<Item = [u8; REPORT_SIZE]>) {
            self.state.borrow_mut().reads.extend(
                reports
                    .into_iter()
                    .map(|report| ReadStep::Report(report.to_vec())),
            );
        }
    }

    impl Transport for SharedTransport {
        fn write_report(&mut self, report: &[u8; REPORT_SIZE]) -> Result<()> {
            self.state.borrow_mut().writes.push(*report);
            Ok(())
        }

        fn read_report(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>> {
            match self.state.borrow_mut().reads.pop_front() {
                Some(ReadStep::Report(report)) => Ok(Some(report)),
                Some(ReadStep::Disconnected) => Err(Error::Disconnected),
                Some(ReadStep::Timeout) | None => Ok(None),
            }
        }
    }

    fn immediate_timing() -> Timing {
        Timing {
            request_timeout: Duration::from_millis(100),
            cooldown: Duration::ZERO,
        }
    }

    #[test]
    fn frame_message_should_split_payload_at_sixty_one_bytes() {
        let reports = frame_message(CHANNEL_RPC, &[b'x'; MAX_CHUNK + 1])
            .unwrap_or_else(|error| panic!("framing should succeed: {error}"));

        assert_eq!((reports.len(), reports[0][2], reports[1][2]), (2, 61, 1));
    }

    #[test]
    fn encode_request_should_escape_non_bmp_unicode_as_surrogate_pair() {
        let request = encode_request("test", json!({"value": "💡"}), 1)
            .unwrap_or_else(|error| panic!("encoding should succeed: {error}"));

        assert!(
            request.contains("\\ud83d\\udca1"),
            "encoded request: {request}"
        );
    }

    #[test]
    fn decoder_should_reassemble_json_split_across_reports() {
        let reports = frame_message(CHANNEL_RPC, br#"{"id":1,"result":{"ok":true}}"#)
            .unwrap_or_else(|error| panic!("framing should succeed: {error}"));
        let mut decoder = Decoder::default();
        let mut messages = Vec::new();
        for report in reports {
            messages.extend(
                decoder
                    .push(&report)
                    .unwrap_or_else(|error| panic!("decoding should succeed: {error}")),
            );
        }

        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn decoder_should_extract_concatenated_json_values() {
        let reports = frame_message(
            CHANNEL_RPC,
            br#"{"id":1,"result":null}{"m":"future.event","p":{"x":1}}"#,
        )
        .unwrap_or_else(|error| panic!("framing should succeed: {error}"));
        let mut decoder = Decoder::default();
        let mut messages = Vec::new();
        for report in reports {
            messages.extend(
                decoder
                    .push(&report)
                    .unwrap_or_else(|error| panic!("decoding should succeed: {error}")),
            );
        }

        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn decoder_should_reject_payload_length_larger_than_report() {
        let report = [REPORT_ID, CHANNEL_RPC, 62];
        let error = Decoder::default()
            .push(&report)
            .expect_err("invalid length should fail");

        assert!(matches!(error, Error::Protocol { .. }));
    }

    #[test]
    fn call_should_queue_notification_received_before_response() {
        let notification = frame_message(
            CHANNEL_RPC,
            br#"{"m":"v.oai.hid","p":{"k":"AG03","act":1}}"#,
        )
        .unwrap_or_else(|error| panic!("notification should frame: {error}"));
        let transport = SharedTransport::with_reports(
            notification
                .into_iter()
                .chain(rpc_response(1, json!({"version": "0.6.2"}))),
        );
        let mut protocol = Protocol::with_test_timing(transport, immediate_timing());

        let _: Value = protocol
            .call("sys.version", Value::Null)
            .unwrap_or_else(|error| panic!("call should succeed: {error}"));
        let event = protocol
            .poll_event(Duration::ZERO)
            .unwrap_or_else(|error| panic!("poll should succeed: {error}"));

        assert!(matches!(event, Some(DeviceEvent::Input(InputEvent { .. }))));
    }

    #[test]
    fn call_should_ignore_response_with_foreign_id() {
        let transport = SharedTransport::with_reports(
            rpc_response(42, json!({"foreign": true}))
                .into_iter()
                .chain(rpc_response(1, json!({"local": true}))),
        );
        let mut protocol = Protocol::with_test_timing(transport, immediate_timing());

        let result: Value = protocol
            .call("test", Value::Null)
            .unwrap_or_else(|error| panic!("call should succeed: {error}"));

        assert_eq!(result, json!({"local": true}));
    }

    #[test]
    fn polling_should_preserve_debug_radial_and_unknown_events() {
        let debug = frame_message(CHANNEL_DEBUG, b"firmware note\n")
            .unwrap_or_else(|error| panic!("debug line should frame: {error}"));
        let radial = rpc_notification("v.oai.rad", json!({"a": 90, "d": 0.5}));
        let unknown = rpc_notification("future.event", json!({"answer": 42}));
        let transport =
            SharedTransport::with_reports(debug.into_iter().chain(radial).chain(unknown));
        let mut protocol = Protocol::with_test_timing(transport, immediate_timing());

        let first = protocol
            .poll_event(Duration::ZERO)
            .unwrap_or_else(|error| panic!("debug poll should succeed: {error}"));
        let second = protocol
            .poll_event(Duration::ZERO)
            .unwrap_or_else(|error| panic!("radial poll should succeed: {error}"));
        let third = protocol
            .poll_event(Duration::ZERO)
            .unwrap_or_else(|error| panic!("unknown poll should succeed: {error}"));

        assert!(matches!(first, Some(DeviceEvent::Debug(ref line)) if line == "firmware note"));
        assert!(matches!(second, Some(DeviceEvent::Radial(ref value)) if value["a"] == 90));
        assert!(matches!(
            third,
            Some(DeviceEvent::UnknownNotification { ref method, ref params })
                if method == "future.event" && params["answer"] == 42
        ));
    }

    #[test]
    fn call_should_report_timeout_when_no_response_arrives() {
        let transport = SharedTransport::default();
        transport
            .state
            .borrow_mut()
            .reads
            .push_back(ReadStep::Timeout);
        let mut protocol = Protocol::with_test_timing(transport, immediate_timing());

        let error = protocol
            .call::<Value>("test", Value::Null)
            .expect_err("missing response should time out");

        assert!(matches!(error, Error::Timeout { .. }));
    }

    #[test]
    fn call_should_propagate_disconnect() {
        let transport = SharedTransport::default();
        transport
            .state
            .borrow_mut()
            .reads
            .push_back(ReadStep::Disconnected);
        let mut protocol = Protocol::with_test_timing(transport, immediate_timing());

        let error = protocol
            .call::<Value>("test", Value::Null)
            .expect_err("disconnect should fail");

        assert!(matches!(error, Error::Disconnected));
    }

    #[test]
    fn allocate_id_should_wrap_from_998_to_1() {
        let transport = SharedTransport::default();
        let mut protocol = Protocol::with_test_timing(transport, immediate_timing());
        protocol.next_id = MAX_RPC_ID;

        let ids = (protocol.allocate_id(), protocol.allocate_id());

        assert_eq!(ids, (998, 1));
    }

    #[test]
    fn call_should_wait_for_configured_cooldown() {
        let transport = SharedTransport::with_reports(rpc_response(1, Value::Null));
        transport.push_reports(rpc_response(2, Value::Null));
        let timing = Timing {
            request_timeout: Duration::from_millis(100),
            cooldown: Duration::from_millis(5),
        };
        let mut protocol = Protocol::with_test_timing(transport, timing);
        let _: Value = protocol
            .call("first", Value::Null)
            .unwrap_or_else(|error| panic!("first call should succeed: {error}"));
        let started = Instant::now();

        let _: Value = protocol
            .call("second", Value::Null)
            .unwrap_or_else(|error| panic!("second call should succeed: {error}"));

        assert!(started.elapsed() >= Duration::from_millis(5));
    }

    proptest! {
        #[test]
        fn framed_chunks_should_round_trip(payload in prop::collection::vec(any::<u8>(), 1..4096)) {
            let reports = frame_message(CHANNEL_RPC, &payload)
                .unwrap_or_else(|error| panic!("framing should succeed: {error}"));
            let reconstructed: Vec<u8> = reports
                .iter()
                .flat_map(|report| {
                    let length = usize::from(report[2]);
                    report[3..3 + length].iter().copied()
                })
                .collect();

            prop_assert_eq!(reconstructed, payload);
        }
    }
}
