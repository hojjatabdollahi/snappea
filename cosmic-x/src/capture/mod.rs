// SPDX-License-Identifier: GPL-3.0-only

//! State of a capture in progress and the handlers that drive it.

pub mod countdown;
pub mod detect;
pub mod flow;
pub mod msg;
pub mod shortcuts;
pub mod text;
pub mod update;
pub mod view;

use crate::capture::detect::DetectedQrCode;
use crate::capture::detect::OcrStatus;
use crate::capture::detect::OcrTextOverlay;
use crate::config::Container;
use crate::config::RedactTool;
use crate::config::SaveLocationChoice;
use crate::config::ShapeTool;
use crate::config::VideoSaveLocationChoice;
use crate::dbus::PortalResponse;
use crate::dbus::ScreenshotResult;
use crate::geometry::Choice;
use crate::geometry::ImageSaveLocation;
use crate::recording::encoder::EncoderInfo;
use crate::wayland::ShmImage;
use cosmic::iced::Animation;
use cosmic::iced::animation::Easing;
use cosmic::iced::core::Point;
use cosmic::iced::core::Rectangle;
use cosmic::iced::core::Vector;
use image::RgbaImage;
use rustix::fd::AsFd;
use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::mpsc::Sender;
use viewer_tools::OperationStack;
use viewer_tools::ToolOperation;
use viewer_tools::annotate::AnnotateColor;
use viewer_tools::annotate::MagnifierOperation;
use viewer_tools::annotate::PixelateOperation;
use viewer_tools::annotate::RedactOperation;
use viewer_tools::annotate::ShapeKind;
use viewer_tools::annotate::{HighlighterOperation, PenOperation, ShapeOperation, TextOperation};

#[derive(Clone, Debug)]
pub struct PortalContext {
    pub tx: Sender<PortalResponse<ScreenshotResult>>,
    /// Whether a portal caller is waiting on `tx`. If so a file is always written,
    /// even for "copy to clipboard", so the caller gets a usable URI.
    pub expects_response: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DetectionState {
    pub qr_codes: Vec<DetectedQrCode>,
    pub qr_scanning: bool,
    pub ocr_status: OcrStatus,
    pub ocr_overlays: Vec<OcrTextOverlay>,
    pub ocr_text: Option<String>,
    /// Bumped whenever results are cleared. An OCR run carries the generation
    /// it started under, and its result is dropped if that has moved on.
    pub ocr_generation: u64,
}

impl DetectionState {
    pub fn clear(&mut self) {
        self.ocr_generation += 1;
        self.ocr_status = OcrStatus::Idle;
        self.ocr_text = None;
        self.ocr_overlays.clear();
        self.qr_codes.clear();
        self.qr_scanning = false;
    }
}

#[derive(Clone, Debug, Default)]
pub struct AnnotationState {
    /// The picked operation, an index into `stack`. Anything that adds,
    /// removes or reorders operations clears it.
    pub selected: Option<usize>,
    /// Everything drawn on the capture, with its undo history.
    pub stack: OperationStack,
    /// Which shape a click would draw, if any.
    pub shape_mode: Option<ShapeKind>,
    /// Where the shape being drawn started, in global logical coordinates.
    pub shape_drawing: Option<(f32, f32)>,
    pub redact_mode: bool,
    pub redact_drawing: Option<(f32, f32)>,
    pub pixelate_mode: bool,
    pub pixelate_drawing: Option<(f32, f32)>,
    pub pen_mode: bool,
    pub text_mode: bool,
    /// The highlighter shares the pen's drawing path. Only the committed
    /// operation differs.
    pub highlighter_mode: bool,
    /// Points collected so far for the in-progress freehand stroke.
    pub stroke_drawing: Option<Vec<(f32, f32)>>,
    pub magnifier_mode: bool,
    pub magnifier_drawing: Option<(f32, f32)>,
    /// The magnifier being adjusted, an index into `stack`.
    pub selected_magnifier: Option<usize>,
}

/// Whether an operation hides content instead of marking it up.
pub fn is_redaction(op: &dyn ToolOperation) -> bool {
    let any = op.as_any();
    any.is::<RedactOperation>() || any.is::<PixelateOperation>()
}

/// The tool a picked annotation was made with and the settings it carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Picked {
    pub tool: ShapeTool,
    pub color: Option<AnnotateColor>,
    /// Stroke width, block size or zoom, by tool.
    pub size: Option<f32>,
}

