use std::collections::HashMap;
use tokio::sync::mpsc::Sender;
use zbus::zvariant;
use crate::capture::image::ScreenshotImage;
use crate::capture::ocr::{OcrStatus, OcrTextOverlay};
use crate::capture::qr::DetectedQrCode;
use crate::config::{
    RedactTool, SaveLocation, ShapeColor, ShapeTool, ToolbarPosition,
};
use crate::core::portal::{PortalResponse};
use crate::domain::{
    Action, Annotation, ArrowAnnotation, Choice, CircleOutlineAnnotation,
    ImageSaveLocation, PixelateAnnotation, RectOutlineAnnotation, RedactAnnotation,
};
use crate::screenshot::portal::{ScreenshotOptions, ScreenshotResult};

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
    pub toplevel_images: HashMap<String, Vec<ScreenshotImage>>,
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
                Annotation::Arrow(_) | Annotation::Circle(_) | Annotation::Rectangle(_)
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
        self.redactions.clear();
        self.pixelations.clear();

        for annotation in self.annotations.iter().take(self.annotation_index) {
            match annotation {
                Annotation::Arrow(a) => self.arrows.push(a.clone()),
                Annotation::Circle(c) => self.circles.push(c.clone()),
                Annotation::Rectangle(r) => self.rect_outlines.push(r.clone()),
                Annotation::Redact(r) => self.redactions.push(r.clone()),
                Annotation::Pixelate(p) => self.pixelations.push(p.clone()),
            }
        }
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
    }
}

#[derive(Clone, Debug)]
pub struct SessionState {
    pub choice: Choice,
    pub action: Action,
    pub location: ImageSaveLocation,
    pub highlighted_window_index: usize,
    pub focused_output_index: usize,
    pub also_copy_to_clipboard: bool,
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub toolbar_position: ToolbarPosition,
    pub settings_drawer_open: bool,
    pub primary_shape_tool: ShapeTool,
    pub shape_popup_open: bool,
    pub shape_color: ShapeColor,
    pub shape_shadow: bool,
    pub primary_redact_tool: RedactTool,
    pub redact_popup_open: bool,
    pub pixelation_block_size: u32,
    pub magnifier_enabled: bool,
    pub save_location_setting: SaveLocation,
    pub copy_to_clipboard_on_save: bool,
}

impl UiState {
    pub fn close_all_popups(&mut self) {
        self.shape_popup_open = false;
        self.redact_popup_open = false;
        self.settings_drawer_open = false;
    }
}
