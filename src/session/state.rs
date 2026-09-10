use crate::capture::image::ScreenshotImage;
use crate::capture::ocr::{OcrStatus, OcrTextOverlay};
use crate::capture::qr::DetectedQrCode;
use crate::config::{
    Container, KeyBinding, RedactTool, SaveLocationChoice, ShapeColor, ShapeTool, ToolbarPosition,
    VideoSaveLocationChoice,
};
use crate::core::portal::PortalResponse;
use crate::domain::{
    Action, Annotation, ArrowAnnotation, Choice, CircleOutlineAnnotation, ImageSaveLocation,
    LineAnnotation, MagnifierAnnotation, PencilAnnotation, PixelateAnnotation,
    RectOutlineAnnotation, RedactAnnotation, TextAnnotation, TextEditing,
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
    /// Whether a D-Bus caller is actually awaiting the response on `tx`.
    ///
    /// True for a real `org.freedesktop.impl.portal.Screenshot` request (the
    /// caller expects a file URI back); false for user-initiated captures
    /// (PrintScreen/control) where the response is discarded. When true, a file
    /// must always be written even if the user chose "copy to clipboard", so the
    /// caller receives a usable URI rather than `clipboard:///`.
    pub expects_response: bool,
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
    pub lines: Vec<LineAnnotation>,
    pub line_mode: bool,
    pub line_drawing: Option<(f32, f32)>,
    pub pencils: Vec<PencilAnnotation>,
    pub pencil_mode: bool,
    /// Points collected so far for the in-progress freehand stroke
    pub pencil_drawing: Option<Vec<(f32, f32)>>,
    pub texts: Vec<TextAnnotation>,
    pub text_mode: bool,
    /// The text annotation currently being typed, if any
    pub text_editing: Option<TextEditing>,
    /// Index (into `texts`) of the currently selected label, if any
    pub selected_text: Option<usize>,
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
        self.lines.clear();
        self.line_mode = false;
        self.line_drawing = None;
        self.pencils.clear();
        self.pencil_mode = false;
        self.pencil_drawing = None;
        self.texts.clear();
        self.text_mode = false;
        self.text_editing = None;
        self.selected_text = None;
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
        self.lines.clear();
        self.line_drawing = None;
        self.line_mode = false;
        self.pencils.clear();
        self.pencil_drawing = None;
        self.pencil_mode = false;
        self.texts.clear();
        self.text_editing = None;
        self.text_mode = false;
        self.selected_text = None;
        self.magnifiers.clear();
        self.magnifier_drawing = None;
        self.magnifier_mode = false;
        self.selected_magnifier = None;
        // Also filter unified annotations array
        self.annotations.retain(|a| a.is_redaction());
        self.annotation_index = self.annotations.len();
    }

    pub fn clear_redactions(&mut self) {
        // Clear only redaction annotations from the unified array
        self.annotations.retain(|a| a.is_shape());
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
        self.lines.clear();
        self.pencils.clear();
        self.texts.clear();
        self.magnifiers.clear();
        self.redactions.clear();
        self.pixelations.clear();

        for annotation in self.annotations.iter().take(self.annotation_index) {
            match annotation {
                Annotation::Arrow(a) => self.arrows.push(a.clone()),
                Annotation::Line(l) => self.lines.push(l.clone()),
                Annotation::Circle(c) => self.circles.push(c.clone()),
                Annotation::Rectangle(r) => self.rect_outlines.push(r.clone()),
                Annotation::Pencil(p) => self.pencils.push(p.clone()),
                Annotation::Text(t) => self.texts.push(t.clone()),
                Annotation::Magnifier(m) => self.magnifiers.push(m.clone()),
                Annotation::Redact(r) => self.redactions.push(r.clone()),
                Annotation::Pixelate(p) => self.pixelations.push(p.clone()),
            }
        }

        // Keep the selections valid after the arrays change (e.g. undo/redo)
        if let Some(idx) = self.selected_magnifier
            && idx >= self.magnifiers.len()
        {
            self.selected_magnifier = None;
        }
        if let Some(idx) = self.selected_text
            && idx >= self.texts.len()
        {
            self.selected_text = None;
        }
    }

    /// Map the nth annotation matching `pred` to its index in the unified
    /// `annotations` array (respecting the current undo index).
    ///
    /// The flat per-type vectors are projections of `annotations`, so editing an
    /// item in place requires translating back to the unified index.
    fn unified_index_of_nth(&self, n: usize, pred: fn(&Annotation) -> bool) -> Option<usize> {
        let mut count = 0;
        for (i, a) in self
            .annotations
            .iter()
            .take(self.annotation_index)
            .enumerate()
        {
            if pred(a) {
                if count == n {
                    return Some(i);
                }
                count += 1;
            }
        }
        None
    }

    /// The unified index of the text label at `texts[text_idx]`.
    pub fn unified_index_of_text(&self, text_idx: usize) -> Option<usize> {
        self.unified_index_of_nth(text_idx, |a| matches!(a, Annotation::Text(_)))
    }

    /// Apply an in-place edit to the currently selected text label, updating both
    /// the unified annotation array (source of truth) and the flat `texts` list.
    pub fn edit_selected_text(&mut self, f: impl Fn(&mut TextAnnotation)) {
        let Some(sel) = self.selected_text else {
            return;
        };
        let Some(unified_idx) = self.unified_index_of_text(sel) else {
            return;
        };
        if let Some(Annotation::Text(t)) = self.annotations.get_mut(unified_idx) {
            f(t);
        }
        self.rebuild_arrays();
    }

    /// Map the selected magnifier (index into `magnifiers`) to its position in
    /// the unified `annotations` array (respecting the current undo index).
    fn selected_magnifier_unified_index(&self) -> Option<usize> {
        let target = self.selected_magnifier?;
        let mut count = 0;
        for (i, a) in self
            .annotations
            .iter()
            .take(self.annotation_index)
            .enumerate()
        {
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

    /// Whether any shape annotation exists (drives the shape "clear" button).
    ///
    /// Derived from the unified list so a new annotation type is covered
    /// automatically via [`Annotation::is_shape`].
    pub fn has_shapes(&self) -> bool {
        self.annotations
            .iter()
            .take(self.annotation_index)
            .any(Annotation::is_shape)
    }

    /// Whether any redaction annotation exists (drives the redact "clear" button).
    pub fn has_redactions(&self) -> bool {
        self.annotations
            .iter()
            .take(self.annotation_index)
            .any(Annotation::is_redaction)
    }

    /// Finish the in-progress text edit, keeping the label.
    ///
    /// Self-contained (the pending edit carries its own styling) so *any* code
    /// path that ends an edit — Escape, opening a popup, switching tools,
    /// capturing — saves the text instead of throwing it away. Blank text is
    /// dropped, and re-edits replace the original in place.
    pub fn commit_text_editing(&mut self) {
        let Some(editing) = self.text_editing.take() else {
            return;
        };
        let keep = !editing.content.trim().is_empty();

        if let Some(unified_idx) = editing.replacing {
            if keep {
                let updated = editing.into_annotation();
                if let Some(slot) = self.annotations.get_mut(unified_idx) {
                    *slot = Annotation::Text(updated);
                }
            } else {
                // Emptying an existing label deletes it.
                if unified_idx < self.annotations.len() {
                    self.annotations.remove(unified_idx);
                    self.annotation_index = self.annotation_index.saturating_sub(1);
                }
                self.selected_text = None;
            }
            self.rebuild_arrays();
            return;
        }

        if keep {
            self.add(Annotation::Text(editing.into_annotation()));
            self.rebuild_arrays();
        }
    }

    /// Whether there is annotation work that would be lost by exiting.
    ///
    /// Covers committed annotations, a partially drawn shape, and a half-typed
    /// text label — anything the user would be surprised to lose to a stray
    /// Escape press.
    pub fn has_unsaved_work(&self) -> bool {
        self.annotation_index > 0
            || self.text_editing.is_some()
            || self.arrow_drawing.is_some()
            || self.line_drawing.is_some()
            || self.circle_drawing.is_some()
            || self.rect_outline_drawing.is_some()
            || self.pencil_drawing.is_some()
            || self.magnifier_drawing.is_some()
            || self.redact_drawing.is_some()
            || self.pixelate_drawing.is_some()
    }

    /// Whether any annotation drawing mode is currently active.
    ///
    /// Used to suppress selection-rectangle dragging while a tool owns the
    /// mouse. Keep this in sync when adding a new annotation tool.
    pub fn any_mode_active(&self) -> bool {
        self.arrow_mode
            || self.line_mode
            || self.circle_mode
            || self.rect_outline_mode
            || self.pencil_mode
            || self.text_mode
            || self.magnifier_mode
            || self.redact_mode
            || self.pixelate_mode
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
        self.line_mode = false;
        self.line_drawing = None;
        self.pencil_mode = false;
        self.pencil_drawing = None;
        self.text_mode = false;
        // Save, don't discard: this runs whenever a popup opens or the tool
        // changes, and losing a just-typed label there is surprising.
        self.commit_text_editing();
        self.magnifier_mode = false;
        self.magnifier_drawing = None;
        // Note: `selected_magnifier` and `selected_text` are intentionally
        // preserved here so the right-click config popup (which disables modes)
        // can still edit the selected item — that's the whole point of selecting
        // it. They are cleared when switching to another tool.
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
    /// Stroke thickness (logical units) used for new shape annotations
    pub shape_thickness: f32,
    /// Font size (logical units) used for new text annotations
    pub text_font_size: f32,
    /// Whether a second Escape press will discard the screenshot.
    ///
    /// Armed by the first Escape when annotation work would be lost, so an
    /// accidental press can't throw away a whole annotated capture.
    pub exit_armed: bool,
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
    /// User-rebindable key that copies the selection to the clipboard
    pub copy_shortcut: KeyBinding,
    /// Whether the settings drawer is waiting for the next key press to become
    /// the new copy shortcut. While set, keys are captured instead of acted on.
    pub capturing_copy_shortcut: bool,
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
        // The rebinding prompt swallows every key press, so it must never
        // outlive the drawer that shows it — otherwise the keyboard goes dead
        // with nothing on screen explaining why.
        self.capturing_copy_shortcut = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ArrowAnnotation, MagnifierAnnotation, SHAPE_THICKNESS_DEFAULT};

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
            thickness: SHAPE_THICKNESS_DEFAULT,
        }
    }

    fn line() -> LineAnnotation {
        LineAnnotation {
            start_x: 1.0,
            start_y: 2.0,
            end_x: 30.0,
            end_y: 40.0,
            color: ShapeColor::default(),
            shadow: true,
            thickness: SHAPE_THICKNESS_DEFAULT,
        }
    }

    fn pencil() -> PencilAnnotation {
        PencilAnnotation {
            points: vec![(0.0, 0.0), (5.0, 5.0), (10.0, 2.0)],
            color: ShapeColor::default(),
            shadow: true,
            thickness: SHAPE_THICKNESS_DEFAULT,
        }
    }

    #[test]
    fn rebuild_arrays_restores_lines_and_pencils() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Line(line()));
        st.add(Annotation::Pencil(pencil()));
        st.rebuild_arrays();

        assert_eq!(st.lines.len(), 1);
        assert_eq!(st.pencils.len(), 1);
        assert_eq!(st.pencils[0].points.len(), 3);
    }

    #[test]
    fn undo_redo_round_trips_lines_and_pencils() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Line(line()));
        st.add(Annotation::Pencil(pencil()));
        st.rebuild_arrays();

        st.undo(); // drop the pencil
        assert_eq!(st.lines.len(), 1);
        assert_eq!(st.pencils.len(), 0);

        st.undo(); // drop the line
        assert_eq!(st.lines.len(), 0);

        st.redo();
        st.redo();
        assert_eq!(st.lines.len(), 1);
        assert_eq!(st.pencils.len(), 1);
    }

    #[test]
    fn line_and_pencil_count_as_shapes_not_redactions() {
        assert!(Annotation::Line(line()).is_shape());
        assert!(Annotation::Pencil(pencil()).is_shape());
        assert!(!Annotation::Line(line()).is_redaction());
        assert!(!Annotation::Pencil(pencil()).is_redaction());

        let mut st = AnnotationState::default();
        st.add(Annotation::Line(line()));
        st.add(Annotation::Pencil(pencil()));
        assert!(st.has_shapes());
        assert!(!st.has_redactions());

        // Clearing shapes must remove both new types.
        st.clear_shapes();
        assert!(!st.has_shapes());
        assert!(st.lines.is_empty());
        assert!(st.pencils.is_empty());
    }

    #[test]
    fn clear_redactions_keeps_lines_and_pencils() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Line(line()));
        st.add(Annotation::Pencil(pencil()));
        st.add(Annotation::Redact(RedactAnnotation {
            x: 0.0,
            y: 0.0,
            x2: 5.0,
            y2: 5.0,
        }));
        st.clear_redactions();
        st.rebuild_arrays();

        assert_eq!(st.lines.len(), 1);
        assert_eq!(st.pencils.len(), 1);
        assert!(st.redactions.is_empty());
    }

    #[test]
    fn single_point_pencil_stroke_is_invalid() {
        // A click without a drag produces one point and nothing visible.
        let single = PencilAnnotation {
            points: vec![(1.0, 1.0)],
            color: ShapeColor::default(),
            shadow: false,
            thickness: SHAPE_THICKNESS_DEFAULT,
        };
        assert!(!single.is_valid());
        assert!(pencil().is_valid());
    }

    fn editing(content: &str) -> TextEditing {
        TextEditing {
            x: 40.0,
            y: 50.0,
            content: content.to_string(),
            font_size: 24.0,
            color: ShapeColor::default(),
            shadow: true,
            replacing: None,
        }
    }

    fn text_ann(content: &str) -> TextAnnotation {
        TextAnnotation {
            x: 40.0,
            y: 50.0,
            content: content.to_string(),
            font_size: 24.0,
            color: ShapeColor::default(),
            shadow: true,
        }
    }

    #[test]
    fn text_survives_undo_redo_and_rebuild() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("hello")));
        st.add(Annotation::Line(line()));
        st.rebuild_arrays();
        assert_eq!(st.texts.len(), 1);
        assert_eq!(st.texts[0].content, "hello");

        st.undo(); // drop the line
        assert_eq!(st.texts.len(), 1);
        st.undo(); // drop the text
        assert!(st.texts.is_empty());
        st.redo();
        assert_eq!(st.texts.len(), 1);
    }

    #[test]
    fn blank_text_is_not_worth_keeping() {
        assert!(!text_ann("").is_valid());
        assert!(!text_ann("   ").is_valid());
        assert!(!text_ann("\n\t ").is_valid());
        assert!(text_ann("hi").is_valid());
    }

    #[test]
    fn text_counts_as_a_shape_and_is_cleared_with_shapes() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("note")));
        assert!(Annotation::Text(text_ann("note")).is_shape());
        assert!(!Annotation::Text(text_ann("note")).is_redaction());
        assert!(st.has_shapes());

        st.clear_shapes();
        assert!(st.texts.is_empty());
        assert!(!st.has_shapes());
    }

    #[test]
    fn clear_redactions_keeps_text() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("keep me")));
        st.add(Annotation::Redact(RedactAnnotation {
            x: 0.0,
            y: 0.0,
            x2: 5.0,
            y2: 5.0,
        }));
        st.clear_redactions();
        st.rebuild_arrays();
        assert_eq!(st.texts.len(), 1);
        assert!(st.redactions.is_empty());
    }

    #[test]
    fn clearing_all_drops_the_in_progress_text_edit() {
        let mut st = AnnotationState::default();
        st.text_mode = true;
        st.text_editing = Some(editing("half typed"));
        st.clear_all();
        assert!(st.text_editing.is_none());
        assert!(!st.text_mode);
    }

    #[test]
    fn opening_a_popup_saves_the_label_being_typed() {
        // Regression: right-clicking the annotation button calls
        // disable_all_modes(), which used to throw away the pending label.
        let mut st = AnnotationState::default();
        st.text_mode = true;
        st.text_editing = Some(editing("hello"));

        st.disable_all_modes();

        assert!(st.text_editing.is_none(), "the edit should be finished");
        assert_eq!(st.texts.len(), 1, "and the label kept");
        assert_eq!(st.texts[0].content, "hello");
    }

    #[test]
    fn thickness_is_stored_per_annotation() {
        // Each shape keeps the thickness it was drawn with, so changing the
        // slider later must not restyle existing shapes.
        let mut thin = line();
        thin.thickness = 2.0;
        let mut thick = line();
        thick.thickness = 10.0;

        let mut st = AnnotationState::default();
        st.add(Annotation::Line(thin));
        st.add(Annotation::Line(thick));
        st.rebuild_arrays();

        assert_eq!(st.lines[0].thickness, 2.0);
        assert_eq!(st.lines[1].thickness, 10.0);
    }

    #[test]
    fn thickness_survives_undo_redo() {
        let mut wide = line();
        wide.thickness = 9.0;
        let mut st = AnnotationState::default();
        st.add(Annotation::Line(wide));
        st.rebuild_arrays();

        st.undo();
        st.redo();
        assert_eq!(st.lines[0].thickness, 9.0);
    }

    #[test]
    fn opening_a_popup_keeps_the_text_selection() {
        // Regression: the config popup calls disable_all_modes(); clearing the
        // selection there made it impossible to select a label and then change
        // its font size or colour from the popup.
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("pick me")));
        st.rebuild_arrays();
        st.text_mode = true;
        st.selected_text = Some(0);

        st.disable_all_modes();

        assert_eq!(
            st.selected_text,
            Some(0),
            "selection must survive so the popup can edit it"
        );

        // ...and editing through the popup now reaches the label.
        st.edit_selected_text(|t| t.font_size = 64.0);
        assert_eq!(st.texts[0].font_size, 64.0);
    }

    #[test]
    fn committing_keeps_the_styling_chosen_when_the_edit_began() {
        // The pending edit carries its own styling so no code path needs the UI
        // state to save it.
        let mut st = AnnotationState::default();
        let mut e = editing("styled");
        e.font_size = 40.0;
        e.shadow = false;
        st.text_editing = Some(e);

        st.commit_text_editing();
        assert_eq!(st.texts[0].font_size, 40.0);
        assert!(!st.texts[0].shadow);
    }

    #[test]
    fn committing_blank_text_adds_nothing() {
        let mut st = AnnotationState::default();
        st.text_editing = Some(editing("   "));
        st.commit_text_editing();
        assert!(st.texts.is_empty());
        assert_eq!(st.annotation_index, 0);
    }

    #[test]
    fn re_editing_replaces_in_place_instead_of_duplicating() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("before")));
        st.add(Annotation::Line(line()));
        st.rebuild_arrays();

        let unified = st.unified_index_of_text(0).expect("text is present");
        let mut e = editing("after");
        e.replacing = Some(unified);
        st.text_editing = Some(e);
        st.commit_text_editing();

        assert_eq!(st.texts.len(), 1, "must not duplicate the label");
        assert_eq!(st.texts[0].content, "after");
        // The line after it is untouched, so z-order is preserved.
        assert_eq!(st.lines.len(), 1);
        assert_eq!(st.annotations.len(), 2);
    }

    #[test]
    fn emptying_an_existing_label_deletes_it() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("doomed")));
        st.rebuild_arrays();

        let unified = st.unified_index_of_text(0).unwrap();
        let mut e = editing("");
        e.replacing = Some(unified);
        st.text_editing = Some(e);
        st.commit_text_editing();

        assert!(st.texts.is_empty());
        assert!(st.annotations.is_empty());
        assert_eq!(st.annotation_index, 0);
        assert!(st.selected_text.is_none());
    }

    #[test]
    fn committed_text_is_undoable() {
        // Committing goes through the normal history, so Escape-then-undo works.
        let mut st = AnnotationState::default();
        st.text_editing = Some(editing("oops"));
        st.commit_text_editing();
        assert_eq!(st.texts.len(), 1);

        st.undo();
        assert!(st.texts.is_empty());
        st.redo();
        assert_eq!(st.texts.len(), 1);
    }

    #[test]
    fn has_unsaved_work_covers_committed_and_in_progress() {
        let mut st = AnnotationState::default();
        assert!(
            !st.has_unsaved_work(),
            "a clean session has nothing to lose"
        );

        // A committed annotation counts.
        st.add(Annotation::Text(text_ann("done")));
        assert!(st.has_unsaved_work());

        // So does a half-drawn shape with nothing committed.
        let mut st = AnnotationState::default();
        st.line_drawing = Some((1.0, 2.0));
        assert!(st.has_unsaved_work());

        // And a half-typed label.
        let mut st = AnnotationState::default();
        st.text_editing = Some(editing("typing"));
        assert!(st.has_unsaved_work());
    }

    #[test]
    fn undoing_everything_means_nothing_is_at_risk() {
        // After undoing back to the start, Escape shouldn't nag.
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("x")));
        st.undo();
        assert!(!st.has_unsaved_work());
    }

    #[test]
    fn unified_index_of_text_skips_other_annotations() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Line(line())); // 0
        st.add(Annotation::Text(text_ann("first"))); // 1
        st.add(Annotation::Pencil(pencil())); // 2
        st.add(Annotation::Text(text_ann("second"))); // 3
        st.rebuild_arrays();

        assert_eq!(st.unified_index_of_text(0), Some(1));
        assert_eq!(st.unified_index_of_text(1), Some(3));
        assert_eq!(st.unified_index_of_text(2), None);
    }

    #[test]
    fn edit_selected_text_targets_the_right_label() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("first")));
        st.add(Annotation::Line(line()));
        st.add(Annotation::Text(text_ann("second")));
        st.rebuild_arrays();

        st.selected_text = Some(1);
        st.edit_selected_text(|t| t.font_size = 40.0);

        assert_eq!(st.texts[0].font_size, 24.0, "first label must be untouched");
        assert_eq!(st.texts[1].font_size, 40.0);
    }

    #[test]
    fn edit_selected_text_moves_only_the_selection() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("a")));
        st.add(Annotation::Text(text_ann("b")));
        st.rebuild_arrays();

        st.selected_text = Some(0);
        st.edit_selected_text(|t| {
            t.x = 500.0;
            t.y = 600.0;
        });

        assert_eq!((st.texts[0].x, st.texts[0].y), (500.0, 600.0));
        assert_eq!((st.texts[1].x, st.texts[1].y), (40.0, 50.0));
    }

    #[test]
    fn text_selection_is_dropped_when_undo_removes_the_label() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("gone soon")));
        st.rebuild_arrays();
        st.selected_text = Some(0);

        st.undo();
        assert!(
            st.selected_text.is_none(),
            "a stale selection would index out of bounds"
        );
    }

    #[test]
    fn edit_selected_text_is_a_no_op_without_a_selection() {
        let mut st = AnnotationState::default();
        st.add(Annotation::Text(text_ann("untouched")));
        st.rebuild_arrays();
        st.selected_text = None;
        st.edit_selected_text(|t| t.font_size = 99.0);
        assert_eq!(st.texts[0].font_size, 24.0);
    }

    #[test]
    fn any_mode_active_tracks_text() {
        let mut st = AnnotationState::default();
        assert!(!st.any_mode_active());
        st.text_mode = true;
        assert!(st.any_mode_active());
    }

    #[test]
    fn any_mode_active_tracks_line_and_pencil() {
        let mut st = AnnotationState::default();
        assert!(!st.any_mode_active());

        st.line_mode = true;
        assert!(st.any_mode_active());
        st.line_mode = false;

        st.pencil_mode = true;
        assert!(st.any_mode_active());
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