impl AnnotationState {
    pub const fn clear_selection(&mut self) {
        self.selected = None;
    }

    #[must_use]
    pub fn selected_annotation(&self) -> Option<&dyn ToolOperation> {
        self.stack.get(self.selected?)
    }

    /// The picked annotation's tool and the settings it was drawn with, so the
    /// toolbar can show and edit them.
    #[must_use]
    pub fn picked(&self) -> Option<Picked> {
        let op = self.selected_annotation()?;
        let any = op.as_any();
        Some(if let Some(s) = any.downcast_ref::<ShapeOperation>() {
            Picked {
                tool: ShapeTool::SHAPES
                    .iter()
                    .copied()
                    .find(|t| t.shape_kind() == Some(s.kind))?,
                color: Some(AnnotateColor(s.color)),
                size: Some(s.width),
            }
        } else if let Some(p) = any.downcast_ref::<PenOperation>() {
            Picked {
                tool: ShapeTool::Pen,
                color: Some(AnnotateColor(p.color)),
                size: Some(p.width),
            }
        } else if let Some(h) = any.downcast_ref::<HighlighterOperation>() {
            Picked {
                tool: ShapeTool::Highlighter,
                color: Some(AnnotateColor(h.color)),
                size: Some(h.width),
            }
        } else if let Some(px) = any.downcast_ref::<PixelateOperation>() {
            Picked {
                tool: ShapeTool::Pixelate,
                color: None,
                size: Some(px.block_size),
            }
        } else if let Some(m) = any.downcast_ref::<MagnifierOperation>() {
            Picked {
                tool: ShapeTool::Magnifier,
                color: Some(AnnotateColor(m.color)),
                size: Some(m.magnification),
            }
        } else {
            let text = any.downcast_ref::<TextOperation>()?;
            // Its size is laid out with the text. Only the color is edited here.
            Picked {
                tool: ShapeTool::Text,
                color: Some(AnnotateColor(text.color)),
                size: None,
            }
        })
    }

