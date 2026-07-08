use crate::capture::image::ScreenshotImage;
use crate::capture::ocr::{OcrStatus, OcrTextOverlay};
use crate::capture::qr::DetectedQrCode;
use crate::config::{
    Container, RedactTool, SaveLocationChoice, ShapeColor, ShapeTool, ToolbarPosition,
    VideoSaveLocationChoice,
};
use crate::core::portal::PortalResponse;
use crate::domain::{
    Action, Annotation, ArrowAnnotation, Choice, CircleOutlineAnnotation, ImageSaveLocation,
    MagnifierAnnotation, PixelateAnnotation, RectOutlineAnnotation, RedactAnnotation,
};
use crate::screencast::encoder::EncoderInfo;
use crate::screenshot::portal::{ScreenshotOptions, ScreenshotResult};
use cosmic::iced::Animation;
use cosmic::iced::core::Rectangle;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc::Sender;
use zbus::zvariant;

#[derive(Clone, Debug)]
pub struct PortalContext {
    pub handle: zvariant::ObjectPath<'static>,
    pub app_id: String,
    pub parent_window: String,
    pub options: ScreenshotOptions,
    pub tx: Sender<PortalResponse<ScreenshotResult>>,
}

#[derive(Clone, Debug)]
pub struct CaptureData {
    pub output_images: HashMap<String, ScreenshotImage>,
}

#[derive(Clone, Debug, Default)]
pub struct DetectionState {
    pub qr_codes: Vec<DetectedQrCode>,
    pub qr_scanning: bool,
    pub ocr_status: OcrStatus,
    pub ocr_overlays: Vec<OcrTextOverlay>,
    pub ocr_text: Option<String>,
}

impl DetectionState {
    pub fn clear(&mut self) {
        self.ocr_status = OcrStatus::Idle;
        self.ocr_text = None;
        self.ocr_overlays.clear();
        self.qr_codes.clear();
        self.qr_scanning = false;
    }
}

#[derive(Clone, Debug, Default)]
pub struct AnnotationState {
    pub annotations: Vec<Annotation>,
    pub annotation_index: usize,
    pub arrows: Vec<ArrowAnnotation>,
    pub arrow_mode: bool,
    pub arrow_drawing: Option<(f32, f32)>,
    pub redactions: Vec<RedactAnnotation>,
    pub redact_mode: bool,
    pub redact_drawing: Option<(f32, f32)>,
    pub pixelations: Vec<PixelateAnnotation>,
    pub pixelate_mode: bool,
    pub pixelate_drawing: Option<(f32, f32)>,
    pub circles: Vec<CircleOutlineAnnotation>,
    pub circle_mode: bool,
    pub circle_drawing: Option<(f32, f32)>,
    pub rect_outlines: Vec<RectOutlineAnnotation>,
    pub rect_outline_mode: bool,
    pub rect_outline_drawing: Option<(f32, f32)>,
    pub magnifiers: Vec<MagnifierAnnotation>,
    pub magnifier_mode: bool,
    pub magnifier_drawing: Option<(f32, f32)>,
    /// Index (into `magnifiers`) of the currently selected magnifier, if any
    pub selected_magnifier: Option<usize>,
}

impl AnnotationState {
    pub fn clear_all(&mut self) {
        self.annotations.clear();
        self.annotation_index = 0;
        self.arrows.clear();
        self.arrow_mode = false;
        self.arrow_drawing = None;
        self.redactions.clear();
        self.redact_mode = false;
        self.redact_drawing = None;
        self.pixelations.clear();
        self.pixelate_mode = false;
        self.pixelate_drawing = None;
        self.circles.clear();
        self.circle_mode = false;
        self.circle_drawing = None;
        self.rect_outlines.clear();
        self.rect_outline_mode = false;
        self.rect_outline_drawing = None;
        self.magnifiers.clear();
        self.magnifier_mode = false;
        self.magnifier_drawing = None;
        self.selected_magnifier = None;
    }

    pub fn clear_shapes(&mut self) {
        self.arrows.clear();
        self.arrow_drawing = None;
        self.arrow_mode = false;
        self.circles.clear();
        self.circle_drawing = None;
        self.circle_mode = false;
        self.rect_outlines.clear();
        self.rect_outline_drawing = None;
        self.rect_outline_mode = false;
        self.magnifiers.clear();
        self.magnifier_drawing = None;
        self.magnifier_mode = false;
        self.selected_magnifier = None;
        // Also filter unified annotations array
        self.annotations
            .retain(|a| matches!(a, Annotation::Redact(_) | Annotation::Pixelate(_)));
        self.annotation_index = self.annotations.len();
    }

    pub fn clear_redactions(&mut self) {
        // Clear only redaction annotations from the unified array
        self.annotations.retain(|a| {
            matches!(
                a,
                Annotation::Arrow(_)
                    | Annotation::Circle(_)
                    | Annotation::Rectangle(_)
                    | Annotation::Magnifier(_)
            )
        });
        self.annotation_index = self.annotations.len();

        // Clear redaction arrays
        self.redactions.clear();
        self.pixelations.clear();

        // Disable redact modes
        self.redact_mode = false;
        self.redact_drawing = None;
        self.pixelate_mode = false;
        self.pixelate_drawing = None;
    }

