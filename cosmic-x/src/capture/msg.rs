// SPDX-License-Identifier: GPL-3.0-only

//! Messages for a capture in progress.

use wayland_client::protocol::wl_output::WlOutput;

use crate::capture::detect::DetectedQrCode;
use crate::capture::detect::OcrStatus;
use crate::config::{Container, RedactTool, ShapeTool};
use crate::geometry::Choice;
use cosmic::iced::core::{Point, Rectangle};
use cosmic::iced::time::Instant;
use cosmic::iced::window;
use cosmic::widget::segmented_button;
use viewer_tools::annotate::AnnotateColor;

/// Common draw action for annotation tools (arrow, circle, rectangle, redact, pixelate)
#[derive(Debug, Clone)]
pub enum DrawAction {
    /// Toggle drawing mode on/off
    ModeToggle,
    /// Start drawing at position
    Start(f32, f32),
    /// Drag moved to position while drawing (used by freehand/pencil to append a point)
    Move(f32, f32),
    /// End drawing at position
    End(f32, f32),
}

/// What can happen to a text label. More than `DrawAction`, since an edit is
/// placed, typed, styled and finished.
#[derive(Debug, Clone)]
pub enum TextAction {
    /// Pointer went down at this global point
    Press(f32, f32),
    /// Pointer moved to this global point while down
    Drag(f32, f32),
    /// Pointer came up at this global point
    Release(f32, f32),
    /// A key, with the text it produced if any
    Key(
        cosmic::iced::keyboard::Key,
        cosmic::iced::keyboard::Modifiers,
        Option<String>,
    ),
    /// Finish the edit, keeping whatever was typed
    Commit,
    /// Clipboard contents, for Ctrl+V
    Paste(String),
    /// Font family picked, by index into the family list
    Family(usize),
    /// Font size picked, by index into the presets
    Size(usize),
    /// A bold, italic or underline segment was pressed
    StyleActivated(segmented_button::Entity),
    /// An alignment segment was pressed
    AlignActivated(segmented_button::Entity),
    /// The font and style popup opened or closed
    TogglePopup,
}

#[derive(Debug, Clone)]
pub enum DrawMsg {
    /// Shape annotation actions, carrying which shape
    Shape(viewer_tools::annotate::ShapeKind, DrawAction),
    /// Text label actions
    Text(TextAction),
    /// Freehand (pencil) annotation actions
    Pen(DrawAction),
    /// Magnifier annotation actions
    Magnifier(DrawAction),
    /// Select a magnifier for editing (index into magnifiers, or None to deselect)
    MagnifierSelect(Option<usize>),
    /// Move the given magnifier so its center is at (global x, y)
    MagnifierMove(usize, f32, f32),
    /// Resize the given magnifier to the given radius (global logical units)
    MagnifierResize(usize, f32),
    /// Set the magnification (zoom) of the given magnifier
    MagnifierSetZoom(usize, f32),
    /// Redaction (black box) actions
    Redact(DrawAction),
    /// Pixelation actions
    Pixelate(DrawAction),
    /// Clear all shape annotations (keeps redactions)
    ClearShapes,
    /// Clear all redactions (keeps shapes)
    ClearRedactions,
    /// Pick annotations up instead of drawing new ones
    ToggleMoveMode,
    /// Pick out an annotation, or clear the pick
    SelectAnnotation(Option<usize>),
    /// Drop the picked annotation
    DeleteSelected,
    /// Shift the annotation at this index by (dx, dy) logical units
    MoveAnnotation(usize, f32, f32),
    /// Undo last annotation
    Undo,
    /// Redo undone annotation
    Redo,
}

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
    /// Enter or leave the toolbar's annotation section
    AnnotateModeToggle,
    /// Set the primary shape tool
    SetShapeTool(ShapeTool),
    /// Cycle to next shape tool
    CycleShapeTool,
    /// Shape popup actions
    ShapePopup(ToolPopupAction),
    FreehandPopup(ToolPopupAction),
    /// Capture delay dropdown
    DelayPopup(ToolPopupAction),
    /// Stroke width dropdown
    StrokePopup(ToolPopupAction),
    /// Font size dropdown
    FontPopup(ToolPopupAction),
    /// Set shape annotation color
    SetAnnotateColor(AnnotateColor),
    /// Drive the custom color picker
    ColorPicker(cosmic::widget::color_picker::ColorPickerUpdate),
    /// Set shape stroke thickness (UI only, no save)
    SetShapeThickness(f32),
    /// Save the current shape stroke thickness to config
    SaveShapeThickness,

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

    /// Magnifier tool mode toggle
    MagnifierModeToggle,
    /// Magnifier popup actions
    MagnifierPopup(ToolPopupAction),
    /// Set magnifier magnification (UI only, no save)
    SetMagnification(f32),
    /// Save current magnifier magnification to config
    SaveMagnification,

    /// Pencil popup actions
    PencilPopup(ToolPopupAction),
    /// Set pencil color for recording annotations
    SetPencilColor(AnnotateColor),
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

