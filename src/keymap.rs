//! Lossless typed keymap snapshots and targeted editing.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{Error, KeyCode, KeyPosition, LayerId, ProfileId, Result, RgbColor};

const EXPECTED_ROW_LENGTHS: [usize; 4] = [2, 4, 4, 3];

/// SHA-256 revision of the exact text read from or prepared for the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Revision([u8; 32]);

impl Revision {
    /// Calculates a revision from exact UTF-8 text.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self(Sha256::digest(text.as_bytes()).into())
    }

    /// Returns the raw SHA-256 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Warning about a forward-compatible field not modeled by this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapWarning {
    path: String,
    message: String,
}

impl KeymapWarning {
    /// JSON-pointer-like location of the warning.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Human-readable warning.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Structural or semantic keymap validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapIssue {
    path: String,
    message: String,
}

impl KeymapIssue {
    /// JSON-pointer-like location of the invalid value.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Human-readable validation failure.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Complete validation result for a snapshot or draft.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeymapValidation {
    errors: Vec<KeymapIssue>,
    warnings: Vec<KeymapWarning>,
}

impl KeymapValidation {
    /// Returns structural errors that prevent a write plan from being built.
    #[must_use]
    pub fn errors(&self) -> &[KeymapIssue] {
        &self.errors
    }

    /// Returns unknown-field compatibility warnings.
    #[must_use]
    pub fn warnings(&self) -> &[KeymapWarning] {
        &self.warnings
    }

    /// Returns whether the keymap is structurally valid for v1 editing.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// String-valued joystick mode stored in a layer layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct JoystickMode(String);

impl JoystickMode {
    /// Vendor notification mode used by the Codex Micro integration.
    pub const VENDOR: &'static str = "VENDOR";

