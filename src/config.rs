//! Configuration persistence for blazingshot settings

use cosmic::cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};
use serde::{Deserialize, Serialize};

/// Application configuration persisted between sessions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct BlazingshotConfig {
    /// Whether to show the magnifier when dragging selection corners
    pub magnifier_enabled: bool,
}

impl BlazingshotConfig {
    /// Configuration ID for cosmic-config
    pub const ID: &'static str = "org.freedesktop.impl.portal.blazingshot";

    /// Load configuration from disk, or return defaults if unavailable
    pub fn load() -> Self {
        match cosmic_config::Config::new(Self::ID, Self::VERSION) {
            Ok(config) => match Self::get_entry(&config) {
                Ok(entry) => entry,
                Err((errs, entry)) => {
                    log::warn!("Error loading config, using defaults: {:?}", errs);
                    entry
                }
            },
            Err(err) => {
                log::warn!("Could not create config handler: {:?}", err);
                Self::default()
            }
        }
    }

    /// Save configuration to disk
    pub fn save(&self) {
        match cosmic_config::Config::new(Self::ID, Self::VERSION) {
            Ok(config) => {
                if let Err(err) = self.write_entry(&config) {
                    log::error!("Failed to save config: {:?}", err);
                }
            }
            Err(err) => {
                log::error!("Could not create config handler for saving: {:?}", err);
            }
        }
    }
}

impl Default for BlazingshotConfig {
    fn default() -> Self {
        Self {
            // Magnifier enabled by default for precise selection
            magnifier_enabled: true,
        }
    }
}