/// Capture and save workflow messages
#[derive(Debug, Clone)]
pub enum ActionMsg {
    /// Initiate capture
    Capture,
    /// Cancel screenshot
    Cancel,
    /// Escape was pressed. Cancels immediately unless there is unsaved
    /// annotation work, in which case it asks for confirmation first.
    CancelRequested,
    /// Copy to clipboard
    CopyToClipboard,
    /// Save to Pictures folder
    SaveToPictures,
    /// Hide the overlay, wait the configured delay, then re-capture the screen
    DelayedCapture,
    /// Cycle the delayed-screenshot delay (3 -> 5 -> 10 -> 3 seconds)
    CycleCaptureDelay,
    /// Record selected region
    RecordRegion,
    /// Stop recording
    StopRecording,
    /// Toggle annotation mode during recording
    ToggleRecordingAnnotation,
    /// Right-click on pencil button (opens config popup)
    PencilRightClick,
    /// Toggle capture mode. True selects video
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

/// QR code detection messages
#[derive(Debug, Clone)]
pub enum QrMsg {
    /// Put the detected codes away
    Dismiss,
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
    /// A run started under the given generation reports its status.
    Status(u64, OcrStatus),
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

// Re-export SaveLocationChoice and VideoSaveLocationChoice from config
pub use crate::config::{SaveLocationChoice, VideoSaveLocationChoice};

/// Settings and UI messages
#[derive(Debug, Clone)]
pub enum SettingsMsg {
    /// Grip pressed: begin dragging the toolbar
    ToolbarDragStart,
    /// Pointer moved while the grip is held (output-local coordinates)
    /// Pointer moved while the grip is held, on the named output
    ToolbarDragMove(String, Point),
    /// Grip released
    ToolbarDragEnd,
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
    /// Save to the clipboard and nowhere else
    SetSaveLocationClipboard,
    /// Set the delayed-screenshot delay (seconds)
    SetCaptureDelay(u32),
    /// Update the toolbar bounds for recording input filtering
    ToolbarBounds(Rectangle),
    /// Pick an encoder, or `None` to choose the best detected one on each run.
    SetVideoEncoder(Option<String>),
    /// Set video container format
    SetVideoContainer(Container),
    /// Set video framerate
    SetVideoFramerate(u32),
    /// Toggle showing cursor in recordings
    ToggleShowCursor,
    /// Toggle whether a screenshot includes the pointer
    ToggleScreenshotCursor,
    /// Encoders have been detected asynchronously
    EncodersDetected(Vec<crate::recording::encoder::EncoderInfo>),
    /// Animation timeline tick (`window_id`, instant)
    TimelineTick(window::Id, Instant),
    /// Set move offset for dragging selection rectangle
    SetMoveOffset(Option<(i32, i32)>),
    /// Set cosmic-x as the default screenshot portal for the current user
    SetAsDefaultPortal,
    /// Toggle automatic QR scanning
    ToggleRecognizeQrCodes,
}

/// Everything the overlay can ask for.
#[derive(Debug, Clone)]
pub enum Msg {
    /// Drawing/annotation messages
    Draw(DrawMsg),
    /// Tool configuration messages
    Tool(ToolMsg),
    /// Selection mode and navigation messages
    Select(SelectMsg),
    /// Capture workflow messages
    Action(ActionMsg),
    /// Detection (OCR/QR) messages
    Detect(DetectMsg),
    /// Settings and UI messages
    Settings(SettingsMsg),
}

impl Msg {
    // Drawing shortcuts
    pub const fn shape_start(kind: viewer_tools::annotate::ShapeKind, x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Shape(kind, DrawAction::Start(x, y)))
    }
    pub const fn shape_end(kind: viewer_tools::annotate::ShapeKind, x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Shape(kind, DrawAction::End(x, y)))
    }
    pub const fn text(action: TextAction) -> Self {
        Self::Draw(DrawMsg::Text(action))
    }
    pub const fn pen_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Pen(DrawAction::ModeToggle))
    }
    pub const fn pen_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pen(DrawAction::Start(x, y)))
    }
    pub const fn pen_move(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pen(DrawAction::Move(x, y)))
    }
    pub const fn pen_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pen(DrawAction::End(x, y)))
    }
    pub const fn magnifier_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Magnifier(DrawAction::ModeToggle))
    }
    pub const fn magnifier_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Magnifier(DrawAction::Start(x, y)))
    }
    pub const fn magnifier_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Magnifier(DrawAction::End(x, y)))
    }
    pub const fn magnifier_select(index: Option<usize>) -> Self {
        Self::Draw(DrawMsg::MagnifierSelect(index))
    }
    pub const fn magnifier_move(index: usize, x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::MagnifierMove(index, x, y))
    }
    pub const fn magnifier_resize(index: usize, radius: f32) -> Self {
        Self::Draw(DrawMsg::MagnifierResize(index, radius))
    }
    pub const fn magnifier_set_zoom(index: usize, zoom: f32) -> Self {
        Self::Draw(DrawMsg::MagnifierSetZoom(index, zoom))
    }

    pub const fn redact_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Redact(DrawAction::ModeToggle))
    }
    pub const fn redact_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Redact(DrawAction::Start(x, y)))
    }
    pub const fn redact_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Redact(DrawAction::End(x, y)))
    }

    pub const fn pixelate_mode_toggle() -> Self {
        Self::Draw(DrawMsg::Pixelate(DrawAction::ModeToggle))
    }
    pub const fn pixelate_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pixelate(DrawAction::Start(x, y)))
    }
    pub const fn pixelate_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pixelate(DrawAction::End(x, y)))
    }
    pub const fn toggle_move_mode() -> Self {
        Self::Draw(DrawMsg::ToggleMoveMode)
    }
    pub const fn select_annotation(index: Option<usize>) -> Self {
        Self::Draw(DrawMsg::SelectAnnotation(index))
    }
    pub const fn delete_selected() -> Self {
        Self::Draw(DrawMsg::DeleteSelected)
    }

    pub const fn move_annotation(index: usize, dx: f32, dy: f32) -> Self {
        Self::Draw(DrawMsg::MoveAnnotation(index, dx, dy))
    }

    pub const fn undo() -> Self {
        Self::Draw(DrawMsg::Undo)
    }
    pub const fn redo() -> Self {
        Self::Draw(DrawMsg::Redo)
    }
    pub const fn clear_shapes() -> Self {
        Self::Draw(DrawMsg::ClearShapes)
    }
    pub const fn clear_redactions() -> Self {
        Self::Draw(DrawMsg::ClearRedactions)
    }

    // Tool shortcuts
    pub const fn shape_mode_toggle() -> Self {
        Self::Tool(ToolMsg::ShapeModeToggle)
    }
    pub const fn annotate_mode_toggle() -> Self {
        Self::Tool(ToolMsg::AnnotateModeToggle)
    }
    pub const fn set_shape_tool(tool: ShapeTool) -> Self {
        Self::Tool(ToolMsg::SetShapeTool(tool))
    }
    pub const fn cycle_shape_tool() -> Self {
        Self::Tool(ToolMsg::CycleShapeTool)
    }
    pub const fn toggle_freehand_popup() -> Self {
        Self::Tool(ToolMsg::FreehandPopup(ToolPopupAction::Toggle))
    }
    pub const fn toggle_shape_popup() -> Self {
        Self::Tool(ToolMsg::ShapePopup(ToolPopupAction::Toggle))
    }
    pub const fn open_shape_popup() -> Self {
        Self::Tool(ToolMsg::ShapePopup(ToolPopupAction::Open))
    }
    pub const fn close_shape_popup() -> Self {
        Self::Tool(ToolMsg::ShapePopup(ToolPopupAction::Close))
    }
    pub const fn set_shape_color(color: AnnotateColor) -> Self {
        Self::Tool(ToolMsg::SetAnnotateColor(color))
    }
    pub const fn set_shape_thickness(v: f32) -> Self {
        Self::Tool(ToolMsg::SetShapeThickness(v))
    }
    pub const fn save_shape_thickness() -> Self {
        Self::Tool(ToolMsg::SaveShapeThickness)
    }

    pub const fn set_redact_tool(tool: RedactTool) -> Self {
        Self::Tool(ToolMsg::SetRedactTool(tool))
    }
    pub const fn redact_tool_mode_toggle() -> Self {
        Self::Tool(ToolMsg::RedactModeToggle)
    }
    pub const fn cycle_redact_tool() -> Self {
        Self::Tool(ToolMsg::CycleRedactTool)
    }
    pub const fn toggle_redact_popup() -> Self {
        Self::Tool(ToolMsg::RedactPopup(ToolPopupAction::Toggle))
    }
    pub const fn open_redact_popup() -> Self {
        Self::Tool(ToolMsg::RedactPopup(ToolPopupAction::Open))
    }
    pub const fn close_redact_popup() -> Self {
        Self::Tool(ToolMsg::RedactPopup(ToolPopupAction::Close))
    }
    pub const fn set_pixelation_block_size(size: u32) -> Self {
        Self::Tool(ToolMsg::SetPixelationBlockSize(size))
    }
    pub const fn save_pixelation_block_size() -> Self {
        Self::Tool(ToolMsg::SavePixelationBlockSize)
    }

    // Magnifier tool shortcuts
    pub const fn magnifier_tool_mode_toggle() -> Self {
        Self::Tool(ToolMsg::MagnifierModeToggle)
    }
    pub const fn toggle_magnifier_popup() -> Self {
        Self::Tool(ToolMsg::MagnifierPopup(ToolPopupAction::Toggle))
    }
    pub const fn open_magnifier_popup() -> Self {
        Self::Tool(ToolMsg::MagnifierPopup(ToolPopupAction::Open))
    }
    pub const fn close_magnifier_popup() -> Self {
        Self::Tool(ToolMsg::MagnifierPopup(ToolPopupAction::Close))
    }
    pub const fn set_magnification(value: f32) -> Self {
        Self::Tool(ToolMsg::SetMagnification(value))
    }
    pub const fn save_magnification() -> Self {
        Self::Tool(ToolMsg::SaveMagnification)
    }

    // Pencil tool shortcuts (for recording annotations)
    pub const fn toggle_pencil_popup() -> Self {
        Self::Tool(ToolMsg::PencilPopup(ToolPopupAction::Toggle))
    }
    pub const fn close_pencil_popup() -> Self {
        Self::Tool(ToolMsg::PencilPopup(ToolPopupAction::Close))
    }
    pub const fn set_pencil_color(color: AnnotateColor) -> Self {
        Self::Tool(ToolMsg::SetPencilColor(color))
    }
    pub const fn set_pencil_fade_duration(duration: f32) -> Self {
        Self::Tool(ToolMsg::SetPencilFadeDuration(duration))
    }
    pub const fn save_pencil_fade_duration() -> Self {
        Self::Tool(ToolMsg::SavePencilFadeDuration)
    }
    pub const fn set_pencil_thickness(thickness: f32) -> Self {
        Self::Tool(ToolMsg::SetPencilThickness(thickness))
    }
    pub const fn save_pencil_thickness() -> Self {
        Self::Tool(ToolMsg::SavePencilThickness)
    }
    pub const fn clear_stroke_drawings() -> Self {
        Self::Tool(ToolMsg::ClearPencilDrawings)
    }

    // Selection shortcuts
    pub const fn region_mode() -> Self {
        Self::Select(SelectMsg::RegionMode)
    }
    pub const fn screen_mode(output_index: usize) -> Self {
        Self::Select(SelectMsg::ScreenMode(output_index))
    }
    pub const fn navigate_left() -> Self {
        Self::Select(SelectMsg::Navigate(Direction::Left))
    }
    pub const fn navigate_right() -> Self {
        Self::Select(SelectMsg::Navigate(Direction::Right))
    }
    pub const fn navigate_up() -> Self {
        Self::Select(SelectMsg::Navigate(Direction::Up))
    }
    pub const fn navigate_down() -> Self {
        Self::Select(SelectMsg::Navigate(Direction::Down))
    }
    pub const fn confirm() -> Self {
        Self::Select(SelectMsg::Confirm)
    }

    // Capture shortcuts
    pub const fn cancel() -> Self {
        Self::Action(ActionMsg::Cancel)
    }
    pub const fn cancel_requested() -> Self {
        Self::Action(ActionMsg::CancelRequested)
    }
    pub const fn copy_to_clipboard() -> Self {
        Self::Action(ActionMsg::CopyToClipboard)
    }
    pub const fn save_to_pictures() -> Self {
        Self::Action(ActionMsg::SaveToPictures)
    }
    pub const fn delayed_capture() -> Self {
        Self::Action(ActionMsg::DelayedCapture)
    }
    pub const fn cycle_capture_delay() -> Self {
        Self::Action(ActionMsg::CycleCaptureDelay)
    }
    pub const fn record_region() -> Self {
        Self::Action(ActionMsg::RecordRegion)
    }
    pub const fn stop_recording() -> Self {
        Self::Action(ActionMsg::StopRecording)
    }
    pub const fn toggle_recording_annotation() -> Self {
        Self::Action(ActionMsg::ToggleRecordingAnnotation)
    }
    pub const fn choice(c: Choice) -> Self {
        Self::Action(ActionMsg::Choice(c))
    }
    pub const fn output_changed(output: WlOutput) -> Self {
        Self::Action(ActionMsg::OutputChanged(output))
    }
    pub const fn open_url(url: String) -> Self {
        Self::Action(ActionMsg::OpenUrl(url))
    }

    // Detection shortcuts
    pub const fn qr_dismiss() -> Self {
        Self::Detect(DetectMsg::Qr(QrMsg::Dismiss))
    }
    pub const fn qr_requested() -> Self {
        Self::Detect(DetectMsg::Qr(QrMsg::Requested))
    }
    pub const fn qr_detected(codes: Vec<DetectedQrCode>) -> Self {
        Self::Detect(DetectMsg::Qr(QrMsg::Detected(codes)))
    }
    pub const fn qr_copy_and_close() -> Self {
        Self::Detect(DetectMsg::Qr(QrMsg::CopyAndClose))
    }
    pub const fn ocr_requested() -> Self {
        Self::Detect(DetectMsg::Ocr(OcrMsg::Requested))
    }
    pub const fn ocr_status(generation: u64, status: OcrStatus) -> Self {
        Self::Detect(DetectMsg::Ocr(OcrMsg::Status(generation, status)))
    }
    pub const fn ocr_copy_and_close() -> Self {
        Self::Detect(DetectMsg::Ocr(OcrMsg::CopyAndClose))
    }

    // Settings shortcuts
    pub const fn toolbar_drag_start() -> Self {
        Self::Settings(SettingsMsg::ToolbarDragStart)
    }
    pub const fn toolbar_drag_move(output: String, position: Point) -> Self {
        Self::Settings(SettingsMsg::ToolbarDragMove(output, position))
    }
    pub const fn toolbar_drag_end() -> Self {
        Self::Settings(SettingsMsg::ToolbarDragEnd)
    }
    pub const fn toggle_settings_drawer() -> Self {
        Self::Settings(SettingsMsg::ToggleDrawer)
    }
    pub const fn toggle_magnifier() -> Self {
        Self::Settings(SettingsMsg::ToggleMagnifier)
    }
    pub const fn set_save_location_pictures() -> Self {
        Self::Settings(SettingsMsg::SetSaveLocation(SaveLocationChoice::Pictures))
    }
    pub const fn set_save_location_documents() -> Self {
        Self::Settings(SettingsMsg::SetSaveLocation(SaveLocationChoice::Documents))
    }
    pub const fn set_save_location_custom() -> Self {
        Self::Settings(SettingsMsg::SetSaveLocation(SaveLocationChoice::Custom))
    }
    pub const fn set_custom_save_path(path: String) -> Self {
        Self::Settings(SettingsMsg::SetCustomSavePath(path))
    }
    pub const fn browse_save_location() -> Self {
        Self::Settings(SettingsMsg::BrowseSaveLocation)
    }
    pub const fn browse_save_location_result(path: Option<String>) -> Self {
        Self::Settings(SettingsMsg::BrowseSaveLocationResult(path))
    }
    pub const fn set_video_save_location_videos() -> Self {
        Self::Settings(SettingsMsg::SetVideoSaveLocation(
            VideoSaveLocationChoice::Videos,
        ))
    }
    pub const fn set_video_save_location_custom() -> Self {
        Self::Settings(SettingsMsg::SetVideoSaveLocation(
            VideoSaveLocationChoice::Custom,
        ))
    }
    pub const fn set_video_custom_save_path(path: String) -> Self {
        Self::Settings(SettingsMsg::SetVideoCustomSavePath(path))
    }
    pub const fn browse_video_save_location() -> Self {
        Self::Settings(SettingsMsg::BrowseVideoSaveLocation)
    }
    pub const fn browse_video_save_location_result(path: Option<String>) -> Self {
        Self::Settings(SettingsMsg::BrowseVideoSaveLocationResult(path))
    }
    pub const fn set_save_location_clipboard() -> Self {
        Self::Settings(SettingsMsg::SetSaveLocationClipboard)
    }
    pub const fn set_capture_delay(secs: u32) -> Self {
        Self::Settings(SettingsMsg::SetCaptureDelay(secs))
    }
    pub const fn toolbar_bounds(bounds: Rectangle) -> Self {
        Self::Settings(SettingsMsg::ToolbarBounds(bounds))
    }
    pub const fn set_video_encoder(encoder: Option<String>) -> Self {
        Self::Settings(SettingsMsg::SetVideoEncoder(encoder))
    }
    pub const fn set_video_container(container: Container) -> Self {
        Self::Settings(SettingsMsg::SetVideoContainer(container))
    }
    pub const fn set_video_framerate(framerate: u32) -> Self {
        Self::Settings(SettingsMsg::SetVideoFramerate(framerate))
    }
    pub const fn toggle_show_cursor() -> Self {
        Self::Settings(SettingsMsg::ToggleShowCursor)
    }
    pub const fn toggle_screenshot_cursor() -> Self {
        Self::Settings(SettingsMsg::ToggleScreenshotCursor)
    }
    pub const fn timeline_tick(window_id: window::Id, instant: Instant) -> Self {
        Self::Settings(SettingsMsg::TimelineTick(window_id, instant))
    }
    pub const fn set_move_offset(offset: Option<(i32, i32)>) -> Self {
        Self::Settings(SettingsMsg::SetMoveOffset(offset))
    }
    pub const fn set_as_default_portal() -> Self {
        Self::Settings(SettingsMsg::SetAsDefaultPortal)
    }
    pub const fn toggle_recognize_qr_codes() -> Self {
        Self::Settings(SettingsMsg::ToggleRecognizeQrCodes)
    }
    pub const fn toggle_stroke_popup() -> Self {
        Self::Tool(ToolMsg::StrokePopup(ToolPopupAction::Toggle))
    }
    pub const fn toggle_delay_popup() -> Self {
        Self::Tool(ToolMsg::DelayPopup(ToolPopupAction::Toggle))
    }
    pub const fn color_picker(update: cosmic::widget::color_picker::ColorPickerUpdate) -> Self {
        Self::Tool(ToolMsg::ColorPicker(update))
    }
    pub const fn toggle_capture_mode(is_video: bool) -> Self {
        Self::Action(ActionMsg::ToggleCaptureMode(is_video))
    }
}