    /// Edit the picked annotation if it is a `T`.
    pub fn edit_selected<T: 'static>(&mut self, f: impl FnOnce(&mut T)) {
        if let Some(op) = self
            .selected
            .and_then(|i| self.stack.get_mut(i))
            .and_then(|op| op.as_any_mut().downcast_mut::<T>())
        {
            f(op);
        }
    }

    /// The picked annotation, for editing in place.
    fn selected_annotation_mut(&mut self) -> Option<&mut (dyn ToolOperation + 'static)> {
        self.stack.get_mut(self.selected?)
    }

    /// Give the picked annotation a new color, whatever its tool.
    pub fn recolor_selected(&mut self, color: AnnotateColor) {
        if let Some(op) = self.selected_annotation_mut() {
            op.set_color(color.0);
        }
    }

    /// Give the picked annotation a new size: stroke width, block size or zoom,
    /// whichever its tool calls it. Returns whether anything was picked.
    pub fn resize_selected(&mut self, size: f32) -> bool {
        let Some(picked) = self.picked() else {
            return false;
        };
        match picked.tool {
            ShapeTool::Pixelate => self.edit_selected::<PixelateOperation>(|p| p.block_size = size),
            ShapeTool::Magnifier => {
                self.edit_selected::<MagnifierOperation>(|m| m.magnification = size);
            }
            ShapeTool::Text => return false,
            _ => {
                if let Some(op) = self.selected_annotation_mut() {
                    op.set_annotation_stroke(size);
                }
            }
        }
        true
    }

    pub fn clear_all(&mut self) {
        self.stack.clear();
        self.selected = None;
        self.selected_magnifier = None;
        self.disable_all_modes();
    }

    pub fn clear_shapes(&mut self) {
        self.stack.retain(is_redaction);
        self.selected = None;
        self.selected_magnifier = None;
        self.shape_drawing = None;
        self.shape_mode = None;
        self.stroke_drawing = None;
        self.pen_mode = false;
        self.text_mode = false;
        self.highlighter_mode = false;
        self.magnifier_drawing = None;
        self.magnifier_mode = false;
    }

    pub fn clear_redactions(&mut self) {
        self.stack.retain(|op| !is_redaction(op));
        self.selected = None;
        self.selected_magnifier = None;
        self.redact_mode = false;
        self.redact_drawing = None;
        self.pixelate_mode = false;
        self.pixelate_drawing = None;
    }

    pub fn undo(&mut self) {
        self.stack.undo();
        self.forget_stale_magnifier();
    }

    pub fn redo(&mut self) {
        self.stack.redo();
        self.forget_stale_magnifier();
    }

    fn forget_stale_magnifier(&mut self) {
        if self
            .selected_magnifier
            .is_some_and(|i| self.magnifier(i).is_none())
        {
            self.selected_magnifier = None;
        }
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.stack.can_undo()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.stack.can_redo()
    }

    /// Commit an operation. The new one is never the picked one.
    pub fn commit(&mut self, op: Box<dyn ToolOperation>) {
        self.selected = None;
        self.stack.commit(op);
    }

    /// The live operations, in draw order.
    #[must_use]
    pub fn operations(&self) -> &[Box<dyn ToolOperation>] {
        self.stack.operations()
    }

    #[must_use]
    pub fn magnifier(&self, index: usize) -> Option<&MagnifierOperation> {
        self.stack.get(index)?.as_any().downcast_ref()
    }

    /// Every magnifier with its index in the stack.
    pub fn magnifiers(&self) -> impl DoubleEndedIterator<Item = (usize, &MagnifierOperation)> {
        self.stack
            .operations()
            .iter()
            .enumerate()
            .filter_map(|(i, op)| Some((i, op.as_any().downcast_ref()?)))
    }

    #[must_use]
    pub fn selected_magnifier_zoom(&self) -> Option<f32> {
        self.magnifier(self.selected_magnifier?)
            .map(|m| m.magnification)
    }

    pub fn edit_selected_magnifier(&mut self, f: impl FnOnce(&mut MagnifierOperation)) {
        let Some(index) = self.selected_magnifier else {
            return;
        };
        if let Some(m) = self
            .stack
            .get_mut(index)
            .and_then(|op| op.as_any_mut().downcast_mut())
        {
            f(m);
        }
    }

    /// Whether anything other than a redaction has been drawn.
    #[must_use]
    pub fn has_shapes(&self) -> bool {
        self.operations()
            .iter()
            .any(|op| !is_redaction(op.as_ref()))
    }

    #[must_use]
    pub fn has_redactions(&self) -> bool {
        self.operations().iter().any(|op| is_redaction(op.as_ref()))
    }

    /// Whether there is annotation work that would be lost by exiting.
    #[must_use]
    pub fn has_unsaved_work(&self) -> bool {
        self.stack.can_undo()
            || self.shape_drawing.is_some()
            || self.stroke_drawing.is_some()
            || self.magnifier_drawing.is_some()
            || self.redact_drawing.is_some()
            || self.pixelate_drawing.is_some()
    }

    /// Whether any drawing tool owns the mouse.
    #[must_use]
    pub const fn any_mode_active(&self) -> bool {
        self.shape_mode.is_some()
            || self.text_mode
            || self.pen_mode
            || self.highlighter_mode
            || self.magnifier_mode
            || self.redact_mode
            || self.pixelate_mode
    }

    pub fn disable_all_modes(&mut self) {
        self.shape_mode = None;
        self.shape_drawing = None;
        self.redact_mode = false;
        self.redact_drawing = None;
        self.pixelate_mode = false;
        self.pixelate_drawing = None;
        self.pen_mode = false;
        self.highlighter_mode = false;
        self.stroke_drawing = None;
        self.text_mode = false;
        self.magnifier_mode = false;
        self.magnifier_drawing = None;
        // `selected_magnifier` stays: the zoom dropdown disables the modes and
        // still needs to know which magnifier it is editing.
    }
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub choice: Choice,
    pub location: ImageSaveLocation,
    pub focused_output_index: usize,
    pub also_copy_to_clipboard: bool,
    /// Whether the mouse has entered any output yet
    pub has_mouse_entered: bool,
}

