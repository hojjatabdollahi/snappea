//! Message types for screenshot session
//!
//! This module contains:
//! - Msg enum with nested sub-enums for organized message handling
//! - Event enum for portal events

use wayland_client::protocol::wl_output::WlOutput;

use crate::capture::ocr::OcrStatus;
use crate::capture::qr::DetectedQrCode;
use crate::config::{Container, RedactTool, ShapeColor, ShapeTool, ToolbarPosition};
use crate::domain::Choice;
use cosmic::iced::time::Instant;
use cosmic::iced::window;
use cosmic::iced_core::Rectangle;
use cosmic::widget::segmented_button;

// ============================================================================
// Drawing Action Types
// ============================================================================

/// Common draw action for annotation tools (arrow, circle, rectangle, redact, pixelate)
#[derive(Debug, Clone)]
pub enum DrawAction {
    /// Toggle drawing mode on/off
    ModeToggle,
    /// Start drawing at position
    Start(f32, f32),
    /// End drawing at position
    End(f32, f32),
}

/// All drawing/annotation messages
#[derive(Debug, Clone)]
pub enum DrawMsg {
    /// Arrow annotation actions
    Arrow(DrawAction),
    /// Circle/ellipse annotation actions
    Circle(DrawAction),
    /// Rectangle outline annotation actions
    Rectangle(DrawAction),
    /// Redaction (black box) actions
    Redact(DrawAction),
    /// Pixelation actions
    Pixelate(DrawAction),
    /// Clear all shape annotations (keeps redactions)
    ClearShapes,
    /// Clear all redactions (keeps shapes)
    ClearRedactions,
    /// Undo last annotation
    Undo,
    /// Redo undone annotation
    Redo,
}

// ============================================================================
// Tool Popup/Settings Types
// ============================================================================

/// Tool popup actions
#[derive(Debug, Clone)]
pub enum ToolPopupAction {
    /// Toggle popup visibility
    Toggle,
    /// Open popup
    Open,
    /// Close popup
    Close,
}

/// Tool configuration messages (popups and settings for shape/redact tools)
#[derive(Debug, Clone)]
pub enum ToolMsg {
    /// Shape tool actions
    ShapeModeToggle,
    /// Set the primary shape tool
    SetShapeTool(ShapeTool),
    /// Cycle to next shape tool
    CycleShapeTool,
    /// Shape popup actions
    ShapePopup(ToolPopupAction),
    /// Set shape annotation color
    SetShapeColor(ShapeColor),
    /// Toggle shadow on shapes
    ToggleShapeShadow,

    /// Set the primary redact tool
    SetRedactTool(RedactTool),
    /// Redact tool mode toggle
    RedactModeToggle,
    /// Cycle to next redact tool
    CycleRedactTool,
    /// Redact popup actions
    RedactPopup(ToolPopupAction),
    /// Set pixelation block size (UI only, no save)
    SetPixelationBlockSize(u32),
    /// Save current pixelation block size to config
    SavePixelationBlockSize,

    /// Pencil popup actions
    PencilPopup(ToolPopupAction),
    /// Set pencil color for recording annotations
    SetPencilColor(ShapeColor),
    /// Set pencil fade duration (during drag, no save)
    SetPencilFadeDuration(f32),
    /// Save pencil fade duration (on release)
    SavePencilFadeDuration,
    /// Set pencil line thickness (during drag, no save)
    SetPencilThickness(f32),
    /// Save pencil line thickness (on release)
    SavePencilThickness,
    /// Clear all pencil drawings
    ClearPencilDrawings,
}

// ============================================================================
// Selection/Navigation Types
// ============================================================================

/// Navigation direction for keyboard navigation
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Selection mode and navigation messages
#[derive(Debug, Clone)]
pub enum SelectMsg {
    /// Switch to rectangle selection mode
    RegionMode,
    /// Select screen at index
    ScreenMode(usize),
    /// Navigate in direction
    Navigate(Direction),
    /// Confirm current selection
    Confirm,
}

// ============================================================================
// Capture/Output Types
// ============================================================================

