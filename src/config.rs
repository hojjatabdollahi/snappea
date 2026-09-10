//! Configuration persistence for snappea settings

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use cosmic::iced::Color;
use cosmic::iced::keyboard::{Key, Modifiers, key::Named};
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

/// Save location choice for UI selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SaveLocationChoice {
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
    Circle,
    Rectangle,
    Pencil,
    Text,
}

impl ShapeTool {
    /// Every shape tool, in the order shown in the picker and cycled by `next()`.
    ///
    /// Driving the UI from this slice (rather than hand-written match arms per
    /// button) means adding a tool only requires extending this list plus the
    /// per-tool metadata below.
    pub const ALL: &'static [ShapeTool] = &[
        ShapeTool::Arrow,
        ShapeTool::Line,
        ShapeTool::Circle,
        ShapeTool::Rectangle,
        ShapeTool::Pencil,
        ShapeTool::Text,
    ];

    /// Number of shape tools (used for the split-button option indicator)
    pub const COUNT: usize = Self::ALL.len();

    /// Get the next shape tool in the cycle
    pub fn next(self) -> Self {
        let i = self.index();
        Self::ALL[(i + 1) % Self::COUNT]
    }

    /// Position of this tool in the cycle (for the split-button indicator)
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|t| *t == self)
            .unwrap_or_default()
    }

    /// Freedesktop symbolic icon name for this tool.
    ///
    /// Matches the icon set cosmic-viewer uses for its annotate tools so the
    /// two apps look consistent.
    pub fn icon_name(self) -> &'static str {
        match self {
            ShapeTool::Arrow => "insert-arrow-symbolic",
            ShapeTool::Line => "insert-line-symbolic",
            ShapeTool::Circle => "insert-ellipse-symbolic",
            ShapeTool::Rectangle => "insert-rectangle-symbolic",
            ShapeTool::Pencil => "insert-drawing-symbolic",
            ShapeTool::Text => "insert-text-symbolic",
        }
    }

    /// Short label shown next to the icon in the tool picker
    pub fn label(self) -> String {
        match self {
            ShapeTool::Arrow => fl!("arrow"),
            ShapeTool::Line => fl!("line"),
            ShapeTool::Circle => fl!("oval-circle"),
            ShapeTool::Rectangle => fl!("rectangle-square"),
            ShapeTool::Pencil => fl!("pencil"),
            ShapeTool::Text => fl!("text"),
        }
    }

    /// Get the tooltip text for this shape tool
    pub fn tooltip(self) -> String {
        match self {
            ShapeTool::Arrow => fl!("draw-arrow"),
            ShapeTool::Line => fl!("draw-line"),
            ShapeTool::Circle => fl!("draw-circle"),
            ShapeTool::Rectangle => fl!("draw-rectangle"),
            ShapeTool::Pencil => fl!("draw-pencil"),
            ShapeTool::Text => fl!("draw-text"),
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

/// A named (non-character) key that may be bound to an action.
///
/// Deliberately a small closed set rather than a mirror of iced's `Named`: only
/// keys that make sense as a shortcut are listed, and everything else converts
/// to `None`, which is what makes unbindable keys (bare modifiers, media keys)
/// impossible to write into the config in the first place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindNamed {
    Enter,
    Space,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

impl BindNamed {
    fn from_named(named: Named) -> Option<Self> {
        Some(match named {
            Named::Enter => Self::Enter,
            // No `Named::Space` arm: iced has no such variant — space always
            // arrives as `Key::Character(" ")`, folded in by `BindKey::from_key`.
            Named::Tab => Self::Tab,
            Named::Backspace => Self::Backspace,
            Named::Delete => Self::Delete,
            Named::Insert => Self::Insert,
            Named::Home => Self::Home,
            Named::End => Self::End,
            Named::PageUp => Self::PageUp,
            Named::PageDown => Self::PageDown,
            Named::F1 => Self::F1,
            Named::F2 => Self::F2,
            Named::F3 => Self::F3,
            Named::F4 => Self::F4,
            Named::F5 => Self::F5,
            Named::F6 => Self::F6,
            Named::F7 => Self::F7,
            Named::F8 => Self::F8,
            Named::F9 => Self::F9,
            Named::F10 => Self::F10,
            Named::F11 => Self::F11,
            Named::F12 => Self::F12,
            _ => return None,
        })
    }

    /// Human-readable name for tooltips and the settings button
    fn label(self) -> &'static str {
        match self {
            Self::Enter => "Enter",
            Self::Space => "Space",
            Self::Tab => "Tab",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Insert => "Insert",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "PageUp",
            Self::PageDown => "PageDown",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
        }
    }
}

/// The key half of a binding, without modifiers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BindKey {
    Named(BindNamed),
    /// A printable character, always stored lowercase — case is carried by the
    /// binding's `shift` flag instead, so "c" and "C" can't both be stored.
    Character(String),
}

