//! Event types for ScreenshotSelection widget
//!
//! These events provide a decoupled interface between the widget and the
//! application's message system. The parent component translates these
//! events to its own message types.

use crate::config::{Container, RedactTool, ShapeColor, ShapeTool, ToolbarPosition};
use crate::domain::Choice;
use wayland_client::protocol::wl_output::WlOutput;

/// Point in widget coordinates
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Type of annotation being drawn
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationType {
    Arrow,
    Circle,
    Rectangle,
    Redact,
    Pixelate,
}

/// Annotation drawing events
#[derive(Clone, Debug)]
pub enum AnnotationEvent {
    /// Drawing started at position
    Started(AnnotationType, Point),
    /// Drawing ended at position
    Ended(AnnotationType, Point),
    /// Mode toggled for an annotation type
    ModeToggle(AnnotationType),
    /// Clear all shape annotations (arrows, circles, rectangles)
    ClearShapes,
    /// Clear all redaction annotations (redact, pixelate)
    ClearRedactions,
}

/// Selection mode events
#[derive(Clone, Debug)]
pub enum SelectionEvent {
    /// Choice changed
    ChoiceChanged(Choice),
    /// Output changed (for wayland output tracking)
    OutputChanged(WlOutput),
    /// Window selected (app_id, output_index)
    WindowSelected(String, usize),
    /// Screen mode activated for an output
    ScreenMode(usize),
    /// Selection confirmed (Enter pressed)
    Confirm,
}

/// Detection feature events
#[derive(Clone, Debug)]
pub enum DetectionEvent {
    /// OCR detection requested
    OcrRequested,
    /// OCR copy and close
    OcrCopyAndClose,
    /// QR detection requested
    QrRequested,
    /// QR copy and close
    QrCopyAndClose,
    /// Open URL from QR code
    OpenUrl(String),
}

/// Tool popup events
#[derive(Clone, Debug)]
pub enum ToolPopupEvent {
    /// Shape popup toggled
    ShapePopupToggle,
    /// Shape popup opened
    ShapePopupOpen,
    /// Shape popup closed
    ShapePopupClose,
    /// Shape tool selected
    ShapeToolSet(ShapeTool),
    /// Shape color selected
    ShapeColorSet(ShapeColor),
    /// Shape shadow toggled
    ShapeShadowToggle,
    /// Shape mode toggled (for primary shape)
    ShapeModeToggle,
    /// Redact mode toggled (for primary redact/pixelate)
    RedactModeToggle,
    /// Redact popup toggled
    RedactPopupToggle,
    /// Redact popup opened
    RedactPopupOpen,
    /// Redact popup closed
    RedactPopupClose,
    /// Redact tool selected
    RedactToolSet(RedactTool),
    /// Pixelation size changed
    PixelationSizeSet(u32),
    /// Pixelation size saved
    PixelationSizeSave,
}

/// Settings drawer events
#[derive(Clone, Debug)]
pub enum SettingsEvent {
    /// Settings drawer toggled
    DrawerToggle,
    /// Magnifier toggled
    MagnifierToggle,
    /// Toolbar position changed
    ToolbarPosition(ToolbarPosition),
    /// Save location set to Pictures
    SaveLocationPictures,
    /// Save location set to Documents
    SaveLocationDocuments,
    /// Copy on save toggled
    CopyOnSaveToggle,
    /// Video encoder selected (gst_element name)
    VideoEncoderSet(String),
    /// Video container format selected
    VideoContainerSet(Container),
    /// Video framerate selected
    VideoFramerateSet(u32),
    /// Show cursor in recordings toggled
    ShowCursorToggle,
}

/// Capture action events
#[derive(Clone, Debug)]
pub enum CaptureEvent {
    /// Copy to clipboard
    CopyToClipboard,
    /// Save to pictures folder
    SaveToPictures,
    /// Record selected region
    RecordRegion,
    /// Cancel screenshot
    Cancel,
    /// Toggle capture mode (screenshot vs video) - true = video mode
    CaptureModeToggle(bool),
}

/// All events that can be emitted by ScreenshotSelection widget
#[derive(Clone, Debug)]
pub enum ScreenshotEvent {
    /// Annotation-related events
    Annotation(AnnotationEvent),
    /// Selection mode events
    Selection(SelectionEvent),
    /// Detection feature events
    Detection(DetectionEvent),
    /// Tool popup events
    ToolPopup(ToolPopupEvent),
    /// Settings events
    Settings(SettingsEvent),
    /// Capture action events
    Capture(CaptureEvent),
}

// ============================================================================
// Convenience constructors
// ============================================================================