/// Capture and save workflow messages
#[derive(Debug, Clone)]
pub enum CaptureMsg {
    /// Initiate capture
    Capture,
    /// Cancel screenshot
    Cancel,
    /// Copy to clipboard
    CopyToClipboard,
    /// Save to Pictures folder
    SaveToPictures,
    /// Record selected region
    RecordRegion,
    /// Stop recording
    StopRecording,
    /// Hide toolbar to system tray during recording
    HideToTray,
    /// Toggle annotation mode during recording
    ToggleRecordingAnnotation,
    /// Right-click on pencil button (opens config popup)
    PencilRightClick,
    /// Toggle capture mode (screenshot vs video) - true = video mode
    ToggleCaptureMode(bool),
    /// Set selection choice
    Choice(Choice),
    /// Set location index
    Location(usize),
    /// Output changed event
    OutputChanged(WlOutput),

    /// Open URL in browser
    OpenUrl(String),
}

// ============================================================================
// Detection (OCR/QR) Types
// ============================================================================

/// QR code detection messages
#[derive(Debug, Clone)]
pub enum QrMsg {
    /// Request QR detection
    Requested,
    /// QR codes detected
    Detected(Vec<DetectedQrCode>),
    /// Copy QR content and close
    CopyAndClose,
}

/// OCR detection messages
#[derive(Debug, Clone)]
pub enum OcrMsg {
    /// Request OCR
    Requested,
    /// OCR status update
    Status(OcrStatus),
    /// Clear OCR status
    StatusClear,
    /// Copy OCR text and close
    CopyAndClose,
}

/// Detection feature messages (OCR and QR)
#[derive(Debug, Clone)]
pub enum DetectMsg {
    /// QR code detection messages
    Qr(QrMsg),
    /// OCR text detection messages
    Ocr(OcrMsg),
}

// ============================================================================
// Settings/UI Types
// ============================================================================

// Re-export SaveLocationChoice and VideoSaveLocationChoice from config
pub use crate::config::{SaveLocationChoice, VideoSaveLocationChoice};

/// Settings and UI messages
#[derive(Debug, Clone)]
pub enum SettingsMsg {
    /// Change toolbar position
    ToolbarPosition(ToolbarPosition),
    /// Toggle settings drawer visibility
    ToggleDrawer,
    /// Toggle magnifier on/off
    ToggleMagnifier,
    /// Set save location
    SetSaveLocation(SaveLocationChoice),
    /// Set custom save path for screenshots
    SetCustomSavePath(String),
    /// Browse for custom save location for screenshots (hides overlay, opens dialog)
    BrowseSaveLocation,
    /// Browse result: restore overlay and optionally set path
    BrowseSaveLocationResult(Option<String>),
    /// Set video save location
    SetVideoSaveLocation(VideoSaveLocationChoice),
    /// Set custom save path for videos
    SetVideoCustomSavePath(String),
    /// Browse for custom save location for videos (hides overlay, opens dialog)
    BrowseVideoSaveLocation,
    /// Browse video result: restore overlay and optionally set path
    BrowseVideoSaveLocationResult(Option<String>),
    /// Toggle copy to clipboard on save
    ToggleCopyOnSave,
    /// Settings tab activated (by segmented button entity)
    SettingsTabActivated(segmented_button::Entity),
    /// Set toolbar opacity when not hovered
    SetToolbarOpacity(f32),
    /// Save toolbar opacity to config (debounced) with ID to prevent stale saves
    SaveToolbarOpacityDebounced(f32, u64),
    /// Toolbar hover state changed (true = hovered, false = unhovered)
    ToolbarHoverChanged(bool),
    /// Update the toolbar bounds for recording input filtering
    ToolbarBounds(Rectangle),
    /// Set video encoder (gst_element name)
    SetVideoEncoder(String),
    /// Set video container format
    SetVideoContainer(Container),
    /// Set video framerate
    SetVideoFramerate(u32),
    /// Toggle showing cursor in recordings
    ToggleShowCursor,
    /// Toggle hide toolbar to system tray when recording
    ToggleHideToTray,
    /// Encoders have been detected asynchronously
    EncodersDetected(Vec<crate::screencast::encoder::EncoderInfo>),
    /// Animation timeline tick (window_id, instant)
    TimelineTick(window::Id, Instant),
    /// Set move offset for dragging selection rectangle
    SetMoveOffset(Option<(i32, i32)>),
}