/// Progress of each animated toolbar section. Kept on the capture so the
/// row is built with its widths already known.
#[derive(Clone, Debug)]
pub struct ToolbarAnim {
    /// Screenshot/video mode pair
    pub capture_mode: Animation<f32>,
    /// Region, screen and all-screens picker
    pub target: Animation<f32>,
    /// Annotate button, which toggles the drawing tools
    pub tools: Animation<f32>,
    /// The drawing tools, their dropdowns and the swatches
    pub annotate_row: Animation<f32>,
    /// Capture delay
    pub delay: Animation<f32>,
    /// Delete button, present only while an annotation is picked out
    pub delete: Animation<f32>,
    /// Recognize-text button
    pub ocr: Animation<f32>,
}

/// How long a section takes to open or close.
const TOOLBAR_ANIM: Duration = Duration::from_millis(180);

impl ToolbarAnim {
    fn section(shown: bool) -> Animation<f32> {
        Animation::new(if shown { 1.0 } else { 0.0 })
            .easing(Easing::EaseOut)
            .duration(TOOLBAR_ANIM)
    }

    /// Start in a mode without animating into it.
    #[must_use]
    pub fn settled(shown: ToolbarSections) -> Self {
        Self {
            capture_mode: Self::section(shown.capture_mode),
            target: Self::section(shown.target),
            tools: Self::section(shown.tools),
            annotate_row: Self::section(shown.annotate_row),
            delay: Self::section(shown.delay),
            delete: Self::section(shown.delete),
            ocr: Self::section(shown.ocr),
        }
    }

    /// Retarget every section for the current mode. A no-op when nothing changed.
    pub fn sync(&mut self, shown: ToolbarSections, now: Instant) {
        for (anim, target) in [
            (&mut self.capture_mode, shown.capture_mode),
            (&mut self.target, shown.target),
            (&mut self.tools, shown.tools),
            (&mut self.annotate_row, shown.annotate_row),
            (&mut self.delay, shown.delay),
            (&mut self.delete, shown.delete),
            (&mut self.ocr, shown.ocr),
        ] {
            let target = if target { 1.0 } else { 0.0 };
            if anim.value() != target {
                anim.go_mut(target, now);
            }
        }
    }

    /// Read every section's progress at `now`.
    #[must_use]
    pub fn progress(&self, now: Instant) -> ToolbarProgress {
        let at = |anim: &Animation<f32>| anim.interpolate_with(|v| v, now);
        ToolbarProgress {
            capture_mode: at(&self.capture_mode),
            target: at(&self.target),
            tools: at(&self.tools),
            annotate_row: at(&self.annotate_row),
            delay: at(&self.delay),
            delete: at(&self.delete),
            ocr: at(&self.ocr),
        }
    }

    /// Whether anything is still moving, and so whether a redraw is owed.
    #[must_use]
    pub fn is_animating(&self, now: Instant) -> bool {
        self.capture_mode.is_animating(now)
            || self.target.is_animating(now)
            || self.tools.is_animating(now)
            || self.annotate_row.is_animating(now)
            || self.delay.is_animating(now)
            || self.delete.is_animating(now)
            || self.ocr.is_animating(now)
    }
}

/// How far each section is open at one instant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToolbarProgress {
    pub capture_mode: f32,
    pub target: f32,
    pub tools: f32,
    pub annotate_row: f32,
    pub delay: f32,
    /// The delete button, which is there only while something is picked out.
    pub delete: f32,
    /// The recognize-text button.
    pub ocr: f32,
}

