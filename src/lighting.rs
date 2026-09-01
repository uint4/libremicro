//! Typed volatile-lighting commands.

use serde_json::{Map, Value, json};

use crate::{AgentId, Brightness, RgbColor, Speed};

/// Lighting effects accepted by firmware 0.6.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LightingEffect {
    /// LEDs off.
    Off = 0,
    /// Solid color.
    Solid = 1,
    /// Snake animation.
    Snake = 2,
    /// Rainbow animation.
    Rainbow = 3,
    /// Breathing animation.
    Breath = 4,
    /// Gradient animation.
    Gradient = 5,
    /// Shallow breathing animation.
    ShallowBreath = 6,
}

impl LightingEffect {
    pub(crate) const fn code(self) -> u8 {
        self as u8
    }
}

/// Shared configuration for a key or ambient lighting zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightingZone {
    effect: LightingEffect,
    brightness: Brightness,
    speed: Speed,
    magic: u32,
    color: RgbColor,
}

impl LightingZone {
    /// Creates a zone with full brightness, medium animation speed, and the
    /// firmware's observed default magic value of `1`.
    #[must_use]
    pub fn new(effect: LightingEffect, color: RgbColor) -> Self {
        Self {
            effect,
            brightness: Brightness::default(),
            speed: Speed::default(),
            magic: 1,
            color,
        }
    }

    /// Sets normalized brightness.
    #[must_use]
    pub const fn with_brightness(mut self, brightness: Brightness) -> Self {
        self.brightness = brightness;
        self
    }

    /// Sets normalized animation speed.
    #[must_use]
    pub const fn with_speed(mut self, speed: Speed) -> Self {
        self.speed = speed;
        self
    }

    /// Sets the firmware-specific magic value while preserving a typed API.
    #[must_use]
    pub const fn with_magic(mut self, magic: u32) -> Self {
        self.magic = magic;
        self
    }

    pub(crate) fn short_json(self) -> Value {
        json!({
            "e": self.effect.code(),
            "b": self.brightness.get(),
            "s": self.speed.get(),
            "m": self.magic,
            "c": self.color.get(),
        })
    }
}

/// Optional base-lighting updates for the key and ambient zones.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BaseLighting {
    keys: Option<LightingZone>,
    ambient: Option<LightingZone>,
}

impl BaseLighting {
    /// Creates an update that changes no zones until a builder method is used.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            keys: None,
            ambient: None,
        }
    }

    /// Sets the key-lighting zone update.
    #[must_use]
    pub const fn with_keys(mut self, zone: LightingZone) -> Self {
        self.keys = Some(zone);
        self
    }

    /// Sets the ambient-lighting zone update.
    #[must_use]
    pub const fn with_ambient(mut self, zone: LightingZone) -> Self {
        self.ambient = Some(zone);
        self
    }

    /// Returns whether the update contains no zones.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.keys.is_none() && self.ambient.is_none()
    }

    pub(crate) fn to_json(self) -> Value {
        let mut object = Map::new();
        if let Some(keys) = self.keys {
            object.insert("keys".to_owned(), keys.short_json());
        }
        if let Some(ambient) = self.ambient {
            object.insert("ambient".to_owned(), ambient.short_json());
        }
        Value::Object(object)
    }
}

/// Per-agent-key lighting update sent through `v.oai.thstatus`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadLighting {
    id: AgentId,
    color: RgbColor,
    brightness: Brightness,
    effect: LightingEffect,
    speed: Speed,
    sync_keys: bool,
    sync_ambient: bool,
}

impl ThreadLighting {
    /// Creates a solid, fully bright update for an agent key.
    #[must_use]
    pub fn new(id: AgentId, color: RgbColor) -> Self {
        Self {
            id,
            color,
            brightness: Brightness::default(),
            effect: LightingEffect::Solid,
            speed: Speed::default(),
            sync_keys: false,
            sync_ambient: false,
        }
    }

    /// Sets normalized brightness.
    #[must_use]
    pub const fn with_brightness(mut self, brightness: Brightness) -> Self {
        self.brightness = brightness;
        self
    }

    /// Sets the lighting effect.
    #[must_use]
    pub const fn with_effect(mut self, effect: LightingEffect) -> Self {
        self.effect = effect;
        self
    }

    /// Sets normalized animation speed.
    #[must_use]
    pub const fn with_speed(mut self, speed: Speed) -> Self {
        self.speed = speed;
        self
    }

    /// Sets whether the key update is synchronized to the base key zone.
    #[must_use]
    pub const fn with_key_sync(mut self, enabled: bool) -> Self {
        self.sync_keys = enabled;
        self
    }

    /// Sets whether the key update is synchronized to the ambient zone.
    #[must_use]
    pub const fn with_ambient_sync(mut self, enabled: bool) -> Self {
        self.sync_ambient = enabled;
        self
    }

    pub(crate) fn to_json(self) -> Value {
        json!({
            "id": self.id.get(),
            "c": self.color.get(),
            "b": self.brightness.get(),
            "e": self.effect.code(),
            "s": self.speed.get(),
            "sk": u8::from(self.sync_keys),
            "sa": u8::from(self.sync_ambient),
        })
    }
}