impl BindKey {
    /// Normalise a live key press into a bindable key, or `None` if this key
    /// can't be a shortcut (bare modifiers, media keys, dead keys).
    fn from_key(key: &Key) -> Option<Self> {
        match key {
            Key::Named(n) => BindNamed::from_named(*n).map(Self::Named),
            // Space arrives as a character, not a named key (see the shortcuts
            // tests), but reads better as "Space" — fold it into the named set
            // so both spellings compare equal.
            Key::Character(c) if c.as_str() == " " => Some(Self::Named(BindNamed::Space)),
            Key::Character(c) if !c.trim().is_empty() => Some(Self::Character(c.to_lowercase())),
            _ => None,
        }
    }
}

/// A user-rebindable keyboard shortcut.
///
/// Stored as our own owned type rather than iced's `Key`/`Modifiers`, which
/// aren't serialisable (and `Key::Character` holds a borrowed-ish `SmolStr`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: BindKey,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

impl KeyBinding {
    /// A plain named key with no modifiers
    fn plain(key: BindNamed) -> Self {
        Self {
            key: BindKey::Named(key),
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    /// Does this live key press trigger the binding?
    pub fn matches(&self, key: &Key, modifiers: Modifiers) -> bool {
        self.ctrl == modifiers.control()
            && self.alt == modifiers.alt()
            && self.shift == modifiers.shift()
            && BindKey::from_key(key).is_some_and(|pressed| pressed == self.key)
    }

    /// Build a binding from a live key press, or `None` if this key may not be
    /// bound.
    ///
    /// Escape and Ctrl+Enter are refused because the shortcut table matches
    /// them *above* the configurable arm — binding copy to either would hand
    /// the user a shortcut that silently never fires. Bare modifiers and other
    /// unbindable keys fall out via [`BindKey::from_key`] returning `None`.
    pub fn from_event(key: &Key, modifiers: Modifiers) -> Option<Self> {
        if matches!(key, Key::Named(Named::Escape)) {
            return None;
        }
        let bind = BindKey::from_key(key)?;
        if bind == BindKey::Named(BindNamed::Enter) && modifiers.control() {
            return None;
        }
        Some(Self {
            key: bind,
            ctrl: modifiers.control(),
            shift: modifiers.shift(),
            alt: modifiers.alt(),
        })
    }

    /// "Enter", "Ctrl+Shift+C" — for the toolbar tooltip and settings button
    pub fn display_name(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        match &self.key {
            BindKey::Named(n) => out.push_str(n.label()),
            BindKey::Character(c) => out.push_str(&c.to_uppercase()),
        }
        out
    }
}

/// Application configuration persisted between sessions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct SnapPeaConfig {
    /// Whether to show the magnifier when dragging selection corners
    pub magnifier_enabled: bool,
    /// Where to save screenshots (Pictures, Documents, or Custom)
    pub save_location: SaveLocationChoice,
    /// Custom path for saving screenshots (used when save_location is Custom)
    #[serde(default = "default_custom_save_path")]
    pub custom_save_path: String,
    /// Where to save videos (Videos or Custom)
    #[serde(default)]
    pub video_save_location: VideoSaveLocationChoice,
    /// Custom path for saving videos (used when video_save_location is Custom)
    #[serde(default = "default_video_custom_save_path")]
    pub video_custom_save_path: String,
    /// Whether to also copy to clipboard when saving to file
    pub copy_to_clipboard_on_save: bool,
    /// Primary shape tool shown in the button
    pub primary_shape_tool: ShapeTool,
    /// Color for shape annotations
    pub shape_color: ShapeColor,
    /// Whether to add shadow/border to shapes
    pub shape_shadow: bool,
    /// Stroke thickness (logical units) used for new shape annotations
    #[serde(default = "default_shape_thickness")]
    pub shape_thickness: f32,
    /// Font size (logical units) used for new text annotations
    #[serde(default = "default_text_font_size")]
    pub text_font_size: f32,
    /// Primary redact tool shown in the button
    pub primary_redact_tool: RedactTool,
    /// Pixelation block size (larger = more pixelated, range 4-64)
    pub pixelation_block_size: u32,
    /// Magnifier zoom level (range 1.5-10.0)
    #[serde(default = "default_magnifier_magnification")]
    pub magnifier_magnification: f32,
    /// Delay in seconds for the "delayed screenshot" toolbar button
    #[serde(default = "default_capture_delay_secs")]
    pub capture_delay_secs: u32,
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
    /// Key that copies the current selection to the clipboard
    #[serde(default = "default_copy_shortcut")]
    pub copy_shortcut: KeyBinding,
}

fn default_copy_shortcut() -> KeyBinding {
    KeyBinding::plain(BindNamed::Enter)
}

fn default_magnifier_magnification() -> f32 {
    2.5
}

fn default_capture_delay_secs() -> u32 {
    3
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

fn default_shape_thickness() -> f32 {
    crate::domain::SHAPE_THICKNESS_DEFAULT
}

fn default_text_font_size() -> f32 {
    crate::domain::TEXT_SIZE_DEFAULT
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
            save_location: SaveLocationChoice::Pictures,
            custom_save_path: default_custom_save_path(),
            // Video save location defaults
            video_save_location: VideoSaveLocationChoice::Videos,
            video_custom_save_path: default_video_custom_save_path(),
            // Don't copy to clipboard by default when saving
            copy_to_clipboard_on_save: false,
            // Default to Arrow as primary shape tool
            primary_shape_tool: ShapeTool::Arrow,
            // Default red color for shapes
            shape_color: ShapeColor::default(),
            // Shadow enabled by default (matches current arrow behavior)
            shape_shadow: true,
            shape_thickness: crate::domain::SHAPE_THICKNESS_DEFAULT,
            text_font_size: crate::domain::TEXT_SIZE_DEFAULT,
            // Default to Redact as primary redact tool
            primary_redact_tool: RedactTool::Redact,
            // Default pixelation block size (16 is a good balance)
            pixelation_block_size: 16,
            // Default magnifier zoom level
            magnifier_magnification: default_magnifier_magnification(),
            // Default delayed-screenshot delay
            capture_delay_secs: default_capture_delay_secs(),
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
            // Enter copies the selection, as it always has
            copy_shortcut: default_copy_shortcut(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_tool_cycle_visits_every_tool_once() {
        let mut seen = Vec::new();
        let mut tool = ShapeTool::default();
        for _ in 0..ShapeTool::COUNT {
            seen.push(tool);
            tool = tool.next();
        }
        // A full cycle returns to the start and covers the whole list.
        assert_eq!(tool, ShapeTool::default());
        assert_eq!(seen.len(), ShapeTool::ALL.len());
        for t in ShapeTool::ALL {
            assert!(seen.contains(t), "{t:?} missing from the cycle");
        }
    }

    #[test]
    fn shape_tool_index_matches_position_in_all() {
        for (i, tool) in ShapeTool::ALL.iter().enumerate() {
            assert_eq!(tool.index(), i);
        }
    }

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn the_default_copy_shortcut_is_still_enter() {
        let b = default_copy_shortcut();
        assert!(b.matches(&Key::Named(Named::Enter), Modifiers::default()));
        assert_eq!(b.display_name(), "Enter");
    }

    #[test]
    fn a_rebound_key_matches_and_the_old_one_stops_matching() {
        let b = KeyBinding::from_event(&ch("c"), Modifiers::default()).unwrap();
        assert!(b.matches(&ch("c"), Modifiers::default()));
        assert!(!b.matches(&Key::Named(Named::Enter), Modifiers::default()));
    }

    #[test]
    fn modifiers_must_match_exactly() {
        // A plain binding must not fire when a modifier is held, or Ctrl+Enter
        // (save) would also trigger copy.
        let enter = default_copy_shortcut();
        assert!(!enter.matches(&Key::Named(Named::Enter), Modifiers::CTRL));

        let ctrl_c = KeyBinding::from_event(&ch("c"), Modifiers::CTRL).unwrap();
        assert!(ctrl_c.matches(&ch("c"), Modifiers::CTRL));
        assert!(!ctrl_c.matches(&ch("c"), Modifiers::default()));
        assert_eq!(ctrl_c.display_name(), "Ctrl+C");
    }

    #[test]
    fn space_binds_the_same_whether_it_arrives_named_or_as_a_character() {
        // Space reaches the shortcut table as Character(" "), but reads as
        // "Space" in the UI — both spellings must be one binding.
        let b = KeyBinding::from_event(&ch(" "), Modifiers::default()).unwrap();
        assert_eq!(b.key, BindKey::Named(BindNamed::Space));
        assert!(b.matches(&ch(" "), Modifiers::default()));
        assert_eq!(b.display_name(), "Space");
    }

    #[test]
    fn unbindable_keys_are_refused() {
        // Bare modifiers, Escape (cancel) and Ctrl+Enter (save) must not be
        // storable — the shortcut table claims them ahead of the copy arm.
        for (key, mods) in [
            (Key::Named(Named::Escape), Modifiers::default()),
            (Key::Named(Named::Control), Modifiers::CTRL),
            (Key::Named(Named::Shift), Modifiers::SHIFT),
            (Key::Named(Named::Enter), Modifiers::CTRL),
        ] {
            assert!(
                KeyBinding::from_event(&key, mods).is_none(),
                "{key:?} should not be bindable"
            );
        }
    }

    #[test]
    fn a_binding_survives_a_serde_round_trip() {
        let b = KeyBinding::from_event(&ch("c"), Modifiers::CTRL | Modifiers::SHIFT).unwrap();
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(serde_json::from_str::<KeyBinding>(&json).unwrap(), b);
    }

    #[test]
    fn every_shape_tool_has_a_distinct_icon() {
        let mut names: Vec<&str> = ShapeTool::ALL.iter().map(|t| t.icon_name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two tools share an icon");
    }
}