    pub fn undo(&mut self) {
        if self.annotation_index > 0 {
            self.annotation_index -= 1;
            self.rebuild_arrays();
        }
    }

    pub fn redo(&mut self) {
        if self.annotation_index < self.annotations.len() {
            self.annotation_index += 1;
            self.rebuild_arrays();
        }
    }

    pub fn add(&mut self, annotation: Annotation) {
        // Truncate any redo history
        self.annotations.truncate(self.annotation_index);
        self.annotations.push(annotation);
        self.annotation_index = self.annotations.len();
    }

    pub fn rebuild_arrays(&mut self) {
        self.arrows.clear();
        self.circles.clear();
        self.rect_outlines.clear();
        self.magnifiers.clear();
        self.redactions.clear();
        self.pixelations.clear();

        for annotation in self.annotations.iter().take(self.annotation_index) {
            match annotation {
                Annotation::Arrow(a) => self.arrows.push(a.clone()),
                Annotation::Circle(c) => self.circles.push(c.clone()),
                Annotation::Rectangle(r) => self.rect_outlines.push(r.clone()),
                Annotation::Magnifier(m) => self.magnifiers.push(m.clone()),
                Annotation::Redact(r) => self.redactions.push(r.clone()),
                Annotation::Pixelate(p) => self.pixelations.push(p.clone()),
            }
        }

        // Keep the selection valid after the arrays change (e.g. undo/redo)
        if let Some(idx) = self.selected_magnifier
            && idx >= self.magnifiers.len()
        {
            self.selected_magnifier = None;
        }
    }

    /// Map the selected magnifier (index into `magnifiers`) to its position in
    /// the unified `annotations` array (respecting the current undo index).
    fn selected_magnifier_unified_index(&self) -> Option<usize> {
        let target = self.selected_magnifier?;
        let mut count = 0;
        for (i, a) in self.annotations.iter().take(self.annotation_index).enumerate() {
            if matches!(a, Annotation::Magnifier(_)) {
                if count == target {
                    return Some(i);
                }
                count += 1;
            }
        }
        None
    }

    /// The magnification of the currently selected magnifier, if any.
    pub fn selected_magnifier_zoom(&self) -> Option<f32> {
        self.selected_magnifier
            .and_then(|i| self.magnifiers.get(i))
            .map(|m| m.magnification)
    }

    /// Apply an in-place edit to the currently selected magnifier, updating both
    /// the unified annotation array (source of truth) and the flat `magnifiers`.
    pub fn edit_selected_magnifier(&mut self, f: impl Fn(&mut MagnifierAnnotation)) {
        let Some(unified_idx) = self.selected_magnifier_unified_index() else {
            return;
        };
        if let Some(Annotation::Magnifier(m)) = self.annotations.get_mut(unified_idx) {
            f(m);
        }
        self.rebuild_arrays();
    }

    pub fn disable_all_modes(&mut self) {
        self.arrow_mode = false;
        self.arrow_drawing = None;
        self.redact_mode = false;
        self.redact_drawing = None;
        self.pixelate_mode = false;
        self.pixelate_drawing = None;
        self.circle_mode = false;
        self.circle_drawing = None;
        self.rect_outline_mode = false;
        self.rect_outline_drawing = None;
        self.magnifier_mode = false;
        self.magnifier_drawing = None;
        // Note: `selected_magnifier` is intentionally preserved here so the
        // right-click config popup (which disables modes) can still edit the
        // selected magnifier. It is cleared when switching to another tool.
    }
}

