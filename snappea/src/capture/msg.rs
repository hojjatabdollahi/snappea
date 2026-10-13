// SPDX-License-Identifier: GPL-3.0-only

//! Messages for a capture in progress.

use wayland_client::protocol::wl_output::WlOutput;

use crate::capture::detect::DetectedQrCode;
use crate::capture::detect::OcrStatus;
use crate::config::{Container, SaveLocationChoice, ShapeTool, VideoSaveLocationChoice};
use crate::geometry::Choice;
use cosmic::iced::core::{Point, Rectangle};
use cosmic::iced::time::Instant;
use cosmic::iced::window;
use cosmic::widget::segmented_button;
use viewer_tools::annotate::AnnotateColor;

/// One gesture of a drawing tool, in global logical coordinates.
#[derive(Debug, Clone, Copy)]
pub enum DrawAction {
    Start(f32, f32),
    Move(f32, f32),
    End(f32, f32),
}

/// What can happen to a text label. More than `DrawAction`, since an edit is
/// placed, typed, styled and finished.
#[derive(Debug, Clone)]
pub enum TextAction {
    Press(f32, f32),
    Drag(f32, f32),
    Release(f32, f32),
    /// A key, with the text it produced if any
    Key(
        cosmic::iced::keyboard::Key,
        cosmic::iced::keyboard::Modifiers,
        Option<String>,
    ),
    Commit,
    Paste(String),
    Family(usize),
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
    Shape(viewer_tools::annotate::ShapeKind, DrawAction),
    Text(TextAction),
    Pen(DrawAction),
    Magnifier(DrawAction),
    MagnifierSelect(Option<usize>),
    MagnifierMove(usize, f32, f32),
    MagnifierResize(usize, f32),
    MagnifierSetZoom(usize, f32),
    Redact(DrawAction),
    Pixelate(DrawAction),
    ToggleMoveMode,
    SelectAnnotation(Option<usize>),
    DeleteSelected,
    MoveAnnotation(usize, f32, f32),
    Undo,
    Redo,
}

/// Tool popup actions
#[derive(Debug, Clone, Copy)]
pub enum ToolPopupAction {
    Toggle,
    Close,
}

/// Tool configuration messages (popups and settings for shape/redact tools)
#[derive(Debug, Clone)]
pub enum ToolMsg {
    ShapeModeToggle,
    AnnotateModeToggle,
    SetShapeTool(ShapeTool),
    CycleShapeTool,
    ShapePopup(ToolPopupAction),
    FreehandPopup(ToolPopupAction),
    DelayPopup(ToolPopupAction),
    StrokePopup(ToolPopupAction),
    FontPopup(ToolPopupAction),
    SetAnnotateColor(AnnotateColor),
    ColorPicker(cosmic::widget::color_picker::ColorPickerUpdate),
    SetShapeThickness(f32),
    RedactModeToggle,
    CycleRedactTool,
    SetPencilColor(AnnotateColor),
    SetPencilFadeDuration(f32),
    SetPencilThickness(f32),
}

/// Navigation direction for keyboard navigation
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Left,
    Right,
}

/// Selection mode and navigation messages
#[derive(Debug, Clone)]
pub enum SelectMsg {
    RegionMode,
    ScreenMode(usize),
    Navigate(Direction),
    Confirm,
}

/// Capture and save workflow messages
#[derive(Debug, Clone)]
pub enum ActionMsg {
    Capture,
    Cancel,
    /// Escape was pressed. Cancels immediately unless there is unsaved
    /// annotation work, in which case it asks for confirmation first.
    CancelRequested,
    CopyToClipboard,
    SaveToPictures,
    DelayedCapture,
    RecordRegion,
    StopRecording,
    ToggleRecordingAnnotation,
    ToggleCaptureMode(bool),
    Choice(Choice),
    OutputChanged(WlOutput),
    OpenUrl(String),
}

/// QR code detection messages
#[derive(Debug, Clone)]
pub enum QrMsg {
    Dismiss,
    Requested,
    Detected(Vec<DetectedQrCode>),
    CopyAndClose,
}

/// OCR detection messages
#[derive(Debug, Clone)]
pub enum OcrMsg {
    Requested,
    Status(u64, OcrStatus),
    CopyAndClose,
}

/// Detection feature messages (OCR and QR)
#[derive(Debug, Clone)]
pub enum DetectMsg {
    Qr(QrMsg),
    Ocr(OcrMsg),
}

/// Settings and UI messages
#[derive(Debug, Clone)]
pub enum SettingsMsg {
    ToolbarDragStart,
    ToolbarDragMove(String, Point),
    ToolbarDragEnd,
    ToggleDrawer,
    ToggleMagnifier,
    SetSaveLocation(SaveLocationChoice),
    BrowseSaveLocation,
    BrowseSaveLocationResult(Option<String>),
    SetVideoSaveLocation(VideoSaveLocationChoice),
    BrowseVideoSaveLocation,
    BrowseVideoSaveLocationResult(Option<String>),
    SetCaptureDelay(u32),
    ToolbarBounds(Rectangle),
    SetVideoEncoder(Option<String>),
    SetVideoContainer(Container),
    SetVideoFramerate(u32),
    ToggleShowCursor,
    ToggleScreenshotCursor,
    EncodersDetected(Vec<crate::recording::encoder::EncoderInfo>),
    TimelineTick(window::Id, Instant),
    /// selection rectangle
    SetMoveOffset(Option<(i32, i32)>),
    SetAsDefaultPortal,
    ToggleRecognizeQrCodes,
}

/// Everything the overlay can ask for.
#[derive(Debug, Clone)]
pub enum Msg {
    Draw(DrawMsg),
    Tool(ToolMsg),
    Select(SelectMsg),
    Action(ActionMsg),
    Detect(DetectMsg),
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
    pub const fn pen_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pen(DrawAction::Start(x, y)))
    }
    pub const fn pen_move(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pen(DrawAction::Move(x, y)))
    }
    pub const fn pen_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Pen(DrawAction::End(x, y)))
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
    pub const fn redact_start(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Redact(DrawAction::Start(x, y)))
    }
    pub const fn redact_end(x: f32, y: f32) -> Self {
        Self::Draw(DrawMsg::Redact(DrawAction::End(x, y)))
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
    pub const fn close_shape_popup() -> Self {
        Self::Tool(ToolMsg::ShapePopup(ToolPopupAction::Close))
    }
    pub const fn set_shape_color(color: AnnotateColor) -> Self {
        Self::Tool(ToolMsg::SetAnnotateColor(color))
    }
    pub const fn set_shape_thickness(v: f32) -> Self {
        Self::Tool(ToolMsg::SetShapeThickness(v))
    }
    pub const fn redact_tool_mode_toggle() -> Self {
        Self::Tool(ToolMsg::RedactModeToggle)
    }
    pub const fn cycle_redact_tool() -> Self {
        Self::Tool(ToolMsg::CycleRedactTool)
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
    pub const fn record_region() -> Self {
        Self::Action(ActionMsg::RecordRegion)
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
    pub const fn set_save_location_clipboard() -> Self {
        Self::Settings(SettingsMsg::SetSaveLocation(SaveLocationChoice::Clipboard))
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
    pub const fn browse_video_save_location() -> Self {
        Self::Settings(SettingsMsg::BrowseVideoSaveLocation)
    }
    pub const fn browse_video_save_location_result(path: Option<String>) -> Self {
        Self::Settings(SettingsMsg::BrowseVideoSaveLocationResult(path))
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