    /// Creates a joystick mode while preserving future firmware values.
    ///
    /// # Errors
    ///
    /// Rejects empty, control-character, whitespace, or overlong values.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let invalid = value.is_empty()
            || value.len() > 64
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace());
        if invalid {
            Err(Error::validation(
                "joystick mode",
                format!("{value:?} is not a valid firmware mode"),
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the firmware mode text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for JoystickMode {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Editable encoder binding within the observed three-entry encoder group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderControl {
    /// Counter-clockwise rotation binding.
    CounterClockwise,
    /// Clockwise rotation binding.
    Clockwise,
    /// Encoder-click binding.
    Press,
}

impl EncoderControl {
    const fn index(self) -> usize {
        match self {
            Self::CounterClockwise => 0,
            Self::Clockwise => 1,
            Self::Press => 2,
        }
    }
}

/// Lossless typed representation of `keymap.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeymapDocument {
    version: u32,
    #[serde(rename = "activeProfileId")]
    active_profile_id: ProfileId,
    profiles: Vec<KeymapProfile>,
    #[serde(rename = "linkedApps", default)]
    linked_apps: Vec<Value>,
    #[serde(default)]
    macros: Vec<Value>,
    #[serde(rename = "macrosGroups", default)]
    macro_groups: Vec<Value>,
    #[serde(rename = "multiActions", default)]
    multi_actions: Vec<Value>,
    #[serde(rename = "multiActionsGroups", default)]
    multi_action_groups: Vec<Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl KeymapDocument {
    /// Configuration schema version reported in the document.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Active profile identifier stored in the document.
    #[must_use]
    pub const fn active_profile_id(&self) -> ProfileId {
        self.active_profile_id
    }

    /// Existing profiles, in device order.
    #[must_use]
    pub fn profiles(&self) -> &[KeymapProfile] {
        &self.profiles
    }

    /// Opaque linked-application records preserved during edits.
    #[must_use]
    pub fn linked_apps(&self) -> &[Value] {
        &self.linked_apps
    }

    /// Opaque macro records preserved during edits.
    #[must_use]
    pub fn macros(&self) -> &[Value] {
        &self.macros
    }

    /// Opaque macro-group records preserved during edits.
    #[must_use]
    pub fn macro_groups(&self) -> &[Value] {
        &self.macro_groups
    }

    /// Opaque multi-action records preserved during edits.
    #[must_use]
    pub fn multi_actions(&self) -> &[Value] {
        &self.multi_actions
    }

    /// Opaque multi-action-group records preserved during edits.
    #[must_use]
    pub fn multi_action_groups(&self) -> &[Value] {
        &self.multi_action_groups
    }

    /// Unknown top-level fields preserved during edits.
    #[must_use]
    pub const fn extra_fields(&self) -> &Map<String, Value> {
        &self.extra
    }
}

/// Existing profile in a keymap document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeymapProfile {
    id: ProfileId,
    name: String,
    layers: Vec<KeymapLayer>,
    #[serde(rename = "macrosUsed", default)]
    macros_used: Vec<Value>,
    #[serde(rename = "multiActionsUsed", default)]
    multi_actions_used: Vec<Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl KeymapProfile {
    /// Stable profile identifier.
    #[must_use]
    pub const fn id(&self) -> ProfileId {
        self.id
    }

    /// User-visible profile name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Existing layers, in device order.
    #[must_use]
    pub fn layers(&self) -> &[KeymapLayer] {
        &self.layers
    }

    /// Opaque macro references preserved during edits.
    #[must_use]
    pub fn macros_used(&self) -> &[Value] {
        &self.macros_used
    }

    /// Opaque multi-action references preserved during edits.
    #[must_use]
    pub fn multi_actions_used(&self) -> &[Value] {
        &self.multi_actions_used
    }

    /// Unknown profile fields preserved during edits.
    #[must_use]
    pub const fn extra_fields(&self) -> &Map<String, Value> {
        &self.extra
    }
}

/// Existing layer in a keymap profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeymapLayer {
    id: LayerId,
    name: String,
    #[serde(default)]
    color: Option<RgbColor>,
    layout: KeymapLayout,
    #[serde(default)]
    lights: Option<Value>,
    #[serde(default)]
    os: Option<u32>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl KeymapLayer {
    /// Stable layer identifier.
    #[must_use]
    pub const fn id(&self) -> LayerId {
        self.id
    }

    /// User-visible layer name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Optional layer color metadata.
    #[must_use]
    pub const fn color(&self) -> Option<RgbColor> {
        self.color
    }

    /// Physical-control layout.
    #[must_use]
    pub const fn layout(&self) -> &KeymapLayout {
        &self.layout
    }

    /// Opaque layer lighting metadata preserved during edits.
    #[must_use]
    pub const fn lights(&self) -> Option<&Value> {
        self.lights.as_ref()
    }

    /// Optional operating-system selector preserved during edits.
    #[must_use]
    pub const fn os(&self) -> Option<u32> {
        self.os
    }

    /// Unknown layer fields preserved during edits.
    #[must_use]
    pub const fn extra_fields(&self) -> &Map<String, Value> {
        &self.extra
    }
}

/// Typed physical-control layout for a layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeymapLayout {
    #[serde(default)]
    encoders: Vec<Vec<KeyCode>>,
    #[serde(default)]
    buttons: Vec<Value>,
    keymap: Vec<Vec<KeyCode>>,
    #[serde(default)]
    joystick: Option<KeymapJoystick>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl KeymapLayout {
    /// Encoder binding groups.
    #[must_use]
    pub fn encoders(&self) -> &[Vec<KeyCode>] {
        &self.encoders
    }

    /// Opaque button metadata preserved during edits.
    #[must_use]
    pub fn buttons(&self) -> &[Value] {
        &self.buttons
    }

    /// Firmware key matrix in its native `[2, 4, 4, 3]` row layout.
    #[must_use]
    pub fn key_matrix(&self) -> &[Vec<KeyCode>] {
        &self.keymap
    }

    /// Joystick configuration, when present.
    #[must_use]
    pub const fn joystick(&self) -> Option<&KeymapJoystick> {
        self.joystick.as_ref()
    }

    /// Unknown layout fields preserved during edits.
    #[must_use]
    pub const fn extra_fields(&self) -> &Map<String, Value> {
        &self.extra
    }
}

/// Joystick configuration inside a keymap layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeymapJoystick {
    #[serde(rename = "type")]
    mode: JoystickMode,
    #[serde(default)]
    sectors: Vec<Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl KeymapJoystick {
    /// Current joystick mode.
    #[must_use]
    pub const fn mode(&self) -> &JoystickMode {
        &self.mode
    }

    /// Opaque radial sectors preserved during edits.
    #[must_use]
    pub fn sectors(&self) -> &[Value] {
        &self.sectors
    }

    /// Unknown joystick fields preserved during edits.
    #[must_use]
    pub const fn extra_fields(&self) -> &Map<String, Value> {
        &self.extra
    }
}

/// Immutable keymap text, typed document, revision, and diagnostics read from
/// the device.
#[derive(Debug, Clone)]
pub struct KeymapSnapshot {
    original: String,
    revision: Revision,
    document: KeymapDocument,
    validation: KeymapValidation,
}

impl KeymapSnapshot {
    /// Parses exact `keymap.json` text without discarding unknown fields.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Protocol`] when the text is not a document matching
    /// the minimally known keymap schema.
    pub fn from_json(text: impl Into<String>) -> Result<Self> {
        let original = text.into();
        let document: KeymapDocument = serde_json::from_str(&original)
            .map_err(|error| Error::protocol(format!("invalid keymap.json: {error}")))?;
        let validation = validate_document(&document);
        for warning in validation.warnings() {
            tracing::warn!(
                path = warning.path(),
                message = warning.message(),
                "unknown keymap JSON field"
            );
        }
        Ok(Self {
            revision: Revision::from_text(&original),
            original,
            document,
            validation,
        })
    }

    /// Exact text received from the firmware.
    #[must_use]
    pub fn original_json(&self) -> &str {
        &self.original
    }

    /// SHA-256 revision of [`Self::original_json`].
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Parsed lossless typed document.
    #[must_use]
    pub const fn document(&self) -> &KeymapDocument {
        &self.document
    }

    /// Structural errors and unknown-field warnings.
    #[must_use]
    pub const fn validation(&self) -> &KeymapValidation {
        &self.validation
    }

    /// Creates an editable copy tied to this exact snapshot revision.
    #[must_use]
    pub fn draft(&self) -> KeymapDraft {
        KeymapDraft {
            original: self.original.clone(),
            base_revision: self.revision,
            document: self.document.clone(),
            dirty: false,
        }
    }
}

/// Editable key bindings for existing profiles and layers.
#[derive(Debug, Clone)]
pub struct KeymapDraft {
    original: String,
    base_revision: Revision,
    document: KeymapDocument,
    dirty: bool,
}

impl KeymapDraft {
    /// Current typed draft document.
    #[must_use]
    pub const fn document(&self) -> &KeymapDocument {
        &self.document
    }

    /// Validates geometry, identifiers, and compatibility warnings.
    #[must_use]
    pub fn validation(&self) -> KeymapValidation {
        validate_document(&self.document)
    }

    /// Changes one physical key in an existing profile and layer.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] if the profile, layer, or expected
    /// `[2, 4, 4, 3]` key geometry is absent.
    pub fn set_key_binding(
        &mut self,
        profile: ProfileId,
        layer: LayerId,
        position: KeyPosition,
        keycode: KeyCode,
    ) -> Result<()> {
        let layer = find_layer_mut(&mut self.document, profile, layer)?;
        let (row, column) = matrix_location(position);
        let binding = layer
            .layout
            .keymap
            .get_mut(row)
            .and_then(|row_values| row_values.get_mut(column))
            .ok_or_else(|| {
                Error::validation(
                    "keymap geometry",
                    format!("physical position {} is missing", position.get()),
                )
            })?;
        if *binding != keycode {
            *binding = keycode;
            self.dirty = true;
        }
        Ok(())
    }

    /// Changes one of the three bindings in the existing encoder group.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] if the profile, layer, or expected
    /// encoder group is absent.
    pub fn set_encoder_binding(
        &mut self,
        profile: ProfileId,
        layer: LayerId,
        control: EncoderControl,
        keycode: KeyCode,
    ) -> Result<()> {
        let layer = find_layer_mut(&mut self.document, profile, layer)?;
        let binding = layer
            .layout
            .encoders
            .first_mut()
            .and_then(|encoder| encoder.get_mut(control.index()))
            .ok_or_else(|| Error::validation("encoder layout", "expected one three-entry group"))?;
        if *binding != keycode {
            *binding = keycode;
            self.dirty = true;
        }
        Ok(())
    }

    /// Changes the existing joystick mode without replacing its sectors or
    /// unknown metadata.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] if the selected layer has no joystick
    /// configuration.
    pub fn set_joystick_mode(
        &mut self,
        profile: ProfileId,
        layer: LayerId,
        mode: JoystickMode,
    ) -> Result<()> {
        let layer = find_layer_mut(&mut self.document, profile, layer)?;
        let joystick = layer.layout.joystick.as_mut().ok_or_else(|| {
            Error::validation("joystick layout", "selected layer has no joystick")
        })?;
        if joystick.mode != mode {
            joystick.mode = mode;
            self.dirty = true;
        }
        Ok(())
    }

    /// Validates and serializes the draft into a compare-before-write plan.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] when any structural errors remain or
    /// serialization unexpectedly fails.
    pub fn into_write_plan(self) -> Result<KeymapWritePlan> {
        let validation = validate_document(&self.document);
        if let Some(issue) = validation.errors().first() {
            return Err(Error::validation(
                "keymap draft",
                format!("{}: {}", issue.path(), issue.message()),
            ));
        }
        let candidate = if self.dirty {
            serde_json::to_string(&self.document).map_err(|error| {
                Error::validation("keymap draft", format!("serialization failed: {error}"))
            })?
        } else {
            self.original.clone()
        };
        Ok(KeymapWritePlan {
            expected_revision: self.base_revision,
            candidate_revision: Revision::from_text(&candidate),
            original: self.original,
            candidate,
        })
    }
}

/// Validated compare-before-write keymap transaction input.
#[derive(Debug, Clone)]
pub struct KeymapWritePlan {
    expected_revision: Revision,
    candidate_revision: Revision,
    original: String,
    candidate: String,
}

impl KeymapWritePlan {
    /// Revision that must still be live before applying the plan.
    #[must_use]
    pub const fn expected_revision(&self) -> Revision {
        self.expected_revision
    }

    /// Revision expected after a successful apply.
    #[must_use]
    pub const fn candidate_revision(&self) -> Revision {
        self.candidate_revision
    }

    /// Exact original text that callers should durably back up.
    #[must_use]
    pub fn original_json(&self) -> &str {
        &self.original
    }

    /// Exact candidate text that would be written.
    #[must_use]
    pub fn candidate_json(&self) -> &str {
        &self.candidate
    }

    /// Returns whether applying the plan requires no device write.
    #[must_use]
    pub fn is_unchanged(&self) -> bool {
        self.original == self.candidate
    }
}

/// Failure encountered while attempting to restore the original keymap.
#[cfg(feature = "persistent-writes")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackFailure {
    message: String,
}

#[cfg(feature = "persistent-writes")]
impl RollbackFailure {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Human-readable rollback failure.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Final state reported by a feature-gated keymap apply operation.
#[cfg(feature = "persistent-writes")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeymapApplyOutcome {
    /// Candidate and original were identical, so no write occurred.
    Unchanged,
    /// Candidate was written and verified exactly.
    Applied {
        /// Verified candidate revision.
        revision: Revision,
    },
    /// Candidate verification failed, but the exact original was restored and
    /// verified.
    RolledBack {
        /// Failure that triggered rollback.
        cause: String,
    },
    /// Candidate verification failed and the rollback could not be verified.
    RollbackFailed {
        /// Failure that triggered rollback.
        cause: String,
        /// Rollback failure details.
        rollback: RollbackFailure,
    },
}

fn find_layer_mut(
    document: &mut KeymapDocument,
    profile_id: ProfileId,
    layer_id: LayerId,
) -> Result<&mut KeymapLayer> {
    let profile = document
        .profiles
        .iter_mut()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| {
            Error::validation("profile id", format!("profile {profile_id} does not exist"))
        })?;
    profile
        .layers
        .iter_mut()
        .find(|layer| layer.id == layer_id)
        .ok_or_else(|| {
            Error::validation(
                "layer id",
                format!("layer {layer_id} does not exist in profile {profile_id}"),
            )
        })
}

const fn matrix_location(position: KeyPosition) -> (usize, usize) {
    match position.get() {
        0..=1 => (0, position.get() as usize),
        2..=5 => (1, (position.get() - 2) as usize),
        6..=9 => (2, (position.get() - 6) as usize),
        10..=12 => (3, (position.get() - 10) as usize),
        _ => (usize::MAX, usize::MAX),
    }
}

fn validate_document(document: &KeymapDocument) -> KeymapValidation {
    let mut validation = KeymapValidation::default();
    collect_unknown_fields(&mut validation.warnings, "", &document.extra);

    if document.profiles.is_empty() {
        validation
            .errors
            .push(issue("/profiles", "at least one profile is required"));
    }

    let mut profile_ids = HashSet::new();
    for (profile_index, profile) in document.profiles.iter().enumerate() {
        let profile_path = format!("/profiles/{profile_index}");
        if !profile_ids.insert(profile.id) {
            validation.errors.push(issue(
                format!("{profile_path}/id"),
                format!("duplicate profile id {}", profile.id),
            ));
        }
        collect_unknown_fields(&mut validation.warnings, &profile_path, &profile.extra);
        if profile.layers.is_empty() {
            validation.errors.push(issue(
                format!("{profile_path}/layers"),
                "at least one layer is required",
            ));
        }
        let mut layer_ids = HashSet::new();
        for (layer_index, layer) in profile.layers.iter().enumerate() {
            let layer_path = format!("{profile_path}/layers/{layer_index}");
            if !layer_ids.insert(layer.id) {
                validation.errors.push(issue(
                    format!("{layer_path}/id"),
                    format!("duplicate layer id {}", layer.id),
                ));
            }
            collect_unknown_fields(&mut validation.warnings, &layer_path, &layer.extra);
            validate_layout(&layer.layout, &layer_path, &mut validation);
        }
    }

    if !document
        .profiles
        .iter()
        .any(|profile| profile.id == document.active_profile_id)
    {
        validation.errors.push(issue(
            "/activeProfileId",
            format!("profile {} does not exist", document.active_profile_id),
        ));
    }
    validation
}

fn validate_layout(layout: &KeymapLayout, layer_path: &str, validation: &mut KeymapValidation) {
    let layout_path = format!("{layer_path}/layout");
    collect_unknown_fields(&mut validation.warnings, &layout_path, &layout.extra);
    if layout.keymap.len() != EXPECTED_ROW_LENGTHS.len() {
        validation.errors.push(issue(
            format!("{layout_path}/keymap"),
            format!("expected 4 rows, found {}", layout.keymap.len()),
        ));
    }
    for (row_index, expected) in EXPECTED_ROW_LENGTHS.iter().copied().enumerate() {
        if let Some(row) = layout
            .keymap
            .get(row_index)
            .filter(|row| row.len() != expected)
        {
            validation.errors.push(issue(
                format!("{layout_path}/keymap/{row_index}"),
                format!("expected {expected} entries, found {}", row.len()),
            ));
        }
    }
    if layout.encoders.len() != 1 || layout.encoders.first().map(Vec::len) != Some(3) {
        validation.errors.push(issue(
            format!("{layout_path}/encoders"),
            "expected one three-entry encoder group",
        ));
    }
    if let Some(joystick) = &layout.joystick {
        collect_unknown_fields(
            &mut validation.warnings,
            &format!("{layout_path}/joystick"),
            &joystick.extra,
        );
    }
}

fn collect_unknown_fields(
    warnings: &mut Vec<KeymapWarning>,
    parent: &str,
    fields: &Map<String, Value>,
) {
    warnings.extend(fields.keys().map(|field| KeymapWarning {
        path: format!("{parent}/{}", escape_json_pointer(field)),
        message: "field is not modeled by libremicro v1 and was preserved".to_owned(),
    }));
}

fn escape_json_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn issue(path: impl Into<String>, message: impl Into<String>) -> KeymapIssue {
    KeymapIssue {
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/keymap-0.6.2.json");

    fn snapshot() -> KeymapSnapshot {
        KeymapSnapshot::from_json(FIXTURE)
            .unwrap_or_else(|error| panic!("fixture should parse: {error}"))
    }

    #[test]
    fn fixture_should_match_expected_codex_micro_geometry() {
        let snapshot = snapshot();

        assert!(snapshot.validation().is_valid());
    }

    #[test]
    fn from_json_should_report_unknown_fields_with_json_pointer() {
        let mut value: Value = serde_json::from_str(FIXTURE)
            .unwrap_or_else(|error| panic!("fixture should parse as JSON: {error}"));
        value["future/field"] = json!({"enabled": true});
        let snapshot = KeymapSnapshot::from_json(value.to_string())
            .unwrap_or_else(|error| panic!("extended fixture should parse: {error}"));

        assert_eq!(snapshot.validation().warnings()[0].path(), "/future~1field");
    }

    #[test]
    fn edited_plan_should_preserve_unknown_json_values() {
        let mut value: Value = serde_json::from_str(FIXTURE)
            .unwrap_or_else(|error| panic!("fixture should parse as JSON: {error}"));
        value["future"] = json!({"nested": [1, 2, 3]});
        let snapshot = KeymapSnapshot::from_json(value.to_string())
            .unwrap_or_else(|error| panic!("extended fixture should parse: {error}"));
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
        let plan = draft
            .into_write_plan()
            .unwrap_or_else(|error| panic!("plan should build: {error}"));
        let candidate: Value = serde_json::from_str(plan.candidate_json())
            .unwrap_or_else(|error| panic!("candidate should be JSON: {error}"));

        assert_eq!(candidate["future"], json!({"nested": [1, 2, 3]}));
    }

    #[test]
    fn edited_plan_should_preserve_opaque_configuration_sections() {
        let mut value: Value = serde_json::from_str(FIXTURE)
            .unwrap_or_else(|error| panic!("fixture should parse as JSON: {error}"));
        value["linkedApps"] = json!([{"application": "example.invalid", "profileId": 0}]);
        value["macros"] = json!([{"id": 7, "steps": [{"futureStep": true}]}]);
        value["multiActions"] = json!([{"id": 8, "action": {"futureAction": 9}}]);
        let expected_linked_apps = value["linkedApps"].clone();
        let expected_macros = value["macros"].clone();
        let expected_actions = value["multiActions"].clone();
        let snapshot = KeymapSnapshot::from_json(value.to_string())
            .unwrap_or_else(|error| panic!("extended fixture should parse: {error}"));
        let mut draft = snapshot.draft();
        draft
            .set_key_binding(
                ProfileId::new(0),
                LayerId::new(0),
                KeyPosition::new(0)
                    .unwrap_or_else(|error| panic!("position should be valid: {error}")),
                KeyCode::new("KC_A")
                    .unwrap_or_else(|error| panic!("keycode should be valid: {error}")),
            )
            .unwrap_or_else(|error| panic!("edit should succeed: {error}"));
        let plan = draft
            .into_write_plan()
            .unwrap_or_else(|error| panic!("plan should build: {error}"));
        let candidate: Value = serde_json::from_str(plan.candidate_json())
            .unwrap_or_else(|error| panic!("candidate should be JSON: {error}"));

        assert_eq!(candidate["linkedApps"], expected_linked_apps);
        assert_eq!(candidate["macros"], expected_macros);
        assert_eq!(candidate["multiActions"], expected_actions);
    }

    #[test]
    fn set_key_binding_should_target_firmware_row_major_position() {
        let snapshot = snapshot();
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

        assert_eq!(
            draft.document().profiles()[0].layers()[0]
                .layout()
                .key_matrix()[1][1]
                .as_str(),
            "KC_B"
        );
    }

    #[test]
    fn unchanged_draft_should_reuse_exact_original_text() {
        let snapshot = snapshot();
        let plan = snapshot
            .draft()
            .into_write_plan()
            .unwrap_or_else(|error| panic!("plan should build: {error}"));

        assert!(plan.is_unchanged());
    }

    #[test]
    fn malformed_geometry_should_prevent_write_plan() {
        let mut value: Value = serde_json::from_str(FIXTURE)
            .unwrap_or_else(|error| panic!("fixture should parse as JSON: {error}"));
        value["profiles"][0]["layers"][0]["layout"]["keymap"][0] = json!(["KC_A"]);
        let snapshot = KeymapSnapshot::from_json(value.to_string())
            .unwrap_or_else(|error| panic!("structurally bad fixture should still parse: {error}"));

        let error = snapshot
            .draft()
            .into_write_plan()
            .expect_err("invalid geometry must block planning");

        assert!(matches!(error, Error::Validation { .. }));
    }

    #[test]
    fn set_encoder_binding_should_preserve_other_encoder_bindings() {
        let snapshot = snapshot();
        let mut draft = snapshot.draft();
        draft
            .set_encoder_binding(
                ProfileId::new(0),
                LayerId::new(0),
                EncoderControl::Press,
                KeyCode::new("KC_MUTE")
                    .unwrap_or_else(|error| panic!("keycode should be valid: {error}")),
            )
            .unwrap_or_else(|error| panic!("encoder edit should succeed: {error}"));

        assert_eq!(
            draft.document().profiles()[0].layers()[0]
                .layout()
                .encoders()[0][2]
                .as_str(),
            "KC_MUTE"
        );
    }

    #[test]
    fn set_joystick_mode_should_preserve_sectors() {
        let snapshot = snapshot();
        let mut draft = snapshot.draft();
        draft
            .set_joystick_mode(
                ProfileId::new(0),
                LayerId::new(0),
                JoystickMode::new("RADIAL")
                    .unwrap_or_else(|error| panic!("mode should be valid: {error}")),
            )
            .unwrap_or_else(|error| panic!("joystick edit should succeed: {error}"));
        let joystick = draft.document().profiles()[0].layers()[0]
            .layout()
            .joystick()
            .unwrap_or_else(|| panic!("fixture should contain a joystick"));

        assert_eq!(
            (joystick.mode().as_str(), joystick.sectors().len()),
            ("RADIAL", 0)
        );
    }

    #[test]
    fn revision_should_change_when_exact_text_changes() {
        let first = Revision::from_text("{}");
        let second = Revision::from_text("{}\n");

        assert_ne!(first, second);
    }
}
