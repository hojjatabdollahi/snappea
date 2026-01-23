use crate::capture::image::ScreenshotImage;
use crate::capture::ocr::{OcrStatus, OcrTextOverlay};
use crate::capture::qr::DetectedQrCode;
use crate::config::{Container, RedactTool, SaveLocation, ShapeColor, ShapeTool, ToolbarPosition};
use crate::core::portal::PortalResponse;
use crate::domain::{
    Action, Annotation, ArrowAnnotation, Choice, CircleOutlineAnnotation, ImageSaveLocation,
    PixelateAnnotation, RectOutlineAnnotation, RedactAnnotation,
};
use crate::screencast::encoder::EncoderInfo;
use crate::screenshot::portal::{ScreenshotOptions, ScreenshotResult};
use cosmic::widget::segmented_button;
use cosmic_time::Timeline;
use std::collections::HashMap;
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
    pub toplevel_images: HashMap<String, Vec<ScreenshotImage>>,
    /// Maps (output_name, local_index) -> global_index for toplevel recording
    /// The outer HashMap key is output_name, inner Vec index is local_index, value is global_index
    pub toplevel_global_indices: HashMap<String, Vec<usize>>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Picture,
    Video,
}

#[derive(Debug)]
pub struct SettingsTabModel {
    pub model: segmented_button::SingleSelectModel,
    pub general_id: segmented_button::Entity,
    pub picture_id: segmented_button::Entity,
    pub video_id: segmented_button::Entity,
    pub active_tab: SettingsTab,
}

unsafe impl Send for SettingsTabModel {}
unsafe impl Sync for SettingsTabModel {}

impl SettingsTabModel {
    pub fn new(active_tab: SettingsTab) -> Self {
        let mut general_id = None;
        let mut picture_id = None;
        let mut video_id = None;

        let mut model = segmented_button::Model::builder()
            .insert(|b| {
                b.text("General")
                    .with_id(|id| general_id = Some(id))
                    .data(SettingsTab::General)
            })
            .insert(|b| {
                b.text("Picture")
                    .with_id(|id| picture_id = Some(id))
                    .data(SettingsTab::Picture)
            })
            .insert(|b| {
                b.text("Video")
                    .with_id(|id| video_id = Some(id))
                    .data(SettingsTab::Video)
            })
            .build();

        let general_id = general_id.expect("General tab id missing");
        let picture_id = picture_id.expect("Picture tab id missing");
        let video_id = video_id.expect("Video tab id missing");

        let active_id = match active_tab {
            SettingsTab::General => general_id,
            SettingsTab::Picture => picture_id,
            SettingsTab::Video => video_id,
        };
        model.activate(active_id);

        Self {
            model,
            general_id,
            picture_id,
            video_id,
            active_tab,
        }
    }

    pub fn activate_tab(&mut self, tab: SettingsTab) {
        if self.active_tab == tab {
            return;
        }

        let id = match tab {
            SettingsTab::General => self.general_id,
            SettingsTab::Picture => self.picture_id,
            SettingsTab::Video => self.video_id,
        };
        self.model.activate(id);
        self.active_tab = tab;
    }
}

impl Clone for SettingsTabModel {
    fn clone(&self) -> Self {
        Self::new(self.active_tab)
    }
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub toolbar_position: ToolbarPosition,
    pub settings_drawer_open: bool,
    pub settings_tab: SettingsTab,
    pub settings_tab_model: SettingsTabModel,
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
    pub toolbar_unhovered_opacity: f32,
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
    /// Animation timeline for UI animations
    pub timeline: Timeline,
}

impl UiState {
    pub fn close_all_popups(&mut self) {
        self.shape_popup_open = false;
        self.redact_popup_open = false;
        self.settings_drawer_open = false;
    }
}
