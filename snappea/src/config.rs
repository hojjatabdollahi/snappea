// SPDX-License-Identifier: GPL-3.0-only

//! Persistent settings.

use crate::fl;
use crate::geometry::{Choice, Rect};
use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};
use viewer_tools::annotate::{AnnotateColor, ShapeKind};

/// Starting stroke thickness for shapes, in logical units.
pub const SHAPE_THICKNESS_DEFAULT: f32 = 3.0;
/// Starting text size, in logical pixels.
pub const TEXT_SIZE_DEFAULT: f32 = 24.0;

/// Save location choice for UI selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SaveLocationChoice {
    /// The clipboard and no file. Every folder choice also copies to the clipboard.
    Clipboard,
    #[default]
    Pictures,
    Documents,
    Custom,
}

/// Save location choice for videos
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VideoSaveLocationChoice {
    #[default]
    Videos,
    Custom,
}

/// Shape annotation tool type (for split button selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShapeTool {
    #[default]
    Arrow,
    Line,
    // Named `Circle` in saved configs. Drawn and labeled as an ellipse.
    Circle,
    Rectangle,
    /// Filled rectangle
    Block,
    Star,
    Text,
    Pen,
    Highlighter,
    Pixelate,
    Magnifier,
}

impl ShapeTool {
    /// Every tool, in picker and cycle order.
    pub const ALL: &'static [Self] = &[
        Self::Arrow,
        Self::Line,
        Self::Circle,
        Self::Rectangle,
        Self::Block,
        Self::Star,
        Self::Text,
        Self::Pen,
        Self::Highlighter,
        Self::Pixelate,
        Self::Magnifier,
    ];

    /// The shapes, in dropdown order. cosmic-viewer's set without the polygon.
    pub const SHAPES: &'static [Self] = &[
        Self::Rectangle,
        Self::Block,
        Self::Circle,
        Self::Arrow,
        Self::Line,
        Self::Star,
    ];

    /// The freehand tools, which share a dropdown.
    pub const FREEHAND: &'static [Self] = &[Self::Pen, Self::Highlighter];

    /// The shape this tool draws, if it draws one.
    #[must_use]
    pub const fn shape_kind(self) -> Option<ShapeKind> {
        Some(match self {
            Self::Rectangle => ShapeKind::Rectangle,
            Self::Block => ShapeKind::Block,
            Self::Circle => ShapeKind::Ellipse,
            Self::Arrow => ShapeKind::Arrow,
            Self::Line => ShapeKind::Line,
            Self::Star => ShapeKind::Star,
            _ => return None,
        })
    }

    /// Get the next shape tool in the cycle
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// Position of this tool in the cycle
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|t| *t == self)
            .unwrap_or_default()
    }

    /// Symbolic icon name, matching cosmic-viewer.
    pub const fn icon_name(self) -> &'static str {
        match self {
            Self::Arrow => "insert-arrow-symbolic",
            Self::Line => "insert-line-symbolic",
            Self::Circle => "insert-ellipse-symbolic",
            Self::Rectangle => "insert-rectangle-symbolic",
            Self::Pen => "pencil-symbolic",
            Self::Highlighter => "text-highlight-symbolic",
            Self::Block => "insert-rectangle-filled-symbolic",
            Self::Star => "insert-star-symbolic",
            Self::Text => "insert-text-symbolic",
            Self::Pixelate => "insert-blur-symbolic",
            Self::Magnifier => "edit-find-symbolic",
        }
    }

    /// This tool if it is one of `group`, or the group's first. Stored slots can
    /// name a tool from another version.
    #[must_use]
    pub fn or_first_of(self, group: &'static [Self]) -> Self {
        if group.contains(&self) {
            self
        } else {
            group[0]
        }
    }

    /// Short label shown next to the icon in the tool picker
    pub fn label(self) -> String {
        match self {
            Self::Arrow => fl!("arrow"),
            Self::Line => fl!("line"),
            Self::Circle => fl!("ellipse"),
            Self::Rectangle => fl!("rectangle"),
            Self::Pen => fl!("pen"),
            Self::Highlighter => fl!("highlighter"),
            Self::Block => fl!("filled-rectangle"),
            Self::Star => fl!("star"),
            Self::Text => fl!("text-tool"),
            Self::Pixelate => fl!("pixelate-tool"),
            Self::Magnifier => fl!("magnifier-tool"),
        }
    }
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
    pub const fn next(self) -> Self {
        match self {
            Self::Redact => Self::Pixelate,
            Self::Pixelate => Self::Redact,
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
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::Mkv => "mkv",
        }
    }

    /// Get `GStreamer` muxer element name
    pub const fn muxer_element(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4mux",
            Self::Webm => "webmmux",
            Self::Mkv => "matroskamux",
        }
    }
}