/// Which sections a mode shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarSections {
    pub capture_mode: bool,
    pub target: bool,
    pub tools: bool,
    pub annotate_row: bool,
    pub delay: bool,
    pub delete: bool,
    pub ocr: bool,
}

/// What the toolbar is currently doing, as far as which sections it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarMode {
    pub annotating: bool,
    pub video: bool,
    pub has_selection: bool,
    /// Whether the cursor tool has an annotation picked out, and so whether
    /// there is anything for the delete button to delete.
    pub annotation_selected: bool,
    /// Whether there is a region to read text out of, and something to read it
    /// with.
    pub can_ocr: bool,
}

impl ToolbarSections {
    /// Work out the sections from what the toolbar is currently doing.
    #[must_use]
    pub const fn of(mode: ToolbarMode) -> Self {
        let ToolbarMode {
            annotating,
            video,
            has_selection,
            annotation_selected,
            can_ocr,
        } = mode;

        Self {
            // Annotating replaces the middle of the toolbar. The capture mode and
            // target choices are hidden until it is done.
            capture_mode: !annotating,
            target: !annotating,
            // The annotate button stays while annotating.
            tools: has_selection && !video,
            annotate_row: annotating,
            // Screenshots only, and not while annotating. Stays visible while armed so it can be disarmed.
            delay: !video && !annotating,
            // Delete is part of the cursor tool, so it shows only with the
            // annotation row and only while something is picked.
            delete: annotating && annotation_selected,
            // OCR needs a dragged region and tesseract.
            ocr: can_ocr && !video && !annotating,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub now: Instant,
    /// How far each varying toolbar section is open.
    pub toolbar_anim: ToolbarAnim,
    /// Top-left of the floating toolbar per output, in output-local logical
    /// coordinates. Missing outputs center it along the bottom. Not persisted.
    pub toolbar_pos: HashMap<String, Point>,
    /// Whether the grip is currently held.
    pub toolbar_dragging: bool,
    /// Cursor offset within the toolbar when the drag began, resolved on the first motion.
    pub toolbar_drag_offset: Option<Vector>,
    pub settings_drawer_open: bool,
    /// Whether clicking picks annotations up instead of drawing. Excludes every drawing tool.
    pub move_mode: bool,
    /// Whether the toolbar is showing its annotation tools.
    pub annotate_mode: bool,
    /// Capture delay dropdown
    pub delay_popup_open: bool,
    /// Stroke width dropdown
    pub stroke_popup_open: bool,
    /// Font size dropdown, shown in place of stroke width for the text tool
    pub font_popup_open: bool,
    pub primary_shape_tool: ShapeTool,
    /// Which shape and which freehand tool their dropdowns are set to.
    pub shape_choice: ShapeTool,
    pub freehand_choice: ShapeTool,
    pub shape_popup_open: bool,
    /// Pen/highlighter dropdown
    pub freehand_popup_open: bool,
    /// Format for the next text label, and for the one being typed.
    pub text_format: viewer_tools::annotate::TextFormat,
    /// The font/size/style popup
    pub text_format_popup_open: bool,
    /// Which video setting is showing its options, if any
    pub shape_color: AnnotateColor,
    /// Stroke thickness (logical units) used for new shape annotations
    pub shape_thickness: f32,
    /// Highlighter thickness, separate from `shape_thickness`.
    pub highlighter_thickness: f32,
    /// Font size (logical units) used for new text annotations
    pub text_font_size: f32,
    /// Whether a second Escape will discard the screenshot. Armed when work would be lost.
    pub exit_armed: bool,
    pub primary_redact_tool: RedactTool,
    pub redact_popup_open: bool,
    pub pixelation_block_size: u32,
    /// Scan a selection for QR codes as soon as it is made
    pub recognize_qr_codes: bool,
    /// Whether `tesseract` is installed, checked once per session.
    pub tesseract_available: bool,
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
    // Recording settings
    pub available_encoders: Vec<EncoderInfo>,
    pub selected_encoder: Option<String>,
    pub video_container: Container,
    pub video_framerate: u32,
    pub video_show_cursor: bool,
    /// Whether a screenshot includes the pointer.
    pub show_cursor: bool,
    /// Whether video mode is selected (false = screenshot, true = video)
    pub is_video_mode: bool,
    /// Whether recording is currently active
    pub is_recording: bool,
    /// Whether annotation mode is active during recording
    pub recording_annotation_mode: bool,
    /// Whether pencil popup is open during recording
    pub pencil_popup_open: bool,
    /// Pencil color for recording annotations
    pub pencil_color: AnnotateColor,
    /// Duration in seconds before pencil strokes fade away
    pub pencil_fade_duration: f32,
    /// Pencil line thickness in pixels
    pub pencil_thickness: f32,
    /// Last known toolbar bounds (output-local)
    pub toolbar_bounds: Option<Rectangle>,
    /// Move offset for dragging selection rectangle (cursor pos relative to rect top-left when move started)
    pub move_offset: Option<(i32, i32)>,
    /// Whether cosmic-x is currently set as the default screenshot portal for the current user
    pub is_default_portal: bool,
}

impl UiState {
    pub const fn close_all_popups(&mut self) {
        self.shape_popup_open = false;
        self.freehand_popup_open = false;
        self.text_format_popup_open = false;
        self.delay_popup_open = false;
        self.stroke_popup_open = false;
        self.font_popup_open = false;
        self.redact_popup_open = false;
        self.magnifier_popup_open = false;
        self.settings_drawer_open = false;
        self.pencil_popup_open = false;
    }
}

#[cfg(test)]
mod toolbar_anim_tests {
    use super::*;

    /// A plain screenshot with no annotation, recording or delay.
    fn mode(has_selection: bool) -> ToolbarMode {
        ToolbarMode {
            annotating: false,
            video: false,
            has_selection,
            annotation_selected: false,
            can_ocr: false,
        }
    }

    #[test]
    fn disable_modes_clears_text() {
        // The move tool must also disarm the text tool, or it keeps swallowing clicks.
        let mut a = AnnotationState {
            text_mode: true,
            pen_mode: true,
            shape_mode: Some(viewer_tools::annotate::ShapeKind::Arrow),
            ..AnnotationState::default()
        };
        assert!(a.any_mode_active());
        a.disable_all_modes();
        assert!(!a.text_mode, "the text tool is a tool like the others");
        assert!(!a.any_mode_active());
    }

    #[test]
    fn no_delay_shows_targets() {
        let s = ToolbarSections::of(mode(false));
        assert!(s.capture_mode);
        assert!(s.target);
    }

    #[test]
    fn annotate_button_needs_selection() {
        // The tools act on a region. Without one the section is drawn over
        // nothing, so losing the selection closes it.
        let annotating_without = ToolbarSections::of(ToolbarMode {
            annotating: true,
            ..mode(false)
        });
        assert!(!annotating_without.tools, "no target, no tools");
    }

    #[test]
    fn annotating_keeps_exit() {
        let s = ToolbarSections::of(ToolbarMode {
            annotating: true,
            ..mode(true)
        });
        assert!(!s.capture_mode);
        assert!(!s.target);
        assert!(!s.delay);
        // The annotate button lives in `tools` and is how annotating is left.
        assert!(s.tools);
        assert!(s.annotate_row);
    }

    #[test]
    fn video_hides_tools_and_delay() {
        let s = ToolbarSections::of(ToolbarMode {
            video: true,
            ..mode(true)
        });
        assert!(!s.tools);
        assert!(!s.delay);
        assert!(s.capture_mode);
        assert!(s.target);
    }

    #[test]
    fn tools_need_selection() {
        assert!(!ToolbarSections::of(mode(false)).tools);
        assert!(ToolbarSections::of(mode(true)).tools);
    }

    #[test]
    fn settled_toolbar_skips_reveal() {
        let now = Instant::now();
        let shown = ToolbarSections::of(mode(false));
        let anim = ToolbarAnim::settled(shown);
        assert!(!anim.is_animating(now));

        let p = anim.progress(now);
        assert_eq!(p.capture_mode, 1.0);
        assert_eq!(p.target, 1.0);
        // Nothing selected yet, so no tools, and no animation from nothing.
        assert_eq!(p.tools, 0.0);
    }

    #[test]
    fn mode_change_animates_sections() {
        let now = Instant::now();
        let mut anim = ToolbarAnim::settled(ToolbarSections::of(mode(false)));

        anim.sync(ToolbarSections::of(mode(false)), now);
        assert!(!anim.is_animating(now), "nothing changed, nothing moves");

        anim.sync(ToolbarSections::of(mode(true)), now);
        assert!(anim.tools.is_animating(now));
        assert!(
            !anim.target.is_animating(now),
            "unaffected sections stay put"
        );
    }

    /// OCR needs both a region to read and tesseract to read it with. Without
    /// tesseract the button would show and then fail.
    #[test]
    fn ocr_needs_region_and_tesseract() {
        let ready = ToolbarMode {
            can_ocr: true,
            ..mode(true)
        };
        assert!(ToolbarSections::of(ready).ocr);
        assert!(
            !ToolbarSections::of(ToolbarMode {
                can_ocr: false,
                ..ready
            })
            .ocr
        );
        assert!(
            !ToolbarSections::of(ToolbarMode {
                video: true,
                ..ready
            })
            .ocr,
            "a recording has no still to read"
        );
        assert!(
            !ToolbarSections::of(ToolbarMode {
                annotating: true,
                ..ready
            })
            .ocr,
            "the drawing tools take the middle of the toolbar"
        );
    }

    #[test]
    fn section_reaches_target() {
        let start = Instant::now();
        let mut anim = ToolbarAnim::settled(ToolbarSections::of(mode(false)));
        anim.sync(ToolbarSections::of(mode(true)), start);

        let done = start + TOOLBAR_ANIM + Duration::from_millis(1);
        assert!(!anim.is_animating(done));
        assert_eq!(anim.progress(done).tools, 1.0);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn picked_annotation_reports_and_takes_its_size_and_color() {
        use cosmic::iced::{Color, Point};
        let mut a = AnnotationState::default();
        a.commit(Box::new(ShapeOperation::new(
            ShapeKind::Arrow,
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Color::BLACK,
            2.0,
        )));
        a.commit(Box::new(PixelateOperation::new(
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            8.0,
        )));
        a.selected = Some(0);
        let picked = a.picked().unwrap();
        assert_eq!((picked.tool, picked.size), (ShapeTool::Arrow, Some(2.0)));
        assert!(a.resize_selected(5.0));
        a.recolor_selected(AnnotateColor(Color::WHITE));
        let picked = a.picked().unwrap();
        assert_eq!(picked.size, Some(5.0));
        assert_eq!(picked.color, Some(AnnotateColor(Color::WHITE)));

        a.selected = Some(1);
        assert_eq!(
            a.picked().map(|p| (p.tool, p.size)),
            Some((ShapeTool::Pixelate, Some(8.0)))
        );
        assert!(a.resize_selected(12.0));
        assert_eq!(a.picked().and_then(|p| p.size), Some(12.0));

        a.selected = None;
        assert!(!a.resize_selected(3.0));
    }
    use cosmic::iced::Color;
    use viewer_tools::annotate::MagnifierOperation;

    fn blob(x: f32) -> Box<dyn ToolOperation> {
        Box::new(MagnifierOperation::new(
            Point::new(x, 0.0),
            Point::new(x + 10.0, 10.0),
            2.0,
            Color::BLACK,
        ))
    }

    /// The pick is an index. Anything that reorders the list must drop it.
    #[test]
    fn commit_clears_selection() {
        let mut state = AnnotationState::default();
        state.commit(blob(0.0));
        state.commit(blob(20.0));
        state.selected = Some(0);

        state.commit(blob(40.0));
        assert_eq!(
            state.selected, None,
            "a new annotation is not the picked one"
        );
    }

    #[test]
    fn stale_selection_is_none() {
        let mut state = AnnotationState::default();
        state.commit(blob(0.0));
        state.selected = Some(0);
        assert!(state.selected_annotation().is_some());

        // A stale index resolves to nothing.
        state.selected = Some(7);
        assert!(state.selected_annotation().is_none());
    }

    #[test]
    fn clear_all_clears_selection() {
        let mut state = AnnotationState::default();
        state.commit(blob(0.0));
        state.selected = Some(0);
        state.clear_all();
        assert_eq!(state.selected, None);
    }

    /// The delete button is a toolbar section, shown only with the annotation row.
    #[test]
    fn delete_needs_selection() {
        let armed = ToolbarMode {
            annotating: true,
            video: false,
            has_selection: true,
            annotation_selected: true,
            can_ocr: false,
        };
        assert!(ToolbarSections::of(armed).delete);
        assert!(
            !ToolbarSections::of(ToolbarMode {
                annotation_selected: false,
                can_ocr: false,
                ..armed
            })
            .delete
        );
        assert!(
            !ToolbarSections::of(ToolbarMode {
                annotating: false,
                ..armed
            })
            .delete,
            "nothing to delete from when the tools are put away"
        );
    }
}

#[derive(Clone, Debug)]
pub struct Capture {
    pub portal: PortalContext,
    pub output_images: HashMap<String, ScreenshotImage>,
    pub detection: DetectionState,
    pub annotations: AnnotationState,
    pub selection: Selection,
    pub ui: UiState,
}

impl Capture {
    /// Clear all annotation state.
    pub fn clear_annotations(&mut self) {
        self.annotations.clear_all();
    }

    /// Put the annotation tools away: nothing armed, nothing half-drawn, no dropdown open.
    pub fn leave_annotate_mode(&mut self) {
        self.ui.annotate_mode = false;
        self.ui.move_mode = false;
        self.close_all_popups();
        self.disable_all_modes();
    }

    /// Clear only shape annotations (arrows, circles, rectangles) - NOT redactions
    pub fn clear_shapes(&mut self) {
        self.annotations.clear_shapes();
    }

    /// Clear OCR/QR state
    pub fn clear_ocr_qr(&mut self) {
        self.detection.clear();
    }

    /// Clear all transient state (annotations + OCR/QR)
    pub fn clear_transient_state(&mut self) {
        self.clear_annotations();
        self.clear_ocr_qr();
        self.close_all_popups();
    }

    /// Disable all drawing modes without clearing annotations
    pub fn disable_all_modes(&mut self) {
        self.annotations.disable_all_modes();
    }

    /// Close all open popups and drawers
    pub const fn close_all_popups(&mut self) {
        self.ui.close_all_popups();
    }
}

// A captured frame and its display handle.

/// A captured screenshot image with both raw RGBA data and a display handle
#[derive(Clone, Debug)]
pub struct ScreenshotImage {
    pub rgba: RgbaImage,
    pub handle: cosmic::widget::image::Handle,
}

impl ScreenshotImage {
    /// Create a new `ScreenshotImage` from a wayland `ShmImage`
    pub fn new<T: AsFd>(img: ShmImage<T>) -> anyhow::Result<Self> {
        let rgba = img.image_transformed()?;
        log::debug!(
            "ScreenshotImage captured: {}x{} pixels",
            rgba.width(),
            rgba.height()
        );
        let handle = cosmic::widget::image::Handle::from_rgba(
            rgba.width(),
            rgba.height(),
            rgba.clone().into_vec(),
        );
        Ok(Self { rgba, handle })
    }

    /// Get the width of the image
    pub fn width(&self) -> u32 {
        self.rgba.width()
    }

    /// Get the height of the image
    pub fn height(&self) -> u32 {
        self.rgba.height()
    }
}