// ============================================================================
// Main Message Enum
// ============================================================================

/// Messages for screenshot session interactions
#[derive(Debug, Clone)]
pub enum Msg {
    /// Drawing/annotation messages
    Draw(DrawMsg),
    /// Tool configuration messages
    Tool(ToolMsg),
    /// Selection mode and navigation messages
    Select(SelectMsg),
    /// Capture workflow messages
    Capture(CaptureMsg),
    /// Detection (OCR/QR) messages
    Detect(DetectMsg),
    /// Settings and UI messages
    Settings(SettingsMsg),
}

// ============================================================================
// Convenience Constructors
// ============================================================================

impl Msg {
    // Drawing shortcuts
    pub fn arrow_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Arrow(DrawAction::ModeToggle))
    }
    pub fn arrow_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Arrow(DrawAction::Start(x, y)))
    }
    pub fn arrow_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Arrow(DrawAction::End(x, y)))
    }
    pub fn circle_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Circle(DrawAction::ModeToggle))
    }
    pub fn circle_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Circle(DrawAction::Start(x, y)))
    }
    pub fn circle_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Circle(DrawAction::End(x, y)))
    }
    pub fn rectangle_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Rectangle(DrawAction::ModeToggle))
    }
    pub fn rectangle_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Rectangle(DrawAction::Start(x, y)))
    }
    pub fn rectangle_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Rectangle(DrawAction::End(x, y)))
    }

    pub fn redact_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Redact(DrawAction::ModeToggle))
    }
    pub fn redact_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Redact(DrawAction::Start(x, y)))
    }
    pub fn redact_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Redact(DrawAction::End(x, y)))
    }

    pub fn pixelate_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Pixelate(DrawAction::ModeToggle))
    }
    pub fn pixelate_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pixelate(DrawAction::Start(x, y)))
    }
    pub fn pixelate_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pixelate(DrawAction::End(x, y)))
    }
    pub fn undo() -> Self {
        Self::Draw(DrawMsg::Undo)
    }
    pub fn redo() -> Self {
        Self::Draw(DrawMsg::Redo)
    }
    pub fn clear_shapes() -> Self {
        Self::Draw(DrawMsg::ClearShapes)
    }
    pub fn clear_redactions() -> Self {
        Self::Draw(DrawMsg::ClearRedactions)
    }

    // Tool shortcuts
    pub fn shape_mode_toggle() -> Self {
        Self::Tool(ToolMsg::ShapeModeToggle)
    }
    pub fn set_shape_tool(tool: ShapeTool) -> Self {
        Self::Tool(ToolMsg::SetShapeTool(tool))
    }
    pub fn cycle_shape_tool() -> Self {
        Self::Tool(ToolMsg::CycleShapeTool)
    }
    pub fn toggle_shape_popup() -> Self {
        Self::Tool(ToolMsg::ShapePopup(ToolPopupAction::Toggle))
    }
    pub fn open_shape_popup() -> Self {
        Self::Tool(ToolMsg::ShapePopup(ToolPopupAction::Open))
    }
    pub fn close_shape_popup() -> Self {
        Self::Tool(ToolMsg::ShapePopup(ToolPopupAction::Close))
    }
    pub fn set_shape_color(color: ShapeColor) -> Self {
        Self::Tool(ToolMsg::SetShapeColor(color))
    }
    pub fn toggle_shape_shadow() -> Self {
        Self::Tool(ToolMsg::ToggleShapeShadow)
    }

    pub fn set_redact_tool(tool: RedactTool) -> Self {
        Self::Tool(ToolMsg::SetRedactTool(tool))
    }
    pub fn redact_tool_mode_toggle() -> Self {
        Self::Tool(ToolMsg::RedactModeToggle)
    }
    pub fn cycle_redact_tool() -> Self {
        Self::Tool(ToolMsg::CycleRedactTool)
    }
    pub fn toggle_redact_popup() -> Self {
        Self::Tool(ToolMsg::RedactPopup(ToolPopupAction::Toggle))
    }
    pub fn open_redact_popup() -> Self {
        Self::Tool(ToolMsg::RedactPopup(ToolPopupAction::Open))
    }
    pub fn close_redact_popup() -> Self {
        Self::Tool(ToolMsg::RedactPopup(ToolPopupAction::Close))
    }
    pub fn set_pixelation_block_size(size: u32) -> Self {
        Self::Tool(ToolMsg::SetPixelationBlockSize(size))
    }
    pub fn save_pixelation_block_size() -> Self {
        Self::Tool(ToolMsg::SavePixelationBlockSize)
    }

    // Pencil tool shortcuts (for recording annotations)
    pub fn toggle_pencil_popup() -> Self {
        Self::Tool(ToolMsg::PencilPopup(ToolPopupAction::Toggle))
    }
    pub fn close_pencil_popup() -> Self {
        Self::Tool(ToolMsg::PencilPopup(ToolPopupAction::Close))
    }
    pub fn set_pencil_color(color: ShapeColor) -> Self {
        Self::Tool(ToolMsg::SetPencilColor(color))
    }
    pub fn set_pencil_fade_duration(duration: f32) -> Self {
        Self::Tool(ToolMsg::SetPencilFadeDuration(duration))
    }
    pub fn save_pencil_fade_duration() -> Self {
        Self::Tool(ToolMsg::SavePencilFadeDuration)
    }
    pub fn set_pencil_thickness(thickness: f32) -> Self {
        Self::Tool(ToolMsg::SetPencilThickness(thickness))
    }
    pub fn save_pencil_thickness() -> Self {
        Self::Tool(ToolMsg::SavePencilThickness)
    }
    pub fn clear_pencil_drawings() -> Self {
        Self::Tool(ToolMsg::ClearPencilDrawings)
    }

    // Selection shortcuts
    pub fn region_mode() -> Self {
        Self::Select(SelectMsg::RegionMode)
    }
    pub fn screen_mode(output_index: usize) -> Self {
        Self::Select(SelectMsg::ScreenMode(output_index))
    }
    pub fn navigate_left() -> Self {
        Self::Select(SelectMsg::Navigate(Direction::Left))
    }
    pub fn navigate_right() -> Self {
        Self::Select(SelectMsg::Navigate(Direction::Right))
    }
    pub fn navigate_up() -> Self {
        Self::Select(SelectMsg::Navigate(Direction::Up))
    }
    pub fn navigate_down() -> Self {
        Self::Select(SelectMsg::Navigate(Direction::Down))
    }
    pub fn confirm() -> Self {
        Self::Select(SelectMsg::Confirm)
    }

    // Capture shortcuts
    pub fn cancel() -> Self {
        Self::Capture(CaptureMsg::Cancel)
    }
    pub fn copy_to_clipboard() -> Self {
        Self::Capture(CaptureMsg::CopyToClipboard)
    }
    pub fn save_to_pictures() -> Self {
        Self::Capture(CaptureMsg::SaveToPictures)
    }
    pub fn record_region() -> Self {
        Self::Capture(CaptureMsg::RecordRegion)
    }
    pub fn stop_recording() -> Self {
        Self::Capture(CaptureMsg::StopRecording)
    }
    pub fn toggle_recording_annotation() -> Self {
        Self::Capture(CaptureMsg::ToggleRecordingAnnotation)
    }
    pub fn choice(c: Choice) -> Self {
        Self::Capture(CaptureMsg::Choice(c))
    }
    pub fn output_changed(output: WlOutput) -> Self {
        Self::Capture(CaptureMsg::OutputChanged(output))
    }
    pub fn open_url(url: String) -> Self {
        Self::Capture(CaptureMsg::OpenUrl(url))
    }

    // Detection shortcuts
    pub fn qr_requested() -> Self {
        Self::Detect(DetectMsg::Qr(QrMsg::Requested))
    }
    pub fn qr_detected(codes: Vec<DetectedQrCode>) -> Self {
        Self::Detect(DetectMsg::Qr(QrMsg::Detected(codes)))
    }
    pub fn qr_copy_and_close() -> Self {
        Self::Detect(DetectMsg::Qr(QrMsg::CopyAndClose))
    }
    pub fn ocr_requested() -> Self {
        Self::Detect(DetectMsg::Ocr(OcrMsg::Requested))
    }
    pub fn ocr_status(status: OcrStatus) -> Self {
        Self::Detect(DetectMsg::Ocr(OcrMsg::Status(status)))
    }
    pub fn ocr_copy_and_close() -> Self {
        Self::Detect(DetectMsg::Ocr(OcrMsg::CopyAndClose))
    }

    // Settings shortcuts
    pub fn toolbar_position(pos: ToolbarPosition) -> Self {
        Self::Settings(SettingsMsg::ToolbarPosition(pos))
    }
    pub fn toggle_settings_drawer() -> Self {
        Self::Settings(SettingsMsg::ToggleDrawer)
    }
    pub fn toggle_magnifier() -> Self {
        Self::Settings(SettingsMsg::ToggleMagnifier)
    }
    pub fn set_save_location_pictures() -> Self {
        Self::Settings(SettingsMsg::SetSaveLocation(SaveLocationChoice::Pictures))
    }
    pub fn set_save_location_documents() -> Self {
        Self::Settings(SettingsMsg::SetSaveLocation(SaveLocationChoice::Documents))
    }
    pub fn set_save_location_custom() -> Self {
        Self::Settings(SettingsMsg::SetSaveLocation(SaveLocationChoice::Custom))
    }
    pub fn set_custom_save_path(path: String) -> Self {
        Self::Settings(SettingsMsg::SetCustomSavePath(path))
    }
    pub fn browse_save_location() -> Self {
        Self::Settings(SettingsMsg::BrowseSaveLocation)
    }
    pub fn browse_save_location_result(path: Option<String>) -> Self {
        Self::Settings(SettingsMsg::BrowseSaveLocationResult(path))
    }
    pub fn set_video_save_location_videos() -> Self {
        Self::Settings(SettingsMsg::SetVideoSaveLocation(
            VideoSaveLocationChoice::Videos,
        ))
    }
    pub fn set_video_save_location_custom() -> Self {
        Self::Settings(SettingsMsg::SetVideoSaveLocation(
            VideoSaveLocationChoice::Custom,
        ))
    }
    pub fn set_video_custom_save_path(path: String) -> Self {
        Self::Settings(SettingsMsg::SetVideoCustomSavePath(path))
    }
    pub fn browse_video_save_location() -> Self {
        Self::Settings(SettingsMsg::BrowseVideoSaveLocation)
    }
    pub fn browse_video_save_location_result(path: Option<String>) -> Self {
        Self::Settings(SettingsMsg::BrowseVideoSaveLocationResult(path))
    }
    pub fn toggle_copy_on_save() -> Self {
        Self::Settings(SettingsMsg::ToggleCopyOnSave)
    }
    pub fn settings_tab_activated(entity: segmented_button::Entity) -> Self {
        Self::Settings(SettingsMsg::SettingsTabActivated(entity))
    }
    pub fn set_toolbar_opacity(opacity: f32) -> Self {
        Self::Settings(SettingsMsg::SetToolbarOpacity(opacity))
    }
    pub fn toolbar_hover_changed(is_hovered: bool) -> Self {
        Self::Settings(SettingsMsg::ToolbarHoverChanged(is_hovered))
    }
    pub fn toolbar_bounds(bounds: Rectangle) -> Self {
        Self::Settings(SettingsMsg::ToolbarBounds(bounds))
    }
    pub fn set_video_encoder(encoder: String) -> Self {
        Self::Settings(SettingsMsg::SetVideoEncoder(encoder))
    }
    pub fn set_video_container(container: Container) -> Self {
        Self::Settings(SettingsMsg::SetVideoContainer(container))
    }
    pub fn set_video_framerate(framerate: u32) -> Self {
        Self::Settings(SettingsMsg::SetVideoFramerate(framerate))
    }
    pub fn toggle_show_cursor() -> Self {
        Self::Settings(SettingsMsg::ToggleShowCursor)
    }
    pub fn toggle_hide_to_tray() -> Self {
        Self::Settings(SettingsMsg::ToggleHideToTray)
    }
    pub fn timeline_tick(window_id: window::Id, instant: Instant) -> Self {
        Self::Settings(SettingsMsg::TimelineTick(window_id, instant))
    }
    pub fn set_move_offset(offset: Option<(i32, i32)>) -> Self {
        Self::Settings(SettingsMsg::SetMoveOffset(offset))
    }
}