/// What the last capture did. The next one starts in the same mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CaptureMode {
    #[default]
    Screenshot,
    Video,
}

/// The last capture's target, restored on the next one where it still fits.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LastTarget {
    #[default]
    None,
    Region(Rect),
    Output(String),
    AllScreens,
}

impl From<&Choice> for LastTarget {
    fn from(choice: &Choice) -> Self {
        match choice {
            Choice::Rectangle(r, _) if r.dimensions().is_some() => Self::Region(*r),
            Choice::Rectangle(..) | Choice::Output(None) => Self::None,
            Choice::Output(Some(name)) => Self::Output(name.clone()),
            Choice::AllScreens => Self::AllScreens,
        }
    }
}

/// Application configuration persisted between sessions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct Config {
    /// Whether to show the magnifier when dragging selection corners
    pub magnifier_enabled: bool,
    /// Where to save screenshots (Pictures, Documents, or Custom)
    pub save_location: SaveLocationChoice,
    /// Custom path for saving screenshots (used when `save_location` is Custom)
    #[serde(default = "default_custom_save_path")]
    pub custom_save_path: String,
    /// Where to save videos (Videos or Custom)
    #[serde(default)]
    pub video_save_location: VideoSaveLocationChoice,
    /// Custom path for saving videos (used when `video_save_location` is Custom)
    #[serde(default = "default_video_custom_save_path")]
    pub video_custom_save_path: String,
    /// Primary shape tool shown in the button
    pub primary_shape_tool: ShapeTool,
    /// Which shape the shapes dropdown is set to, independent of the last tool picked.
    #[serde(default = "default_shape_choice")]
    pub shape_choice: ShapeTool,
    /// Which of the pen and the highlighter their shared slot is set to.
    #[serde(default = "default_freehand_choice")]
    pub freehand_choice: ShapeTool,
    /// Color for shape annotations
    pub shape_color: AnnotateColor,
    /// Stroke thickness (logical units) used for new shape annotations
    #[serde(default = "default_shape_thickness")]
    pub shape_thickness: f32,
    #[serde(default = "default_highlighter_thickness")]
    pub highlighter_thickness: f32,
    /// Font size (logical units) used for new text annotations
    #[serde(default = "default_text_font_size")]
    pub text_font_size: f32,
    /// Primary redact tool shown in the button
    pub primary_redact_tool: RedactTool,
    /// Pixelation block size (larger = more pixelated, range 4-64)
    pub pixelation_block_size: u32,
    /// Scan a selection for QR codes as soon as it is made
    #[serde(default = "default_recognize_qr_codes")]
    pub recognize_qr_codes: bool,
    /// Magnifier zoom level (range 1.5-10.0)
    #[serde(default = "default_magnifier_magnification")]
    pub magnifier_magnification: f32,
    /// Delay in seconds for the "delayed screenshot" toolbar button
    #[serde(default)]
    pub capture_delay_secs: u32,
    /// Video encoder to use (None = auto-detect hardware encoder)
    pub video_encoder: Option<String>,
    /// Video container format
    pub video_container: Container,
    /// Recording framerate (30 or 60)
    pub video_framerate: u32,
    /// Whether to show cursor in recordings
    pub video_show_cursor: bool,
    /// Whether a blinking red border marks the recording region.
    #[serde(default = "default_video_region_border")]
    pub video_region_border: bool,
    /// Whether to record teh system audio
    #[serde(default)]
    pub video_system_audio: bool,
    /// Whether a screenshot includes the pointer. Separate from [`Self::video_show_cursor`].
    #[serde(default)]
    pub show_cursor: bool,
    /// Mode of the last capture, screenshot or video.
    pub last_mode: CaptureMode,
    /// Target of the last capture: a region, an output or every screen.
    pub last_target: LastTarget,
    /// Pencil color for recording annotations (RGB, 0.0-1.0)
    #[serde(default = "default_pencil_color")]
    pub pencil_color: AnnotateColor,
    /// Duration in seconds before pencil strokes fade away
    #[serde(default = "default_pencil_fade_duration")]
    pub pencil_fade_duration: f32,
    /// Pencil line thickness in pixels
    #[serde(default = "default_pencil_thickness")]
    pub pencil_thickness: f32,
}

const fn default_magnifier_magnification() -> f32 {
    2.5
}