impl ScreenshotEvent {
    // Annotation events
    pub fn arrow_start(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Started(
            AnnotationType::Arrow,
            Point::new(x, y),
        ))
    }

    pub fn arrow_end(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Ended(
            AnnotationType::Arrow,
            Point::new(x, y),
        ))
    }

    pub fn circle_start(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Started(
            AnnotationType::Circle,
            Point::new(x, y),
        ))
    }

    pub fn circle_end(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Ended(
            AnnotationType::Circle,
            Point::new(x, y),
        ))
    }

    pub fn rectangle_start(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Started(
            AnnotationType::Rectangle,
            Point::new(x, y),
        ))
    }

    pub fn rectangle_end(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Ended(
            AnnotationType::Rectangle,
            Point::new(x, y),
        ))
    }

    pub fn redact_start(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Started(
            AnnotationType::Redact,
            Point::new(x, y),
        ))
    }

    pub fn redact_end(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Ended(
            AnnotationType::Redact,
            Point::new(x, y),
        ))
    }

    pub fn pixelate_start(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Started(
            AnnotationType::Pixelate,
            Point::new(x, y),
        ))
    }

    pub fn pixelate_end(x: f32, y: f32) -> Self {
        Self::Annotation(AnnotationEvent::Ended(
            AnnotationType::Pixelate,
            Point::new(x, y),
        ))
    }

    pub fn arrow_mode_toggle() -> Self {
        Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Arrow))
    }

    pub fn circle_mode_toggle() -> Self {
        Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Circle))
    }

    pub fn rectangle_mode_toggle() -> Self {
        Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Rectangle))
    }

    pub fn redact_mode_toggle() -> Self {
        Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Redact))
    }

    pub fn pixelate_mode_toggle() -> Self {
        Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Pixelate))
    }

    pub fn clear_shapes() -> Self {
        Self::Annotation(AnnotationEvent::ClearShapes)
    }

    pub fn clear_redactions() -> Self {
        Self::Annotation(AnnotationEvent::ClearRedactions)
    }

    // Selection events
    pub fn choice_changed(choice: Choice) -> Self {
        Self::Selection(SelectionEvent::ChoiceChanged(choice))
    }

    pub fn output_changed(output: WlOutput) -> Self {
        Self::Selection(SelectionEvent::OutputChanged(output))
    }

    pub fn window_selected(app_id: String, output_index: usize) -> Self {
        Self::Selection(SelectionEvent::WindowSelected(app_id, output_index))
    }

    pub fn screen_mode(output_index: usize) -> Self {
        Self::Selection(SelectionEvent::ScreenMode(output_index))
    }

    pub fn confirm() -> Self {
        Self::Selection(SelectionEvent::Confirm)
    }

    // Detection events
    pub fn ocr_requested() -> Self {
        Self::Detection(DetectionEvent::OcrRequested)
    }

    pub fn ocr_copy_and_close() -> Self {
        Self::Detection(DetectionEvent::OcrCopyAndClose)
    }

    pub fn qr_requested() -> Self {
        Self::Detection(DetectionEvent::QrRequested)
    }

    pub fn qr_copy_and_close() -> Self {
        Self::Detection(DetectionEvent::QrCopyAndClose)
    }

    pub fn open_url(url: String) -> Self {
        Self::Detection(DetectionEvent::OpenUrl(url))
    }

    // Tool popup events
    pub fn shape_popup_toggle() -> Self {
        Self::ToolPopup(ToolPopupEvent::ShapePopupToggle)
    }

    pub fn shape_popup_open() -> Self {
        Self::ToolPopup(ToolPopupEvent::ShapePopupOpen)
    }

    pub fn shape_popup_close() -> Self {
        Self::ToolPopup(ToolPopupEvent::ShapePopupClose)
    }

    pub fn shape_tool_set(tool: ShapeTool) -> Self {
        Self::ToolPopup(ToolPopupEvent::ShapeToolSet(tool))
    }

    pub fn shape_color_set(color: ShapeColor) -> Self {
        Self::ToolPopup(ToolPopupEvent::ShapeColorSet(color))
    }

    pub fn shape_shadow_toggle() -> Self {
        Self::ToolPopup(ToolPopupEvent::ShapeShadowToggle)
    }

    pub fn shape_mode_toggle() -> Self {
        Self::ToolPopup(ToolPopupEvent::ShapeModeToggle)
    }

    pub fn redact_tool_mode_toggle() -> Self {
        Self::ToolPopup(ToolPopupEvent::RedactModeToggle)
    }

    pub fn redact_popup_toggle() -> Self {
        Self::ToolPopup(ToolPopupEvent::RedactPopupToggle)
    }

    pub fn redact_popup_open() -> Self {
        Self::ToolPopup(ToolPopupEvent::RedactPopupOpen)
    }

    pub fn redact_popup_close() -> Self {
        Self::ToolPopup(ToolPopupEvent::RedactPopupClose)
    }

    pub fn redact_tool_set(tool: RedactTool) -> Self {
        Self::ToolPopup(ToolPopupEvent::RedactToolSet(tool))
    }

    pub fn pixelation_size_set(size: u32) -> Self {
        Self::ToolPopup(ToolPopupEvent::PixelationSizeSet(size))
    }

    pub fn pixelation_size_save() -> Self {
        Self::ToolPopup(ToolPopupEvent::PixelationSizeSave)
    }

    // Settings events
    pub fn settings_drawer_toggle() -> Self {
        Self::Settings(SettingsEvent::DrawerToggle)
    }

    pub fn magnifier_toggle() -> Self {
        Self::Settings(SettingsEvent::MagnifierToggle)
    }

    pub fn toolbar_position(pos: ToolbarPosition) -> Self {
        Self::Settings(SettingsEvent::ToolbarPosition(pos))
    }

    pub fn save_location_pictures() -> Self {
        Self::Settings(SettingsEvent::SaveLocationPictures)
    }

    pub fn save_location_documents() -> Self {
        Self::Settings(SettingsEvent::SaveLocationDocuments)
    }

    pub fn copy_on_save_toggle() -> Self {
        Self::Settings(SettingsEvent::CopyOnSaveToggle)
    }

    pub fn video_encoder_set(encoder: String) -> Self {
        Self::Settings(SettingsEvent::VideoEncoderSet(encoder))
    }

    pub fn video_container_set(container: Container) -> Self {
        Self::Settings(SettingsEvent::VideoContainerSet(container))
    }

    pub fn video_framerate_set(framerate: u32) -> Self {
        Self::Settings(SettingsEvent::VideoFramerateSet(framerate))
    }

    pub fn show_cursor_toggle() -> Self {
        Self::Settings(SettingsEvent::ShowCursorToggle)
    }

    // Capture events
    pub fn copy_to_clipboard() -> Self {
        Self::Capture(CaptureEvent::CopyToClipboard)
    }

    pub fn save_to_pictures() -> Self {
        Self::Capture(CaptureEvent::SaveToPictures)
    }

    pub fn record_region() -> Self {
        Self::Capture(CaptureEvent::RecordRegion)
    }

    pub fn cancel() -> Self {
        Self::Capture(CaptureEvent::Cancel)
    }

    pub fn capture_mode_toggle(is_video: bool) -> Self {
        Self::Capture(CaptureEvent::CaptureModeToggle(is_video))
    }
}