#[derive(Clone, Debug)]
pub struct SessionState {
    pub choice: Choice,
    pub action: Action,
    pub location: ImageSaveLocation,
    pub focused_output_index: usize,
    pub also_copy_to_clipboard: bool,
    /// Whether the mouse has entered any output yet (used to avoid showing wrong initial highlight)
    pub has_mouse_entered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Picture,
    Video,
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub now: Instant,
    pub toolbar_position: ToolbarPosition,
    pub settings_drawer_open: bool,
    pub settings_tab: SettingsTab,
    pub primary_shape_tool: ShapeTool,
    pub shape_popup_open: bool,
    pub shape_color: ShapeColor,
    pub shape_shadow: bool,
    pub primary_redact_tool: RedactTool,
    pub redact_popup_open: bool,
    pub pixelation_block_size: u32,
    /// Magnifier annotation tool: whether its config popup is open
    pub magnifier_popup_open: bool,
    /// Magnifier annotation tool: zoom level (1.5-10.0)
    pub magnifier_magnification: f32,
    /// Delay (seconds) for the delayed-screenshot toolbar button
    pub capture_delay_secs: u32,
    pub magnifier_enabled: bool,
    pub save_location_setting: SaveLocationChoice,
    pub custom_save_path: String,
    pub video_save_location_setting: VideoSaveLocationChoice,
    pub video_custom_save_path: String,
    pub copy_to_clipboard_on_save: bool,
    pub toolbar_unhovered_opacity: f32,
    /// Whether the toolbar is currently being hovered (for animated opacity)
    pub toolbar_is_hovered: bool,
    pub toolbar_hover_animation: Animation<bool>,
    /// ID for debouncing toolbar opacity saves (incremented on each change)
    pub toolbar_opacity_save_id: u64,
    pub tesseract_available: bool,
    // Recording settings
    pub available_encoders: Vec<EncoderInfo>,
    pub encoder_displays: Vec<(String, String)>, // Cached (display_name, gst_element) for UI
    pub selected_encoder: Option<String>,
    pub video_container: Container,
    pub video_framerate: u32,
    pub video_show_cursor: bool,
    /// Whether video mode is selected (false = screenshot, true = video)
    pub is_video_mode: bool,
    pub capture_mode_animation: Animation<bool>,
    /// Whether recording is currently active
    pub is_recording: bool,
    /// Whether annotation mode is active during recording
    pub recording_annotation_mode: bool,
    /// Whether pencil popup is open during recording
    pub pencil_popup_open: bool,
    /// Pencil color for recording annotations
    pub pencil_color: ShapeColor,
    /// Duration in seconds before pencil strokes fade away
    pub pencil_fade_duration: f32,
    /// Pencil line thickness in pixels
    pub pencil_thickness: f32,
    /// Last known toolbar bounds (output-local)
    pub toolbar_bounds: Option<Rectangle>,
    /// Whether to hide toolbar to system tray when recording
    pub hide_toolbar_to_tray: bool,
    /// Move offset for dragging selection rectangle (cursor pos relative to rect top-left when move started)
    pub move_offset: Option<(i32, i32)>,
    /// Whether snappea is currently set as the default screenshot portal for the current user
    pub is_default_portal: bool,
}

impl UiState {
    pub fn is_animating(&self) -> bool {
        self.toolbar_hover_animation.is_animating(self.now)
            || self.capture_mode_animation.is_animating(self.now)
    }

    pub fn close_all_popups(&mut self) {
        self.shape_popup_open = false;
        self.redact_popup_open = false;
        self.magnifier_popup_open = false;
        self.settings_drawer_open = false;
        self.pencil_popup_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ArrowAnnotation, MagnifierAnnotation};

    fn magnifier(cx: f32, cy: f32, r: f32, zoom: f32) -> MagnifierAnnotation {
        MagnifierAnnotation {
            start_x: cx - r,
            start_y: cy - r,
            end_x: cx + r,
            end_y: cy + r,
            magnification: zoom,
            color: ShapeColor::default(),
            shadow: true,
        }
    }

    fn arrow() -> ArrowAnnotation {
        ArrowAnnotation {
            start_x: 0.0,
            start_y: 0.0,
            end_x: 10.0,
            end_y: 10.0,
            color: ShapeColor::default(),
            shadow: true,
        }
    }

    #[test]
    fn edit_selected_magnifier_targets_correct_unified_entry() {
        let mut st = AnnotationState::default();
        // Interleave an arrow between two magnifiers to exercise index mapping
        st.add(Annotation::Magnifier(magnifier(100.0, 100.0, 50.0, 2.0)));
        st.add(Annotation::Arrow(arrow()));
        st.add(Annotation::Magnifier(magnifier(300.0, 300.0, 40.0, 3.0)));
        st.rebuild_arrays();

        assert_eq!(st.magnifiers.len(), 2);

        // Select the SECOND magnifier and change its zoom
        st.selected_magnifier = Some(1);
        st.edit_selected_magnifier(|m| m.magnification = 7.5);

        // Flat array updated
        assert_eq!(st.magnifiers[1].magnification, 7.5);
        assert_eq!(st.magnifiers[0].magnification, 2.0);
        // Unified (source of truth) updated at the right index (annotations[2])
        match &st.annotations[2] {
            Annotation::Magnifier(m) => assert_eq!(m.magnification, 7.5),
            other => panic!("expected magnifier, got {other:?}"),
        }
        // The arrow in between is untouched
        assert!(matches!(st.annotations[1], Annotation::Arrow(_)));
    }

    #[test]
    fn set_geometry_moves_and_resizes_as_circle() {
        let mut m = magnifier(100.0, 100.0, 50.0, 2.0);
        m.set_geometry(200.0, 250.0, 30.0);
        assert_eq!(m.center(), (200.0, 250.0));
        assert_eq!(m.radius(), 30.0);
    }

    #[test]
    fn selection_cleared_when_index_becomes_invalid() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Magnifier(magnifier(10.0, 10.0, 5.0, 2.0)));
        st.rebuild_arrays();
        st.selected_magnifier = Some(0);
        // Undo removes the magnifier; selection must not dangle
        st.undo();
        assert_eq!(st.selected_magnifier, None);
    }
}