const fn default_recognize_qr_codes() -> bool {
    true
}

const fn default_video_region_border() -> bool {
    true
}

const fn default_highlighter_thickness() -> f32 {
    12.0
}

const fn default_pencil_color() -> AnnotateColor {
    AnnotateColor::RED
}

const fn default_pencil_fade_duration() -> f32 {
    3.0
}

const fn default_pencil_thickness() -> f32 {
    3.0
}

const fn default_shape_thickness() -> f32 {
    SHAPE_THICKNESS_DEFAULT
}

const fn default_shape_choice() -> ShapeTool {
    ShapeTool::Arrow
}

const fn default_freehand_choice() -> ShapeTool {
    ShapeTool::Pen
}

const fn default_text_font_size() -> f32 {
    TEXT_SIZE_DEFAULT
}

fn default_custom_save_path() -> String {
    dirs::picture_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .to_string_lossy()
        .to_string()
}

fn default_video_custom_save_path() -> String {
    dirs::video_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .to_string_lossy()
        .to_string()
}

impl Config {
    /// Configuration ID for cosmic-config
    pub const ID: &'static str = "io.github.hojjatabdollahi.snappea";

    /// Load configuration from disk, or return defaults if unavailable
    pub fn load() -> Self {
        match cosmic_config::Config::new(Self::ID, Self::VERSION) {
            Ok(config) => match Self::get_entry(&config) {
                Ok(entry) => entry,
                Err((errs, entry)) => {
                    log::warn!("Error loading config, using defaults: {errs:?}");
                    entry
                }
            },
            Err(err) => {
                log::warn!("Could not create config handler: {err:?}");
                Self::default()
            }
        }
    }

    /// Persist one setting on a background thread. cosmic-config writes one file
    /// per field, so a whole-config save is ~380ms of blocking I/O against ~14ms
    /// for a key, and it reads first, racing other writes in flight.
    pub fn store<T>(key: &'static str, value: T)
    where
        T: serde::Serialize + Send + 'static,
    {
        std::thread::spawn(move || {
            use cosmic_config::ConfigSet;
            match cosmic_config::Config::new(Self::ID, Self::VERSION) {
                Ok(config) => {
                    if let Err(err) = config.set(key, value) {
                        log::error!("Failed to save config key {key}: {err:?}");
                    }
                }
                Err(err) => log::error!("Could not open config to save {key}: {err:?}"),
            }
        });
    }

    /// Persist several settings as one transaction on a background thread.
    pub fn store_many<F>(f: F)
    where
        F: FnOnce(&cosmic_config::ConfigTransaction<'_>) -> Result<(), cosmic_config::Error>
            + Send
            + 'static,
    {
        std::thread::spawn(
            move || match cosmic_config::Config::new(Self::ID, Self::VERSION) {
                Ok(config) => {
                    let tx = config.transaction();
                    match f(&tx).and_then(|()| tx.commit()) {
                        Ok(()) => {}
                        Err(err) => log::error!("Failed to save config keys: {err:?}"),
                    }
                }
                Err(err) => log::error!("Could not open config to save: {err:?}"),
            },
        );
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            magnifier_enabled: true,
            save_location: SaveLocationChoice::Pictures,
            custom_save_path: default_custom_save_path(),
            video_save_location: VideoSaveLocationChoice::Videos,
            video_custom_save_path: default_video_custom_save_path(),
            primary_shape_tool: ShapeTool::Arrow,
            shape_choice: default_shape_choice(),
            freehand_choice: default_freehand_choice(),
            shape_color: AnnotateColor::RED,
            highlighter_thickness: default_highlighter_thickness(),
            shape_thickness: SHAPE_THICKNESS_DEFAULT,
            text_font_size: TEXT_SIZE_DEFAULT,
            primary_redact_tool: RedactTool::Redact,
            pixelation_block_size: 16,
            recognize_qr_codes: default_recognize_qr_codes(),
            magnifier_magnification: default_magnifier_magnification(),
            capture_delay_secs: 0,
            video_encoder: None,
            video_container: Container::Mp4,
            video_framerate: 60,
            video_show_cursor: true,
            video_region_border: default_video_region_border(),
            video_system_audio: false,
            // Off by default: most screenshots are of the screen, not of the
            // pointer, and an arrow baked into the image cannot be taken out.
            show_cursor: false,
            last_mode: CaptureMode::Screenshot,
            last_target: LastTarget::None,
            pencil_color: default_pencil_color(),
            pencil_fade_duration: default_pencil_fade_duration(),
            pencil_thickness: default_pencil_thickness(),
        }
    }
}