// ============================================================================
// Conversion from ScreenshotEvent to Msg
// ============================================================================

use crate::session::messages::Msg;

impl ScreenshotEvent {
    /// Convert this event to the application's Msg type
    pub fn to_msg(self) -> Msg {
        match self {
            // Annotation events
            Self::Annotation(AnnotationEvent::Started(AnnotationType::Arrow, p)) => {
                Msg::arrow_start(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Ended(AnnotationType::Arrow, p)) => {
                Msg::arrow_end(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Started(AnnotationType::Circle, p)) => {
                Msg::circle_start(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Ended(AnnotationType::Circle, p)) => {
                Msg::circle_end(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Started(AnnotationType::Rectangle, p)) => {
                Msg::rectangle_start(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Ended(AnnotationType::Rectangle, p)) => {
                Msg::rectangle_end(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Started(AnnotationType::Redact, p)) => {
                Msg::redact_start(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Ended(AnnotationType::Redact, p)) => {
                Msg::redact_end(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Started(AnnotationType::Pixelate, p)) => {
                Msg::pixelate_start(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::Ended(AnnotationType::Pixelate, p)) => {
                Msg::pixelate_end(p.x, p.y)
            }
            Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Arrow)) => {
                Msg::arrow_mode_toggle()
            }
            Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Circle)) => {
                Msg::circle_mode_toggle()
            }
            Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Rectangle)) => {
                Msg::rectangle_mode_toggle()
            }
            Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Redact)) => {
                Msg::redact_mode_toggle()
            }
            Self::Annotation(AnnotationEvent::ModeToggle(AnnotationType::Pixelate)) => {
                Msg::pixelate_mode_toggle()
            }
            Self::Annotation(AnnotationEvent::ClearShapes) => Msg::clear_shapes(),
            Self::Annotation(AnnotationEvent::ClearRedactions) => Msg::clear_redactions(),

            // Selection events
            Self::Selection(SelectionEvent::ChoiceChanged(choice)) => Msg::choice(choice),
            Self::Selection(SelectionEvent::OutputChanged(output)) => Msg::output_changed(output),
            Self::Selection(SelectionEvent::WindowSelected(app_id, idx)) => {
                Msg::window_chosen(app_id, idx)
            }
            Self::Selection(SelectionEvent::ScreenMode(idx)) => Msg::screen_mode(idx),
            Self::Selection(SelectionEvent::Confirm) => Msg::confirm(),

            // Detection events
            Self::Detection(DetectionEvent::OcrRequested) => Msg::ocr_requested(),
            Self::Detection(DetectionEvent::OcrCopyAndClose) => Msg::ocr_copy_and_close(),
            Self::Detection(DetectionEvent::QrRequested) => Msg::qr_requested(),
            Self::Detection(DetectionEvent::QrCopyAndClose) => Msg::qr_copy_and_close(),
            Self::Detection(DetectionEvent::OpenUrl(url)) => Msg::open_url(url),

            // Tool popup events
            Self::ToolPopup(ToolPopupEvent::ShapePopupToggle) => Msg::toggle_shape_popup(),
            Self::ToolPopup(ToolPopupEvent::ShapePopupOpen) => Msg::open_shape_popup(),
            Self::ToolPopup(ToolPopupEvent::ShapePopupClose) => Msg::close_shape_popup(),
            Self::ToolPopup(ToolPopupEvent::ShapeToolSet(tool)) => Msg::set_shape_tool(tool),
            Self::ToolPopup(ToolPopupEvent::ShapeColorSet(color)) => Msg::set_shape_color(color),
            Self::ToolPopup(ToolPopupEvent::ShapeShadowToggle) => Msg::toggle_shape_shadow(),
            Self::ToolPopup(ToolPopupEvent::ShapeModeToggle) => Msg::shape_mode_toggle(),
            Self::ToolPopup(ToolPopupEvent::RedactModeToggle) => Msg::redact_tool_mode_toggle(),
            Self::ToolPopup(ToolPopupEvent::RedactPopupToggle) => Msg::toggle_redact_popup(),
            Self::ToolPopup(ToolPopupEvent::RedactPopupOpen) => Msg::open_redact_popup(),
            Self::ToolPopup(ToolPopupEvent::RedactPopupClose) => Msg::close_redact_popup(),
            Self::ToolPopup(ToolPopupEvent::RedactToolSet(tool)) => Msg::set_redact_tool(tool),
            Self::ToolPopup(ToolPopupEvent::PixelationSizeSet(size)) => {
                Msg::set_pixelation_block_size(size)
            }
            Self::ToolPopup(ToolPopupEvent::PixelationSizeSave) => {
                Msg::save_pixelation_block_size()
            }

            // Settings events
            Self::Settings(SettingsEvent::DrawerToggle) => Msg::toggle_settings_drawer(),
            Self::Settings(SettingsEvent::MagnifierToggle) => Msg::toggle_magnifier(),
            Self::Settings(SettingsEvent::ToolbarPosition(pos)) => Msg::toolbar_position(pos),
            Self::Settings(SettingsEvent::SaveLocationPictures) => {
                Msg::set_save_location_pictures()
            }
            Self::Settings(SettingsEvent::SaveLocationDocuments) => {
                Msg::set_save_location_documents()
            }
            Self::Settings(SettingsEvent::CopyOnSaveToggle) => Msg::toggle_copy_on_save(),
            Self::Settings(SettingsEvent::VideoEncoderSet(encoder)) => {
                Msg::set_video_encoder(encoder)
            }
            Self::Settings(SettingsEvent::VideoContainerSet(container)) => {
                Msg::set_video_container(container)
            }
            Self::Settings(SettingsEvent::VideoFramerateSet(framerate)) => {
                Msg::set_video_framerate(framerate)
            }
            Self::Settings(SettingsEvent::ShowCursorToggle) => Msg::toggle_show_cursor(),

            // Capture events
            Self::Capture(CaptureEvent::CopyToClipboard) => Msg::copy_to_clipboard(),
            Self::Capture(CaptureEvent::SaveToPictures) => Msg::save_to_pictures(),
            Self::Capture(CaptureEvent::RecordRegion) => Msg::record_region(),
            Self::Capture(CaptureEvent::Cancel) => Msg::cancel(),
            Self::Capture(CaptureEvent::CaptureModeToggle(is_video)) => {
                Msg::Capture(crate::session::messages::CaptureMsg::ToggleCaptureMode(is_video))
            }
        }
    }
}
