//! Configuration persistence for snappea settings

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use cosmic::iced::Color;
use serde::{Deserialize, Serialize};

use crate::fl;

/// Serializable color representation for config storage
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapeColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Default for ShapeColor {
    fn default() -> Self {
        // Default red color matching current arrow color
        Self {
            r: 0.9,
            g: 0.1,
            b: 0.1,
        }
    }
}

impl From<ShapeColor> for Color {
    fn from(c: ShapeColor) -> Self {
        Color::from_rgb(c.r, c.g, c.b)
    }
}

impl From<Color> for ShapeColor {
    fn from(c: Color) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

impl ShapeColor {
    /// Convert to image crate RGBA format (0-255)
    pub fn to_rgba_u8(self) -> [u8; 4] {
        [
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
            255,
        ]
    }
}

/// Save location for screenshots (Pictures or Documents)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SaveLocation {
    #[default]
    Pictures,
    Documents,
}

/// Shape annotation tool type (for split button selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShapeTool {
    #[default]
    Arrow,
    Circle,
    Rectangle,
}

impl ShapeTool {
    /// Get the next shape tool in the cycle
    pub fn next(self) -> Self {
        match self {
            ShapeTool::Arrow => ShapeTool::Circle,
            ShapeTool::Circle => ShapeTool::Rectangle,
            ShapeTool::Rectangle => ShapeTool::Arrow,
        }
    }

    /// Get the icon name for this shape tool
    pub fn icon_name(self) -> &'static str {
        match self {
            ShapeTool::Arrow => "arrow-symbolic",
            ShapeTool::Circle => "circle-symbolic",
            ShapeTool::Rectangle => "square-symbolic",
        }
    }

    /// Get the tooltip text for this shape tool
    pub fn tooltip(self) -> String {
        match self {
            ShapeTool::Arrow => fl!("draw-arrow"),
            ShapeTool::Circle => fl!("draw-circle"),
            ShapeTool::Rectangle => fl!("draw-rectangle"),
        }
    }
}

/// Toolbar position on screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ToolbarPosition {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

/// Redaction tool type (for split button selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RedactTool {
    #[default]
    Redact,
    Pixelate,
}

impl RedactTool {
    /// Get the next redact tool in the cycle
    pub fn next(self) -> Self {
        match self {
            RedactTool::Redact => RedactTool::Pixelate,
            RedactTool::Pixelate => RedactTool::Redact,
        }
    }

    /// Get the icon name for this redact tool
    pub fn icon_name(self) -> &'static str {
        match self {
            RedactTool::Redact => "redact-symbolic",
            RedactTool::Pixelate => "pixelate-symbolic",
        }
    }

    /// Get the tooltip text for this redact tool
    pub fn tooltip(self) -> String {
        match self {
            RedactTool::Redact => fl!("redact-tool"),
            RedactTool::Pixelate => fl!("pixelate-tool"),
        }
    }

    /// Get the index of this tool (for indicator dots)
    pub fn index(self) -> usize {
        match self {
            RedactTool::Redact => 0,
            RedactTool::Pixelate => 1,
        }
    }
}

/// Video container format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Container {
    #[default]
    Mp4,
    Webm,
    Mkv,
}

impl Container {
    /// Get file extension for this container
    pub fn extension(&self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Webm => "webm",
            Container::Mkv => "mkv",
        }
    }

    /// Get GStreamer muxer element name
    pub fn muxer_element(&self) -> &'static str {
        match self {
            Container::Mp4 => "mp4mux",
            Container::Webm => "webmmux",
            Container::Mkv => "matroskamux",
        }
    }
}

/// Application configuration persisted between sessions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct SnapPeaConfig {
    /// Whether to show the magnifier when dragging selection corners
    pub magnifier_enabled: bool,
    /// Where to save screenshots (Pictures or Documents folder)
    pub save_location: SaveLocation,
    /// Whether to also copy to clipboard when saving to file
    pub copy_to_clipboard_on_save: bool,
    /// Primary shape tool shown in the button
    pub primary_shape_tool: ShapeTool,
    /// Color for shape annotations
    pub shape_color: ShapeColor,
    /// Whether to add shadow/border to shapes
    pub shape_shadow: bool,
    /// Primary redact tool shown in the button
    pub primary_redact_tool: RedactTool,
    /// Pixelation block size (larger = more pixelated, range 4-64)
    pub pixelation_block_size: u32,
    /// Toolbar position on screen
    pub toolbar_position: ToolbarPosition,
    /// Toolbar opacity when not hovered (0.0-1.0)
    #[serde(default = "default_toolbar_unhovered_opacity")]
    pub toolbar_unhovered_opacity: f32,
    /// Video encoder to use (None = auto-detect hardware encoder)
    pub video_encoder: Option<String>,
    /// Video container format
    pub video_container: Container,
    /// Recording framerate (30 or 60)
    pub video_framerate: u32,
    /// Whether to show cursor in recordings
    pub video_show_cursor: bool,
    /// Pencil color for recording annotations (RGB, 0.0-1.0)
    #[serde(default = "default_pencil_color")]
    pub pencil_color: ShapeColor,
    /// Duration in seconds before pencil strokes fade away
    #[serde(default = "default_pencil_fade_duration")]
    pub pencil_fade_duration: f32,
    /// Pencil line thickness in pixels
    #[serde(default = "default_pencil_thickness")]
    pub pencil_thickness: f32,
    /// Whether to hide toolbar to system tray when recording
    #[serde(default)]
    pub hide_toolbar_to_tray: bool,
}

fn default_pencil_color() -> ShapeColor {
    ShapeColor {
        r: 1.0,
        g: 0.9,
        b: 0.0,
    } // Yellow
}

fn default_pencil_fade_duration() -> f32 {
    3.0 // 3 seconds
}

fn default_pencil_thickness() -> f32 {
    3.0 // 3 pixels
}

fn default_toolbar_unhovered_opacity() -> f32 {
    0.5
}

impl SnapPeaConfig {
    /// Configuration ID for cosmic-config
    pub const ID: &'static str = "io.github.hojjatabdollahi.snappea";

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

impl Default for SnapPeaConfig {
    fn default() -> Self {
        Self {
            // Magnifier enabled by default for precise selection
            magnifier_enabled: true,
            // Default to Pictures folder
            save_location: SaveLocation::Pictures,
            // Don't copy to clipboard by default when saving
            copy_to_clipboard_on_save: false,
            // Default to Arrow as primary shape tool
            primary_shape_tool: ShapeTool::Arrow,
            // Default red color for shapes
            shape_color: ShapeColor::default(),
            // Shadow enabled by default (matches current arrow behavior)
            shape_shadow: true,
            // Default to Redact as primary redact tool
            primary_redact_tool: RedactTool::Redact,
            // Default pixelation block size (16 is a good balance)
            pixelation_block_size: 16,
            // Default toolbar position at the bottom
            toolbar_position: ToolbarPosition::Bottom,
            // Default toolbar opacity when idle
            toolbar_unhovered_opacity: default_toolbar_unhovered_opacity(),
            // Recording defaults
            video_encoder: None, // Auto-detect
            video_container: Container::Mp4,
            video_framerate: 60,
            video_show_cursor: true, // Show cursor by default
            pencil_color: default_pencil_color(),
            pencil_fade_duration: default_pencil_fade_duration(),
            pencil_thickness: default_pencil_thickness(),
            hide_toolbar_to_tray: false,
        }
    }
}
