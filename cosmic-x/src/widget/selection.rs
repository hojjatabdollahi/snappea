// SPDX-License-Identifier: GPL-3.0-only

//! The selection widget.

use crate::app::OutputState;
use crate::capture::AnnotationState;
use crate::capture::DetectionState;
use crate::capture::ScreenshotImage;
use crate::capture::UiState;
use crate::capture::detect::DetectedQrCode;
use crate::capture::detect::OcrTextOverlay;
use crate::capture::msg::Msg;
use crate::capture::msg::TextAction;
use crate::fl;
use crate::geometry::Choice;
use crate::geometry::DragState;
use crate::geometry::Rect;
use crate::widget::drawing::draw_inactive_overlay_with_hint;
use crate::widget::drawing::draw_selection_frame_with_handles;
use crate::widget::output_selection::OutputSelection;
use crate::widget::overlays::magnifier_overlays::draw_magnifier_handles;
use crate::widget::overlays::status_overlays::draw_ocr_overlays;
use crate::widget::overlays::status_overlays::draw_ocr_status_indicator;
use crate::widget::overlays::status_overlays::draw_qr_code_overlays;
use crate::widget::overlays::status_overlays::draw_qr_scanning_indicator;
use crate::widget::overlays::status_overlays::draw_status_badge;
use crate::widget::overlays::{ShapesOverlay, draw_operations, samples_pixels};
use crate::widget::rectangle_selection::RectangleSelection;
use crate::widget::tool_button::build_pencil_popup;
use crate::widget::tool_button::build_shape_popup;
use crate::widget::toolbar::build_toolbar;
use cosmic::Element;
use cosmic::cosmic_theme::Spacing;
use cosmic::iced;
use cosmic::iced::core::Layout;
use cosmic::iced::core::Length;
use cosmic::iced::core::Point;
use cosmic::iced::core::Size;
use cosmic::iced::core::layout;
use cosmic::iced::core::overlay;
use cosmic::iced::core::widget::Tree;
use cosmic::iced::widget::canvas;
use cosmic::iced::window;
use cosmic::widget::image;
use viewer_tools::annotate::{MagnifierOperation, TextOperation};

/// Distance (logical px) from a magnifier's ring within which a drag resizes it
const MAGNIFIER_RING_GRAB: f32 = 12.0;
/// Zoom change per mouse wheel notch when scrolling over a selected magnifier
const MAGNIFIER_SCROLL_STEP: f32 = 0.5;
/// Interaction state kept in the widget's `Tree`, so it survives view rebuilds.
#[derive(Default)]
struct InteractionState {
    drag: Option<MagnifierDrag>,
    /// Annotation being dragged by the move tool, and the last pointer position in
    /// global logical coordinates. Deltas are taken against the last position.
    move_drag: Option<(usize, f32, f32)>,
    /// The move tool's last press: when, where (global) and what it hit, for
    /// telling a double-click on a label.
    last_press: Option<(std::time::Instant, f32, f32, Option<usize>)>,
}

/// Two presses this close in time and place are one double-click.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);
const DOUBLE_CLICK_SLACK: f32 = 6.0;

enum MagnifierDrag {
    /// Moving the loupe. Grab offset is (cursor - center) in global logical coords
    Move {
        index: usize,
        grab_dx: f32,
        grab_dy: f32,
    },
    /// Resizing the loupe (radius follows the cursor)
    Resize { index: usize },
}

/// Output context for multi-monitor support
#[derive(Clone, Debug)]
pub struct OutputContext {
    pub output_count: usize,
    pub focused_output_index: usize,
    pub current_output_index: usize,
    pub is_active_output: bool,
    pub has_confirmed_selection: bool,
    /// Whether mouse has entered any output yet (for initial highlight)
    pub has_mouse_entered: bool,
}

/// Place a popup against the toolbar: centered on its button, above the toolbar
/// if there is room, and clamped into the output.
#[allow(clippy::too_many_arguments)]
fn anchor_to_toolbar(
    popup: Size,
    menu_pos: Point,
    menu: Size,
    btn_fraction: f32,
    gap: f32,
    margin: f32,
    output: Size,
) -> Point {
    let anchor_x = menu.width.mul_add(btn_fraction, menu_pos.x);
    let x = (anchor_x - popup.width / 2.0)
        .max(margin)
        .min((output.width - popup.width - margin).max(margin));

    let above = menu_pos.y - popup.height - gap;
    let below = menu_pos.y + menu.height + gap;
    let y = if above >= margin {
        above
    } else if below + popup.height + margin <= output.height {
        below
    } else {
        // Neither side fits. Favor above and let the clamp keep it on-screen.
        above
            .max(margin)
            .min((output.height - popup.height).max(0.0))
    };

    Point { x, y }
}

/// The selection widget.
pub struct ScreenshotSelectionWidget<'a> {
    id: cosmic::widget::Id,

    // Core state
    pub choice: Choice,
    pub output: &'a OutputState,
    pub window_id: window::Id,
    pub spacing: Spacing,
    pub dnd_id: u128,

    // Image references
    pub screenshot_image: &'a ScreenshotImage,

    /// The picked window's pixels and where to put them, drawn centered over a darkened screen.
    // Grouped state (references into the capture)
    pub annotations: &'a AnnotationState,
    pub detection: &'a DetectionState,
    pub ui: &'a UiState,

    // Output context
    pub output_ctx: OutputContext,

    // Computed state
    pub has_any_annotations: bool,
    pub has_any_redactions: bool,
    pub has_ocr_text: bool,

    // Cached computed values
    output_rect: Rect,
    /// This output's name. Toolbar positions are keyed by it.
    output_name: String,
    selection_rect: Option<(f32, f32, f32, f32)>,
    show_qr_overlays: bool,
    /// Whether any toolbar dropdown is floating above the surface
    popover_open: bool,
    /// Whether the text tool is armed
    text_mode: bool,
    /// Whether a label is currently open for editing
    text_open: bool,
    /// The open label and the corner its coordinates are measured from, for
    /// hit-testing the resize handles under the cursor.
    text_edit_ref: Option<(&'a viewer_tools::annotate::TextPreview, (f32, f32))>,
    qr_codes_for_output: Vec<(f32, f32, f32, f32, String)>,
    /// Recognized text regions on this output, for the boxes drawn over them.
    ocr_overlays_for_output: Vec<(f32, f32, f32, f32, i32)>,

    // Pre-built child elements
    bg_element: Element<'a, Msg>,
    fg_element: Element<'a, Msg>,
    menu_element: Element<'a, Msg>,
    shapes_element: Element<'a, Msg>,
    shape_popup_element: Option<Element<'a, Msg>>,
    redact_popup_element: Option<Element<'a, Msg>>,
    pencil_popup_element: Option<Element<'a, Msg>>,
    magnifier_popup_element: Option<Element<'a, Msg>>,
}

impl<'a> ScreenshotSelectionWidget<'a> {
    /// Create a new widget
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        choice: Choice,
        screenshot_image: &'a ScreenshotImage,
        output: &'a OutputState,
        window_id: window::Id,
        spacing: Spacing,
        dnd_id: u128,
        annotations: &'a AnnotationState,
        detection: &'a DetectionState,
        ui: &'a UiState,
        color_picker: &'a cosmic::widget::ColorPickerModel,
        font_families: &'a [&'static str],
        text_style_model: &'a cosmic::widget::segmented_button::MultiSelectModel,
        text_align_model: &'a cosmic::widget::segmented_button::SingleSelectModel,
        text: Option<&'a crate::capture::text::TextEdit>,
        output_ctx: OutputContext,
        has_any_annotations: bool,
        has_any_redactions: bool,
        has_ocr_text: bool,
    ) -> Self {
        let output_rect = create_output_rect(output.logical_pos, output.logical_size);
        let image_scale = screenshot_image.rgba.width() as f32 / output.logical_size.0 as f32;

        // Build QR overlay: only show when not actively dragging a rectangle
        let show_qr_overlays = match choice {
            Choice::Rectangle(_, DragState::None) => true,
            Choice::Rectangle(_, _) => false,
            _ => true,
        };

        // Filter QR codes and OCR overlays for this output
        let qr_codes_for_output = filter_qr_codes_for_output(&detection.qr_codes, &output.name);
        let ocr_overlays_for_output =
            filter_ocr_overlays_for_output(&detection.ocr_overlays, &output.name);

        let selection_rect = calculate_selection_rect(&choice, output_rect, output.logical_size);

        let space_s = spacing.space_s;
        let space_xs = spacing.space_xs;
        let space_xxs = spacing.space_xxs;

        // Build bg_element
        let bg_element: Element<'a, Msg> = image::Image::new(screenshot_image.handle.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        // Build fg_element
        let move_offset = ui.move_offset;
        let fg_element: Element<'a, Msg> = match choice.clone() {
            Choice::Rectangle(r, drag_state) => RectangleSelection::new(
                output_rect,
                r,
                drag_state,
                window_id,
                dnd_id,
                move |s, r| Msg::choice(Choice::Rectangle(r, s)),
                Msg::set_move_offset,
                screenshot_image,
                image_scale,
                // Move mode owns the mouse too, or the selection would take the press.
                annotations.any_mode_active()
                    || ui.move_mode
                    || ui.shape_popup_open
                    || ui.freehand_popup_open
                    || ui.redact_popup_open
                    || ui.magnifier_popup_open
                    || ui.settings_drawer_open,
                ui.magnifier_enabled,
                ui.is_recording,
                move_offset,
            )
            .into(),
            // Every output is captured whole, so there is no selection to draw.
            Choice::AllScreens => cosmic::iced::widget::space()
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            Choice::Output(None) => {
                // Only show focus highlight after mouse has entered an output
                // This prevents wrong initial highlight on first output
                let is_focused = output_ctx.has_mouse_entered
                    && output_ctx.current_output_index == output_ctx.focused_output_index;
                OutputSelection::new(Msg::output_changed(output.output.clone()))
                    .picker_mode(true)
                    .focused(is_focused)
                    .on_click(Msg::confirm())
                    .into()
            }
            Choice::Output(Some(ref selected_output)) => {
                let is_selected = selected_output == &output.name;
                OutputSelection::new(Msg::output_changed(output.output.clone()))
                    .picker_mode(false)
                    .selected(is_selected)
                    .into()
            }
        };

        // Build menu_element
        let has_selection = choice.has_selection();
        // Whether the capture button starts a countdown instead of saving. The
        // recapture drops any annotations, so they do not disarm the delay.
        let delayed = ui.capture_delay_secs > 0 && !ui.is_video_mode;

        let redact_mode_active = match ui.primary_redact_tool {
            crate::config::RedactTool::Redact => annotations.redact_mode,
            crate::config::RedactTool::Pixelate => annotations.pixelate_mode,
        };

        let menu_element = build_toolbar(
            choice.clone(),
            output.name.clone(),
            has_selection,
            ui.primary_redact_tool,
            redact_mode_active,
            ui.redact_popup_open,
            space_s,
            space_xs,
            space_xxs,
            Msg::choice,
            Msg::screen_mode(output_ctx.current_output_index),
            Msg::save_to_pictures(),
            Msg::delayed_capture(),
            Msg::toggle_delay_popup(),
            Box::new(Msg::set_capture_delay),
            ui.delay_popup_open,
            ui.capture_delay_secs,
            delayed,
            Msg::record_region(),
            Msg::stop_recording(),
            Msg::toggle_recording_annotation(),
            Msg::toggle_pencil_popup(),
            Msg::redact_tool_mode_toggle(),
            Msg::toggle_redact_popup(),
            Msg::cancel(),
            Msg::toolbar_drag_start(),
            Msg::toolbar_drag_end(),
            output_ctx.output_count,
            has_ocr_text,
            Msg::ocr_requested(),
            Msg::ocr_copy_and_close(),
            ui.is_video_mode,
            ui.is_recording,
            ui.toolbar_anim.progress(ui.now),
            ui.recording_annotation_mode,
            ui.pencil_popup_open,
            Msg::toggle_capture_mode(false),
            Msg::toggle_capture_mode(true),
            {
                // With an annotation picked up, the controls show and edit it.
                let picked = ui.move_mode.then(|| annotations.picked()).flatten();
                let picked_size = |tool: crate::config::ShapeTool, default: f32| {
                    picked
                        .filter(|p| {
                            p.tool == tool
                                || (tool == crate::config::ShapeTool::Arrow
                                    && p.tool.shape_kind().is_some())
                        })
                        .and_then(|p| p.size)
                        .unwrap_or(default)
                };
                crate::widget::toolbar::AnnotateSection {
                    active: ui.annotate_mode,
                    shape_choice: ui.shape_choice,
                    freehand_choice: ui.freehand_choice,
                    active_tool: if let Some(p) = picked {
                        Some(p.tool)
                    } else if let Some(kind) = annotations.shape_mode {
                        crate::config::ShapeTool::SHAPES
                            .iter()
                            .copied()
                            .find(|t| t.shape_kind() == Some(kind))
                    } else if annotations.text_mode {
                        Some(crate::config::ShapeTool::Text)
                    } else if annotations.pen_mode {
                        Some(crate::config::ShapeTool::Pen)
                    } else if annotations.highlighter_mode {
                        Some(crate::config::ShapeTool::Highlighter)
                    } else if annotations.pixelate_mode {
                        Some(crate::config::ShapeTool::Pixelate)
                    } else if annotations.magnifier_mode {
                        Some(crate::config::ShapeTool::Magnifier)
                    } else {
                        None
                    },
                    color: picked.and_then(|p| p.color).unwrap_or(ui.shape_color),
                    thickness: picked_size(crate::config::ShapeTool::Arrow, ui.shape_thickness),
                    highlighter_thickness: picked_size(
                        crate::config::ShapeTool::Highlighter,
                        ui.highlighter_thickness,
                    ),
                    pixelate_block_size: picked_size(
                        crate::config::ShapeTool::Pixelate,
                        ui.pixelation_block_size as f32,
                    ),
                    magnification: picked_size(
                        crate::config::ShapeTool::Magnifier,
                        ui.magnifier_magnification,
                    ),
                    shape_popup_open: ui.shape_popup_open,
                    freehand_popup_open: ui.freehand_popup_open,
                    text_format_popup_open: ui.text_format_popup_open,
                    // Built here: `dropdown` wants a `Send + Sync` closure the generic toolbar cannot offer.
                    font_family_picker: {
                        let selected = font_families
                            .iter()
                            .position(|f| *f == ui.text_format.font_family);
                        cosmic::widget::dropdown(font_families, selected, move |i| {
                            Msg::text(TextAction::Family(i))
                        })
                        .width(Length::Fixed(160.0))
                        .into()
                    },
                    font_size_picker: {
                        cosmic::widget::dropdown(
                            &viewer_tools::annotate::tool::text::FONT_SIZE_LABELS,
                            ui.text_format.size_index(),
                            move |i| Msg::text(TextAction::Size(i)),
                        )
                        .width(Length::Fixed(96.0))
                        .into()
                    },
                    text_style_control: cosmic::widget::segmented_control::horizontal(
                        text_style_model,
                    )
                    .on_activate(move |entity| Msg::text(TextAction::StyleActivated(entity)))
                    .into(),
                    text_align_control: cosmic::widget::segmented_control::horizontal(
                        text_align_model,
                    )
                    .on_activate(move |entity| Msg::text(TextAction::AlignActivated(entity)))
                    .into(),
                    on_text_format_popup: Msg::text(TextAction::TogglePopup),
                    stroke_popup_open: ui.stroke_popup_open,
                    can_undo: annotations.can_undo(),
                    can_redo: annotations.can_redo(),
                    on_undo: Msg::undo(),
                    on_redo: Msg::redo(),
                    move_mode: ui.move_mode,
                    // A label being typed counts as an annotation. Pressing move commits it first.
                    can_move: text.is_some()
                        || annotations.operations().iter().any(|op| op.movable()),
                    on_move: Msg::toggle_move_mode(),
                    delete_progress: ui.toolbar_anim.progress(ui.now).delete,
                    on_delete_selected: Msg::delete_selected(),
                    on_toggle: Msg::annotate_mode_toggle(),
                    on_shape_popup: Msg::toggle_shape_popup(),
                    on_freehand_popup: Msg::toggle_freehand_popup(),
                    on_stroke_popup: Msg::toggle_stroke_popup(),
                    on_select_tool: Box::new(Msg::set_shape_tool),
                    on_select_color: Box::new(Msg::set_shape_color),
                    on_select_thickness: Box::new(Msg::set_shape_thickness),
                    custom_color: color_picker.get_applied_color(),
                    // Built here: `builder` takes a function pointer the generic toolbar cannot supply.
                    color_picker_panel: (color_picker.get_is_active()
                        && output_ctx.is_active_output)
                        .then(|| {
                            Element::from(
                                color_picker
                                    .builder(|u| {
                                        Msg::Tool(crate::capture::msg::ToolMsg::ColorPicker(u))
                                    })
                                    .reset_label(fl!("reset-to-default"))
                                    .build(fl!("recent-colors"), fl!("copy"), fl!("copied")),
                            )
                        }),
                    on_color_picker_toggle: Msg::color_picker(
                        cosmic::widget::color_picker::ColorPickerUpdate::ToggleColorPicker,
                    ),
                }
            },
            {
                crate::widget::toolbar::SettingsMenu {
                    open: ui.settings_drawer_open,
                    magnifier_enabled: ui.magnifier_enabled,
                    save_location: ui.save_location_setting,
                    custom_save_path: &ui.custom_save_path,
                    video_custom_save_path: &ui.video_custom_save_path,
                    video_save_location: ui.video_save_location_setting,
                    video_show_cursor: ui.video_show_cursor,
                    show_cursor: ui.show_cursor,
                    is_default_portal: ui.is_default_portal,
                    recognize_qr_codes: ui.recognize_qr_codes,
                    // Built here for the same reason as the font picker.
                    encoder_picker: {
                        // Automatic heads the list: it re-picks the best detected encoder on each machine.
                        let elements: Vec<Option<String>> = std::iter::once(None)
                            .chain(
                                ui.available_encoders
                                    .iter()
                                    .map(|e| Some(e.gst_element.clone())),
                            )
                            .collect();
                        let labels: Vec<String> = std::iter::once(fl!("encoder-automatic"))
                            .chain(
                                ui.available_encoders
                                    .iter()
                                    .map(crate::recording::encoder::EncoderInfo::display_name),
                            )
                            .collect();
                        let selected = elements
                            .iter()
                            .position(|e| e.as_deref() == ui.selected_encoder.as_deref());
                        cosmic::widget::dropdown(labels, selected, move |i| {
                            Msg::set_video_encoder(elements.get(i).cloned().flatten())
                        })
                        .width(cosmic::iced::Length::Fill)
                        .into()
                    },
                    format_picker: {
                        // Only the containers this codec can be muxed into.
                        let codec = ui
                            .selected_encoder
                            .as_deref()
                            .or_else(|| {
                                ui.available_encoders
                                    .first()
                                    .map(|e| e.gst_element.as_str())
                            })
                            .and_then(crate::recording::encoder::Codec::from_element_name);
                        let containers = crate::recording::encoder::containers_for(codec);
                        let labels: Vec<String> = containers
                            .iter()
                            .map(|c| crate::widget::toolbar::container_name(*c).to_string())
                            .collect();
                        let selected = containers.iter().position(|c| *c == ui.video_container);
                        cosmic::widget::dropdown(labels, selected, move |i| {
                            Msg::set_video_container(containers[i])
                        })
                        .width(cosmic::iced::Length::Fill)
                        .into()
                    },
                    framerate_picker: {
                        let labels: Vec<String> = crate::widget::toolbar::FRAMERATE_OPTIONS
                            .iter()
                            .map(|fps| format!("{fps} fps"))
                            .collect();
                        let selected = crate::widget::toolbar::FRAMERATE_OPTIONS
                            .iter()
                            .position(|fps| *fps == ui.video_framerate);
                        cosmic::widget::dropdown(labels, selected, move |i| {
                            Msg::set_video_framerate(crate::widget::toolbar::FRAMERATE_OPTIONS[i])
                        })
                        .width(cosmic::iced::Length::Fill)
                        .into()
                    },
                    on_toggle: Msg::toggle_settings_drawer(),
                    on_magnifier: Msg::toggle_magnifier(),
                    on_show_cursor: Msg::toggle_show_cursor(),
                    on_screenshot_cursor: Msg::toggle_screenshot_cursor(),
                    on_default_portal: Msg::set_as_default_portal(),
                    on_recognize_qr_codes: Msg::toggle_recognize_qr_codes(),
                    on_save_clipboard: Msg::set_save_location_clipboard(),
                    on_save_pictures: Msg::set_save_location_pictures(),
                    on_save_documents: Msg::set_save_location_documents(),
                    on_save_custom: Msg::set_save_location_custom(),
                    on_browse_custom: Msg::browse_save_location(),
                    on_save_videos: Msg::set_video_save_location_videos(),
                    on_save_video_custom: Msg::set_video_save_location_custom(),
                    on_browse_video_custom: Msg::browse_video_save_location(),
                }
            },
        );

        // Build shapes_element
        let text_edit = text.map(|edit| (&edit.preview, edit.origin));
        let shapes_element = {
            let program = ShapesOverlay {
                selection_rect,
                output_rect,
                operations: annotations.operations(),
                source: viewer_tools::SampleSource {
                    image: &screenshot_image.rgba,
                    origin: Point::new(output_rect.left as f32, output_rect.top as f32),
                    scale: image_scale,
                    below: &[],
                },
                selected: annotations.selected,
                redact_drawing: annotations.redact_drawing,
                pixelate_drawing: annotations.pixelate_drawing,
                magnifier_drawing: annotations.magnifier_drawing,
                pixelation_block_size: ui.pixelation_block_size as f32,
                magnifier_magnification: ui.magnifier_magnification,
                shape_mode: annotations.shape_mode,
                pen_mode: annotations.pen_mode || annotations.highlighter_mode,
                highlighter_mode: annotations.highlighter_mode,
                highlighter_thickness: ui.highlighter_thickness,
                shape_drawing: annotations.shape_drawing,
                text_edit,
                text_mode: annotations.text_mode,
                on_text_press: Some(Box::new(move |x, y| Msg::text(TextAction::Press(x, y)))),
                on_text_drag: Some(Box::new(move |x, y| Msg::text(TextAction::Drag(x, y)))),
                on_text_release: Some(Box::new({
                    move |x, y| Msg::text(TextAction::Release(x, y))
                })),
                stroke_drawing: annotations.stroke_drawing.clone(),
                on_shape_start: Some(Box::new(Msg::shape_start)),
                on_shape_end: Some(Box::new(Msg::shape_end)),
                on_pencil_start: Some(Box::new(Msg::pen_start)),
                on_pencil_move: Some(Box::new(Msg::pen_move)),
                on_pencil_end: Some(Box::new(Msg::pen_end)),
                shape_color: ui.shape_color,
                shape_thickness: ui.shape_thickness,
            };

            canvas::Canvas::new(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        // Build settings_drawer_element
        // Build shape_popup_element
        // The annotation section has its own shapes popover. This popup stays down while it is up.
        let shape_popup_element = if ui.shape_popup_open && !ui.annotate_mode {
            Some(build_shape_popup(
                ui.primary_shape_tool,
                ui.shape_color,
                has_any_annotations,
                &Msg::set_shape_tool,
                ui.shape_thickness,
                Msg::set_shape_thickness,
                Msg::save_shape_thickness(),
                &Msg::set_shape_color,
                Msg::clear_shapes(),
                space_s,
                space_xs,
            ))
        } else {
            None
        };

        // Build redact_popup_element
        // Redaction lost its toolbar button, and pixelation moved to the
        // annotation row where the stroke dropdown carries its block size.
        let redact_popup_element: Option<Element<'a, Msg>> = None;

        let magnifier_popup_element: Option<Element<'a, Msg>> = None;

        // Build pencil_popup_element (only shown during recording)
        let pencil_popup_element = if ui.pencil_popup_open && ui.is_recording {
            let has_pencil_annotations = true; // Always enable clear button during recording
            Some(build_pencil_popup(
                ui.pencil_color,
                ui.pencil_fade_duration,
                ui.pencil_thickness,
                has_pencil_annotations,
                &Msg::set_pencil_color,
                Msg::set_pencil_fade_duration,
                Msg::save_pencil_fade_duration(),
                Msg::set_pencil_thickness,
                Msg::save_pencil_thickness(),
                Msg::clear_stroke_drawings(),
                space_s,
                space_xs,
            ))
        } else {
            None
        };

        Self {
            id: cosmic::widget::Id::unique(),
            output_name: output.name.clone(),
            choice,
            output,
            window_id,
            spacing,
            dnd_id,
            screenshot_image,
            annotations,
            detection,
            ui,
            output_ctx,
            has_any_annotations,
            has_any_redactions,
            has_ocr_text,
            output_rect,
            selection_rect,
            show_qr_overlays,
            text_mode: annotations.text_mode,
            text_open: text.is_some(),
            text_edit_ref: text_edit,
            popover_open: ui.shape_popup_open
                || ui.freehand_popup_open
                || ui.stroke_popup_open
                || ui.font_popup_open
                || ui.delay_popup_open
                || ui.settings_drawer_open
                || color_picker.get_is_active(),
            qr_codes_for_output,
            ocr_overlays_for_output,
            bg_element,
            fg_element,
            menu_element,
            shapes_element,
            shape_popup_element,
            redact_popup_element,
            pencil_popup_element,
            magnifier_popup_element,
        }
    }

    // Helper methods to check current mode

    const fn is_redact_mode(&self) -> bool {
        self.annotations.redact_mode
    }

    const fn is_pixelate_mode(&self) -> bool {
        self.annotations.pixelate_mode
    }

    const fn is_magnifier_mode(&self) -> bool {
        self.annotations.magnifier_mode
    }

    const fn is_any_drawing_mode(&self) -> bool {
        self.annotations.any_mode_active()
    }
}

impl cosmic::iced::core::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for ScreenshotSelectionWidget<'_>
{
    fn children(&self) -> Vec<Tree> {
        let mut children = vec![
            Tree::new(&self.bg_element),
            Tree::new(&self.fg_element),
            Tree::new(&self.shapes_element),
            Tree::new(&self.menu_element),
        ];
        if let Some(ref selector) = self.shape_popup_element {
            children.push(Tree::new(selector));
        }
        if let Some(ref popup) = self.redact_popup_element {
            children.push(Tree::new(popup));
        }
        if let Some(ref popup) = self.pencil_popup_element {
            children.push(Tree::new(popup));
        }
        if let Some(ref popup) = self.magnifier_popup_element {
            children.push(Tree::new(popup));
        }
        children
    }

    fn diff(&mut self, tree: &mut Tree) {
        let mut elements: Vec<&mut Element<'_, Msg>> = vec![
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.shapes_element,
            &mut self.menu_element,
        ];
        if let Some(ref mut selector) = self.shape_popup_element {
            elements.push(selector);
        }
        if let Some(ref mut popup) = self.redact_popup_element {
            elements.push(popup);
        }
        if let Some(ref mut popup) = self.pencil_popup_element {
            elements.push(popup);
        }
        if let Some(ref mut popup) = self.magnifier_popup_element {
            elements.push(popup);
        }
        tree.diff_children(&mut elements);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let children = &mut tree.children;
        let bg_node = self
            .bg_element
            .as_widget_mut()
            .layout(&mut children[0], renderer, limits);
        let fg_node = self
            .fg_element
            .as_widget_mut()
            .layout(&mut children[1], renderer, limits);
        let shapes_node =
            self.shapes_element
                .as_widget_mut()
                .layout(&mut children[2], renderer, limits);
        let mut menu_node =
            self.menu_element
                .as_widget_mut()
                .layout(&mut children[3], renderer, limits);

        // The sections animate their own widths, so the measured size is the animated one.
        let menu_bounds = menu_node.bounds();
        let margin = 32.0_f32;
        let output = limits.max();

        // Dragged position, or centered along the bottom. Clamped for narrower outputs.
        let default_pos = Point {
            x: (output.width - menu_bounds.width) / 2.0,
            y: output.height - menu_bounds.height - margin,
        };
        let menu_pos = self
            .ui
            .toolbar_pos
            .get(&self.output_name)
            .map_or(default_pos, |pos| Point {
                x: pos
                    .x
                    .clamp(0.0, (output.width - menu_bounds.width).max(0.0)),
                y: pos
                    .y
                    .clamp(0.0, (output.height - menu_bounds.height).max(0.0)),
            });
        menu_node = menu_node.move_to(menu_pos);

        let mut nodes = vec![bg_node, fg_node, shapes_node, menu_node];

        // Layout shape selector popup if present
        if let Some(ref mut selector) = self.shape_popup_element {
            let child_idx = 4;
            let mut selector_node =
                selector
                    .as_widget_mut()
                    .layout(&mut children[child_idx], renderer, limits);
            let selector_bounds = selector_node.bounds();
            let selector_margin = 4.0_f32;
            let shapes_btn_fraction = 0.42_f32;

            let selector_pos = anchor_to_toolbar(
                selector_bounds.size(),
                menu_pos,
                menu_bounds.size(),
                shapes_btn_fraction,
                selector_margin,
                margin,
                output,
            );
            selector_node = selector_node.move_to(selector_pos);
            nodes.push(selector_node);
        }

        // Layout redact popup if present
        if let Some(ref mut popup) = self.redact_popup_element {
            let mut child_idx = 4;
            if self.shape_popup_element.is_some() {
                child_idx += 1;
            }
            let mut popup_node =
                popup
                    .as_widget_mut()
                    .layout(&mut children[child_idx], renderer, limits);
            let popup_bounds = popup_node.bounds();
            let popup_margin = 4.0_f32;
            let redact_btn_fraction = 0.52_f32;

            let popup_pos = anchor_to_toolbar(
                popup_bounds.size(),
                menu_pos,
                menu_bounds.size(),
                redact_btn_fraction,
                popup_margin,
                margin,
                output,
            );
            popup_node = popup_node.move_to(popup_pos);
            nodes.push(popup_node);
        }

        // Layout pencil popup if present (only during recording)
        if let Some(ref mut popup) = self.pencil_popup_element {
            let mut child_idx = 4;
            if self.shape_popup_element.is_some() {
                child_idx += 1;
            }
            if self.redact_popup_element.is_some() {
                child_idx += 1;
            }
            let mut popup_node =
                popup
                    .as_widget_mut()
                    .layout(&mut children[child_idx], renderer, limits);
            let popup_bounds = popup_node.bounds();
            let popup_margin = 4.0_f32;
            // Position near the pencil button: roughly in the middle of the toolbar during recording
            let pencil_btn_fraction = 0.35_f32;

            let popup_pos = anchor_to_toolbar(
                popup_bounds.size(),
                menu_pos,
                menu_bounds.size(),
                pencil_btn_fraction,
                popup_margin,
                margin,
                output,
            );
            popup_node = popup_node.move_to(popup_pos);
            nodes.push(popup_node);
        }

        // Layout magnifier popup if present
        if let Some(ref mut popup) = self.magnifier_popup_element {
            let mut child_idx = 4;
            if self.shape_popup_element.is_some() {
                child_idx += 1;
            }
            if self.redact_popup_element.is_some() {
                child_idx += 1;
            }
            if self.pencil_popup_element.is_some() {
                child_idx += 1;
            }
            let mut popup_node =
                popup
                    .as_widget_mut()
                    .layout(&mut children[child_idx], renderer, limits);
            let popup_bounds = popup_node.bounds();
            let popup_margin = 4.0_f32;
            let magnifier_btn_fraction = 0.60_f32;

            let popup_pos = anchor_to_toolbar(
                popup_bounds.size(),
                menu_pos,
                menu_bounds.size(),
                magnifier_btn_fraction,
                popup_margin,
                margin,
                output,
            );
            popup_node = popup_node.move_to(popup_pos);
            nodes.push(popup_node);
        }

        layout::Node::with_children(
            limits.resolve(Length::Fill, Length::Fill, Size::ZERO),
            nodes,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced::core::renderer::Style,
        layout: Layout<'_>,
        cursor: cosmic::iced::mouse::Cursor,
        viewport: &cosmic::iced::core::Rectangle,
    ) {
        use cosmic::iced::core::Renderer;

        let children = &[
            &self.bg_element,
            &self.fg_element,
            &self.shapes_element,
            &self.menu_element,
        ];
        let mut children_iter = layout.children().zip(children).enumerate();

        // Draw bg_element first
        let bg_info = children_iter.next();
        if let Some((i, (layout, child))) = bg_info {
            let bg_tree = &tree.children[i];
            child
                .as_widget()
                .draw(bg_tree, renderer, theme, style, layout, cursor, viewport);
        }

        // If this is not the active output and there's a confirmed selection, draw a dark overlay
        if !self.output_ctx.is_active_output && self.output_ctx.has_confirmed_selection {
            let bounds = layout.bounds();
            renderer.with_layer(bounds, |renderer| {
                draw_inactive_overlay_with_hint(
                    renderer,
                    bounds,
                    &fl!("change-selection-hint"),
                    0.7,
                );
            });
            return;
        }

        // Get fg_element info
        let fg_info = children_iter.next();

        // Note: fg_element is always drawn after annotations (selection handles on top)

        // Committed annotations, one layer per run ending in a loupe or a
        // pixelation, so what is drawn after those renders above them.
        if let Some((i, (layout, child))) = children_iter.next() {
            let bounds = layout.bounds();
            let origin = Point::new(self.output_rect.left as f32, self.output_rect.top as f32);
            let source = viewer_tools::SampleSource {
                image: &self.screenshot_image.rgba,
                origin,
                scale: self.screenshot_image.rgba.width() as f32
                    / self.output.logical_size.0 as f32,
                below: &[],
            };
            let ops = self.annotations.operations();
            let mut start = 0;
            for end in ops
                .iter()
                .enumerate()
                .filter(|(_, op)| samples_pixels(op.as_ref()))
                .map(|(i, _)| i + 1)
                .chain(std::iter::once(ops.len()))
            {
                if end > start {
                    renderer.with_layer(bounds, |renderer| {
                        draw_operations(renderer, bounds, origin, ops, start..end, source);
                    });
                    start = end;
                }
            }

            // Previews, the brush cursor and the selection box, above everything.
            renderer.with_layer(bounds, |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }

        // Committed arrows are drawn by the shapes overlay through viewer-tools,
        // alongside every other shape, so they are not drawn here.

        // Escape-to-exit confirmation hint
        if self.ui.exit_armed {
            let msg = fl!("press-escape-again");
            // Roughly center the badge horizontally. `draw_status_badge` sizes
            // itself from the string length.
            let approx_w = (msg.len() as f32 * 16.0).mul_add(0.55, 32.0);
            draw_status_badge(
                renderer,
                viewport,
                &msg,
                (viewport.width - approx_w) / 2.0,
                24.0,
                cosmic::iced::Color::from_rgb(0.95, 0.6, 0.1),
                8.0,
            );
        }

        // Draw selection handles on the selected magnifier (when the tool is active)
        if self.is_magnifier_mode()
            && let Some(m) = self
                .annotations
                .selected_magnifier
                .and_then(|i| self.annotations.magnifier(i))
        {
            let accent: cosmic::iced::Color = theme.cosmic().accent_color().into();
            let center = m.center();
            draw_magnifier_handles(
                renderer,
                viewport,
                center.x - self.output_rect.left as f32,
                center.y - self.output_rect.top as f32,
                m.radius(),
                accent,
            );
        }

        // Draw fg_element (selection UI above annotations)
        if let Some((i, (layout, child))) = fg_info {
            renderer.with_layer(layout.bounds(), |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }

        let cosmic_theme = theme.cosmic();
        let accent_color: cosmic::iced::Color = cosmic_theme.accent_color().into();
        let corner_radius: f32 = cosmic_theme.corner_radii.radius_s[0];

        // Draw QR scanning status or QR overlays
        if self.show_qr_overlays {
            if self.detection.qr_scanning {
                draw_qr_scanning_indicator(renderer, viewport, accent_color, corner_radius);
            }

            if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                draw_qr_code_overlays(
                    renderer,
                    viewport,
                    &self.qr_codes_for_output,
                    (sel_x, sel_y, sel_w, sel_h),
                    accent_color,
                    corner_radius,
                );
            }
        }

        // Show OCR status indicator
        draw_ocr_status_indicator(
            renderer,
            viewport,
            &self.detection.ocr_status,
            self.detection.qr_scanning,
            accent_color,
            corner_radius,
        );

        // Draw OCR overlays
        if self.show_qr_overlays {
            draw_ocr_overlays(renderer, viewport, &self.ocr_overlays_for_output);
        }

        // Selection frame, hidden while recording and for whole-screen selections.
        if !self.ui.is_recording
            && !matches!(self.choice, Choice::AllScreens)
            && let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect
        {
            let output_width = (self.output_rect.right - self.output_rect.left) as f32;
            let output_height = (self.output_rect.bottom - self.output_rect.top) as f32;
            draw_selection_frame_with_handles(
                renderer,
                (sel_x, sel_y, sel_w, sel_h),
                (output_width, output_height),
                accent_color,
                corner_radius,
            );
        }

        // Draw the menu. Its own bounds are the animated ones, so it needs no
        // clipping of its own: the sections inside clip themselves as they open.
        let _ = children_iter;
        if let Some((i, (layout, child))) = children_iter.next() {
            renderer.with_layer(layout.bounds(), |renderer| {
                let tree = &tree.children[i];
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
        }

        if let Some(ref selector) = self.shape_popup_element {
            let layout_children: Vec<_> = layout.children().collect();
            let selector_idx = 4;
            if layout_children.len() > selector_idx {
                let selector_layout = layout_children[selector_idx];
                renderer.with_layer(selector_layout.bounds(), |renderer| {
                    let selector_tree = &tree.children[selector_idx];
                    selector.as_widget().draw(
                        selector_tree,
                        renderer,
                        theme,
                        style,
                        selector_layout,
                        cursor,
                        viewport,
                    );
                });
            }
        }

        // Draw redact popup
        if let Some(ref popup) = self.redact_popup_element {
            let layout_children: Vec<_> = layout.children().collect();
            let mut popup_idx = 4;
            if self.shape_popup_element.is_some() {
                popup_idx += 1;
            }
            if layout_children.len() > popup_idx {
                let popup_layout = layout_children[popup_idx];
                renderer.with_layer(popup_layout.bounds(), |renderer| {
                    let popup_tree = &tree.children[popup_idx];
                    popup.as_widget().draw(
                        popup_tree,
                        renderer,
                        theme,
                        style,
                        popup_layout,
                        cursor,
                        viewport,
                    );
                });
            }
        }

        // Draw pencil popup
        if let Some(ref popup) = self.pencil_popup_element {
            let layout_children: Vec<_> = layout.children().collect();
            let mut popup_idx = 4;
            if self.shape_popup_element.is_some() {
                popup_idx += 1;
            }
            if self.redact_popup_element.is_some() {
                popup_idx += 1;
            }
            if layout_children.len() > popup_idx {
                let popup_layout = layout_children[popup_idx];
                renderer.with_layer(popup_layout.bounds(), |renderer| {
                    let popup_tree = &tree.children[popup_idx];
                    popup.as_widget().draw(
                        popup_tree,
                        renderer,
                        theme,
                        style,
                        popup_layout,
                        cursor,
                        viewport,
                    );
                });
            }
        }

        // Draw magnifier popup
        if let Some(ref popup) = self.magnifier_popup_element {
            let layout_children: Vec<_> = layout.children().collect();
            let mut popup_idx = 4;
            if self.shape_popup_element.is_some() {
                popup_idx += 1;
            }
            if self.redact_popup_element.is_some() {
                popup_idx += 1;
            }
            if self.pencil_popup_element.is_some() {
                popup_idx += 1;
            }
            if layout_children.len() > popup_idx {
                let popup_layout = layout_children[popup_idx];
                renderer.with_layer(popup_layout.bounds(), |renderer| {
                    let popup_tree = &tree.children[popup_idx];
                    popup.as_widget().draw(
                        popup_tree,
                        renderer,
                        theme,
                        style,
                        popup_layout,
                        cursor,
                        viewport,
                    );
                });
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &cosmic::iced::core::Event,
        layout: Layout<'_>,
        cursor: cosmic::iced::mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn cosmic::iced::core::Clipboard,
        shell: &mut cosmic::iced::core::Shell<'_, Msg>,
        viewport: &cosmic::iced::core::Rectangle,
    ) {
        use cosmic::iced::core::Event;
        use cosmic::iced::core::mouse::{Button, Event as MouseEvent};

        if self.output_ctx.is_active_output
            && matches!(event, Event::Mouse(_))
            && let Some(menu_layout) = layout.children().nth(3)
        {
            shell.publish(Msg::toolbar_bounds(menu_layout.bounds()));
        }

        // Toolbar drag: the grip reports press and release, this widget tracks the motion.
        if self.ui.toolbar_dragging && self.output_ctx.is_active_output {
            match event {
                Event::Mouse(MouseEvent::CursorMoved { position }) => {
                    shell.publish(Msg::toolbar_drag_move(self.output_name.clone(), *position));
                    shell.capture_event();
                    return;
                }
                Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                    // Releasing outside the grip still has to end the drag.
                    shell.publish(Msg::toolbar_drag_end());
                    shell.capture_event();
                    return;
                }
                _ => {}
            }
        }

        // During recording with annotation mode OFF, pass through mouse events
        // that are outside the toolbar (allows interacting with desktop)
        if self.ui.is_recording
            && !self.ui.recording_annotation_mode
            && let Event::Mouse(_) = &event
            && let Some(pos) = cursor.position()
        {
            let layout_children: Vec<_> = layout.children().collect();
            let toolbar_bounds = layout_children
                .get(3)
                .map(cosmic::iced::advanced::Layout::bounds);

            // Check if click is inside toolbar or any open popup
            let inside_toolbar = toolbar_bounds.is_some_and(|b| b.contains(pos));

            // Also check for open popups
            let inside_pencil_popup = if self.ui.pencil_popup_open {
                let mut popup_idx = 4;
                if self.shape_popup_element.is_some() {
                    popup_idx += 1;
                }
                if self.redact_popup_element.is_some() {
                    popup_idx += 1;
                }
                layout_children
                    .get(popup_idx)
                    .is_some_and(|l| l.bounds().contains(pos))
            } else {
                false
            };

            // If click is outside toolbar and popups, pass it through
            if !inside_toolbar && !inside_pencil_popup {
                return;
            }
        }

        // Handle click-outside-to-close for popups (both left and right click)
        if let Event::Mouse(MouseEvent::ButtonPressed(button)) = &event
            && matches!(button, Button::Left | Button::Right)
            && let Some(pos) = cursor.position()
        {
            let layout_children: Vec<_> = layout.children().collect();

            // Handle shape selector popup click-outside
            if self.ui.shape_popup_open {
                let selector_idx = 4;
                let inside_selector = if layout_children.len() > selector_idx {
                    layout_children[selector_idx].bounds().contains(pos)
                } else {
                    false
                };
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                if !inside_selector && !inside_toolbar {
                    shell.publish(Msg::close_shape_popup());
                    shell.capture_event();
                    return;
                }
            }

            // Handle redact popup click-outside
            if self.ui.redact_popup_open {
                let mut popup_idx = 4;
                if self.shape_popup_element.is_some() {
                    popup_idx += 1;
                }
                let inside_popup = if layout_children.len() > popup_idx {
                    layout_children[popup_idx].bounds().contains(pos)
                } else {
                    false
                };
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                if !inside_popup && !inside_toolbar {
                    shell.publish(Msg::close_redact_popup());
                    shell.capture_event();
                    return;
                }
            }

            // Handle pencil popup click-outside
            if self.ui.pencil_popup_open {
                let mut popup_idx = 4;
                if self.shape_popup_element.is_some() {
                    popup_idx += 1;
                }
                if self.redact_popup_element.is_some() {
                    popup_idx += 1;
                }
                let inside_popup = if layout_children.len() > popup_idx {
                    layout_children[popup_idx].bounds().contains(pos)
                } else {
                    false
                };
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                if !inside_popup && !inside_toolbar {
                    shell.publish(Msg::close_pencil_popup());
                    shell.capture_event();
                    return;
                }
            }

            // Handle magnifier popup click-outside
            if self.ui.magnifier_popup_open {
                let mut popup_idx = 4;
                if self.shape_popup_element.is_some() {
                    popup_idx += 1;
                }
                if self.redact_popup_element.is_some() {
                    popup_idx += 1;
                }
                if self.pencil_popup_element.is_some() {
                    popup_idx += 1;
                }
                let inside_popup = if layout_children.len() > popup_idx {
                    layout_children[popup_idx].bounds().contains(pos)
                } else {
                    false
                };
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                if !inside_popup && !inside_toolbar {
                    shell.publish(Msg::close_magnifier_popup());
                    shell.capture_event();
                    return;
                }
            }

            // Handle settings drawer click-outside
            if self.ui.settings_drawer_open {
                let inside_drawer = if layout_children.len() > 4 {
                    layout_children[4].bounds().contains(pos)
                } else {
                    false
                };
                let inside_toolbar = if layout_children.len() > 3 {
                    layout_children[3].bounds().contains(pos)
                } else {
                    false
                };

                if !inside_drawer && !inside_toolbar {
                    shell.publish(Msg::toggle_settings_drawer());
                    shell.capture_event();
                    return;
                }
            }
        }

        // Block mouse events on non-active outputs
        if !self.output_ctx.is_active_output
            && self.output_ctx.has_confirmed_selection
            && matches!(&event, Event::Mouse(_))
        {
            shell.capture_event();
            return;
        }

        // QR badge clicks use `layout_badge`, the same geometry the drawing uses.
        if matches!(
            &event,
            Event::Mouse(MouseEvent::ButtonPressed(Button::Left))
        ) && let Some(pos) = cursor.position()
            && let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect
        {
            let clamp =
                cosmic::iced::core::Rectangle::new((sel_x, sel_y).into(), (sel_w, sel_h).into());

            for (x, y, _half_width, half_height, content) in &self.qr_codes_for_output {
                let badge = crate::widget::overlays::status_overlays::layout_badge(
                    (*x, *y),
                    *half_height,
                    content,
                    clamp,
                );

                for (action, rect) in &badge.actions {
                    if !rect.contains(pos) {
                        continue;
                    }
                    let msg = match action {
                        crate::capture::detect::QrAction::Open => Msg::open_url(content.clone()),
                        crate::capture::detect::QrAction::Copy => Msg::qr_copy_and_close(),
                        crate::capture::detect::QrAction::Dismiss => Msg::qr_dismiss(),
                    };
                    shell.publish(msg);
                    shell.capture_event();
                    return;
                }
            }
        }

        // Get layout children for bounds checking
        let layout_children = layout.children().collect::<Vec<_>>();

        // Check if click is inside toolbar bounds: if so, only let the toolbar handle it
        // This prevents clicks on the toolbar from starting rectangle selections behind it
        let click_inside_toolbar = if let Event::Mouse(MouseEvent::ButtonPressed(_)) = &event {
            if let Some(pos) = cursor.position() {
                layout_children.len() > 3 && layout_children[3].bounds().contains(pos)
            } else {
                false
            }
        } else {
            false
        };

        // Let child widgets handle the event
        let mut children: Vec<&mut Element<'_, Msg>> = vec![
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.shapes_element,
            &mut self.menu_element,
        ];
        if let Some(ref mut selector) = self.shape_popup_element {
            children.push(selector);
        }
        if let Some(ref mut popup) = self.redact_popup_element {
            children.push(popup);
        }
        if let Some(ref mut popup) = self.pencil_popup_element {
            children.push(popup);
        }
        if let Some(ref mut popup) = self.magnifier_popup_element {
            children.push(popup);
        }

        for (i, (child_layout, child)) in
            layout_children.into_iter().zip(children).enumerate().rev()
        {
            // Skip bg_element (0), fg_element (1), and shapes_element (2) if click is inside toolbar
            // Only let menu_element (3) and popups/drawers handle it
            if click_inside_toolbar && i < 3 {
                continue;
            }

            let child_tree = &mut tree.children[i];
            child.as_widget_mut().update(
                child_tree,
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
            if matches!(event, cosmic::iced::core::event::Event::PlatformSpecific(_)) {
                continue;
            }
            if shell.is_event_captured() {
                return;
            }
        }

        // Capture unhandled clicks inside toolbar and popups to prevent drawing behind them (both left and right)
        if let Event::Mouse(MouseEvent::ButtonPressed(button)) = &event
            && matches!(button, Button::Left | Button::Right)
        {
            // If click was inside toolbar, capture it even if no button handled it
            if click_inside_toolbar {
                shell.capture_event();
                return;
            }

            if let Some(pos) = cursor.position() {
                let layout_children: Vec<_> = layout.children().collect();

                // Check shape popup
                if self.ui.shape_popup_open {
                    let selector_idx = 4;
                    if layout_children.len() > selector_idx
                        && layout_children[selector_idx].bounds().contains(pos)
                    {
                        shell.capture_event();
                        return;
                    }
                }

                // Check redact popup
                if self.ui.redact_popup_open {
                    let mut popup_idx = 4;
                    if self.shape_popup_element.is_some() {
                        popup_idx += 1;
                    }
                    if layout_children.len() > popup_idx
                        && layout_children[popup_idx].bounds().contains(pos)
                    {
                        shell.capture_event();
                        return;
                    }
                }

                // Check pencil popup
                if self.ui.pencil_popup_open {
                    let mut popup_idx = 4;
                    if self.shape_popup_element.is_some() {
                        popup_idx += 1;
                    }
                    if self.redact_popup_element.is_some() {
                        popup_idx += 1;
                    }
                    if layout_children.len() > popup_idx
                        && layout_children[popup_idx].bounds().contains(pos)
                    {
                        shell.capture_event();
                        return;
                    }
                }

                // Check magnifier popup
                if self.ui.magnifier_popup_open {
                    let mut popup_idx = 4;
                    if self.shape_popup_element.is_some() {
                        popup_idx += 1;
                    }
                    if self.redact_popup_element.is_some() {
                        popup_idx += 1;
                    }
                    if self.pencil_popup_element.is_some() {
                        popup_idx += 1;
                    }
                    if layout_children.len() > popup_idx
                        && layout_children[popup_idx].bounds().contains(pos)
                    {
                        shell.capture_event();
                        return;
                    }
                }

                // Check settings drawer
                if self.ui.settings_drawer_open
                    && layout_children.len() > 4
                    && layout_children[4].bounds().contains(pos)
                {
                    shell.capture_event();
                    return;
                }
            }
        }

        // Handle annotation drawing events
        if let Event::Mouse(mouse_event) = &event
            && let Some(pos) = cursor.position()
        {
            const ANNOTATION_MARGIN: f32 = 0.0;

            let clamp_to_selection =
                |x: f32, y: f32, sel_x: f32, sel_y: f32, sel_w: f32, sel_h: f32| -> (f32, f32) {
                    let min_x = sel_x + ANNOTATION_MARGIN;
                    let max_x = sel_x + sel_w - ANNOTATION_MARGIN;
                    let min_y = sel_y + ANNOTATION_MARGIN;
                    let max_y = sel_y + sel_h - ANNOTATION_MARGIN;
                    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
                };

            let inside_inner_selection = |sel_x: f32, sel_y: f32, sel_w: f32, sel_h: f32| -> bool {
                pos.x >= sel_x + ANNOTATION_MARGIN
                    && pos.x <= sel_x + sel_w - ANNOTATION_MARGIN
                    && pos.y >= sel_y + ANNOTATION_MARGIN
                    && pos.y <= sel_y + sel_h - ANNOTATION_MARGIN
            };

            // Move tool: pick an annotation up and carry it.
            //
            // Move mode takes precedence over the drawing tools.
            if self.ui.move_mode {
                let global_x = pos.x + self.output_rect.left as f32;
                let global_y = pos.y + self.output_rect.top as f32;
                let drag_state = tree.state.downcast_mut::<InteractionState>();

                // Every press is taken, hit or miss, so a miss cannot drag the selection
                // and clear the annotations.
                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) => {
                        let hit = self.annotations.stack.hit(Point::new(global_x, global_y));
                        let now = std::time::Instant::now();
                        let double_click = drag_state.last_press.is_some_and(|(at, x, y, h)| {
                            h == hit
                                && now.duration_since(at) <= DOUBLE_CLICK
                                && (global_x - x).abs() <= DOUBLE_CLICK_SLACK
                                && (global_y - y).abs() <= DOUBLE_CLICK_SLACK
                        });
                        drag_state.last_press = Some((now, global_x, global_y, hit));
                        // Double-clicking switches to the annotation's own tool. A label
                        // opens for editing and a loupe gets its resize ring and zoom.
                        if double_click
                            && let Some(index) = hit
                            && let Some(op) = self.annotations.stack.get(index)
                        {
                            let any = op.as_any();
                            let handoff = if any.is::<TextOperation>() {
                                Some(vec![
                                    Msg::set_shape_tool(crate::config::ShapeTool::Text),
                                    Msg::text(TextAction::Press(global_x, global_y)),
                                ])
                            } else if any.is::<MagnifierOperation>() {
                                Some(vec![
                                    Msg::set_shape_tool(crate::config::ShapeTool::Magnifier),
                                    Msg::magnifier_select(Some(index)),
                                ])
                            } else {
                                None
                            };
                            if let Some(messages) = handoff {
                                drag_state.move_drag = None;
                                drag_state.last_press = None;
                                shell.publish(Msg::select_annotation(None));
                                for message in messages {
                                    shell.publish(message);
                                }
                                shell.capture_event();
                                return;
                            }
                        }
                        // A press on an annotation selects it and starts a drag. A press elsewhere deselects.
                        if hit != self.annotations.selected {
                            shell.publish(Msg::select_annotation(hit));
                        }
                        drag_state.move_drag = hit.map(|index| (index, global_x, global_y));
                        shell.capture_event();
                        return;
                    }
                    MouseEvent::CursorMoved { .. } => {
                        if let Some((index, last_x, last_y)) = drag_state.move_drag {
                            let (dx, dy) = (global_x - last_x, global_y - last_y);
                            drag_state.move_drag = Some((index, global_x, global_y));
                            if dx != 0.0 || dy != 0.0 {
                                shell.publish(Msg::move_annotation(index, dx, dy));
                            }
                            shell.capture_event();
                            return;
                        }
                    }
                    MouseEvent::ButtonReleased(Button::Left) => {
                        drag_state.move_drag = None;
                        shell.capture_event();
                        return;
                    }
                    _ => {}
                }
            }

            // Handle redact drawing
            if self.is_redact_mode() {
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(Msg::redact_start(global_x, global_y));
                        }
                        shell.capture_event();
                        return;
                    }
                    MouseEvent::ButtonReleased(Button::Left)
                        if self.annotations.redact_drawing.is_some() =>
                    {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(Msg::redact_end(global_x, global_y));
                        }
                        shell.capture_event();
                        return;
                    }
                    _ => {}
                }
            }

            // Handle pixelate drawing
            if self.is_pixelate_mode() {
                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) if inside_selection => {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(Msg::pixelate_start(global_x, global_y));
                        }
                        shell.capture_event();
                        return;
                    }
                    MouseEvent::ButtonReleased(Button::Left)
                        if self.annotations.pixelate_drawing.is_some() =>
                    {
                        if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                            let (clamped_x, clamped_y) =
                                clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                            let global_x = clamped_x + self.output_rect.left as f32;
                            let global_y = clamped_y + self.output_rect.top as f32;
                            shell.publish(Msg::pixelate_end(global_x, global_y));
                        }
                        shell.capture_event();
                        return;
                    }
                    _ => {}
                }
            }

            // Handle magnifier tool: create new, or select / move / resize existing
            if self.is_magnifier_mode() {
                let drag_state = tree.state.downcast_mut::<InteractionState>();

                let off_x = self.output_rect.left as f32;
                let off_y = self.output_rect.top as f32;
                let cursor_gx = pos.x + off_x;
                let cursor_gy = pos.y + off_y;
                let dist_to = |cx: f32, cy: f32| -> f32 { (cursor_gx - cx).hypot(cursor_gy - cy) };

                let inside_selection =
                    if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                        inside_inner_selection(sel_x, sel_y, sel_w, sel_h)
                    } else {
                        false
                    };

                match mouse_event {
                    MouseEvent::ButtonPressed(Button::Left) => {
                        // 1. Grabbing the ring of the selected magnifier -> resize
                        if let Some(sel) = self.annotations.selected_magnifier
                            && let Some(m) = self.annotations.magnifier(sel)
                        {
                            let Point { x: cx, y: cy } = m.center();
                            if (dist_to(cx, cy) - m.radius()).abs() <= MAGNIFIER_RING_GRAB {
                                drag_state.drag = Some(MagnifierDrag::Resize { index: sel });
                                shell.capture_event();
                                return;
                            }
                        }

                        // 2. Clicking inside an existing loupe -> select + move (topmost first)
                        let mut hit = None;
                        for (i, m) in self.annotations.magnifiers().rev() {
                            let Point { x: cx, y: cy } = m.center();
                            if dist_to(cx, cy) <= m.radius() {
                                hit = Some((i, cx, cy));
                                break;
                            }
                        }
                        if let Some((i, cx, cy)) = hit {
                            drag_state.drag = Some(MagnifierDrag::Move {
                                index: i,
                                grab_dx: cursor_gx - cx,
                                grab_dy: cursor_gy - cy,
                            });
                            shell.publish(Msg::magnifier_select(Some(i)));
                            shell.capture_event();
                            return;
                        }

                        // 3. Empty space with a loupe selected -> only deselect it
                        if self.annotations.selected_magnifier.is_some() {
                            shell.publish(Msg::magnifier_select(None));
                            shell.capture_event();
                            return;
                        }
                        // 4. Empty space inside the selection -> start a new loupe
                        if inside_selection {
                            if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                                let (clamped_x, clamped_y) =
                                    clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                                shell.publish(Msg::magnifier_start(
                                    clamped_x + off_x,
                                    clamped_y + off_y,
                                ));
                            }
                            shell.capture_event();
                        }
                    }
                    MouseEvent::CursorMoved { .. } => match &drag_state.drag {
                        Some(MagnifierDrag::Move {
                            index,
                            grab_dx,
                            grab_dy,
                        }) => {
                            let (index, nx, ny) =
                                (*index, cursor_gx - grab_dx, cursor_gy - grab_dy);
                            shell.publish(Msg::magnifier_move(index, nx, ny));
                            shell.capture_event();
                        }
                        Some(MagnifierDrag::Resize { index }) => {
                            let index = *index;
                            if let Some(m) = self.annotations.magnifier(index) {
                                let Point { x: cx, y: cy } = m.center();
                                let r = dist_to(cx, cy);
                                shell.publish(Msg::magnifier_resize(index, r));
                                shell.capture_event();
                            }
                        }
                        None => {}
                    },
                    MouseEvent::ButtonReleased(Button::Left) => {
                        if drag_state.drag.take().is_some() {
                            shell.capture_event();
                            return;
                        }
                        if self.annotations.magnifier_drawing.is_some() {
                            if let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect {
                                let (clamped_x, clamped_y) =
                                    clamp_to_selection(pos.x, pos.y, sel_x, sel_y, sel_w, sel_h);
                                shell.publish(Msg::magnifier_end(
                                    clamped_x + off_x,
                                    clamped_y + off_y,
                                ));
                            }
                            shell.capture_event();
                        }
                    }
                    MouseEvent::WheelScrolled { delta } => {
                        if let Some(sel) = self.annotations.selected_magnifier
                            && let Some(m) = self.annotations.magnifier(sel)
                        {
                            let Point { x: cx, y: cy } = m.center();
                            if dist_to(cx, cy) <= m.radius() {
                                let step = match delta {
                                    cosmic::iced::core::mouse::ScrollDelta::Lines { y, .. } => *y,
                                    cosmic::iced::core::mouse::ScrollDelta::Pixels {
                                        y, ..
                                    } => *y / 40.0,
                                };
                                if step != 0.0 {
                                    let new_zoom = step
                                        .signum()
                                        .mul_add(MAGNIFIER_SCROLL_STEP, m.magnification);
                                    shell.publish(Msg::magnifier_set_zoom(sel, new_zoom));
                                    shell.capture_event();
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: Layout<'_>,
        cursor: cosmic::iced::mouse::Cursor,
        viewport: &cosmic::iced::core::Rectangle,
        renderer: &cosmic::Renderer,
    ) -> cosmic::iced::mouse::Interaction {
        // If actively dragging a rectangle selection, show the appropriate cursor
        // This takes priority and handles cases where cursor position is unavailable during DnD
        if let Choice::Rectangle(_, drag_state) = &self.choice
            && *drag_state != DragState::None
        {
            return match drag_state {
                DragState::Move => cosmic::iced::mouse::Interaction::Grabbing,
                // Resizing puts the loupe under the pointer. Its crosshair replaces it.
                _ if self.ui.magnifier_enabled => cosmic::iced::mouse::Interaction::Hidden,
                DragState::N | DragState::S => cosmic::iced::mouse::Interaction::ResizingVertically,
                DragState::E | DragState::W => {
                    cosmic::iced::mouse::Interaction::ResizingHorizontally
                }
                DragState::NW | DragState::NE | DragState::SE | DragState::SW => {
                    cosmic::iced::mouse::Interaction::Grabbing
                }
                DragState::None => unreachable!(),
            };
        }

        // The toolbar and its popups set their own cursors, ahead of the tools.
        let over_chrome = cursor.position().is_some_and(|pos| {
            layout
                .children()
                .skip(3)
                .any(|child| child.bounds().contains(pos))
        });

        // Move: open hand over a pickable annotation, closed while carrying.
        if self.ui.move_mode && !over_chrome {
            let dragging = state
                .state
                .downcast_ref::<InteractionState>()
                .move_drag
                .is_some();
            if dragging {
                return cosmic::iced::mouse::Interaction::Grabbing;
            }
            if let Some(pos) = cursor.position() {
                let global_x = pos.x + self.output_rect.left as f32;
                let global_y = pos.y + self.output_rect.top as f32;
                if self
                    .annotations
                    .stack
                    .hit(Point::new(global_x, global_y))
                    .is_some()
                {
                    return cosmic::iced::mouse::Interaction::Grab;
                }
            }
        }

        // Freehand: the brush cursor is drawn by the overlay, so hide the system one.
        if (self.annotations.pen_mode || self.annotations.highlighter_mode)
            && !over_chrome
            && let Some(pos) = cursor.position()
            && self.selection_rect.is_some_and(|(x, y, w, h)| {
                pos.x >= x && pos.x <= x + w && pos.y >= y && pos.y <= y + h
            })
        {
            return cosmic::iced::mouse::Interaction::Hidden;
        }

        // Text: the resize handles and the caret each have their own cursor, and
        // the tool armed over bare selection offers to place a label.
        if (self.text_open || self.text_mode)
            && !self.ui.move_mode
            && !over_chrome
            && let Some(pos) = cursor.position()
        {
            let global_x = pos.x + self.output_rect.left as f32;
            let global_y = pos.y + self.output_rect.top as f32;
            if let Some(edit) = self.text_edit_ref {
                let handle = edit.0.handle_at(cosmic::iced::Point::new(
                    global_x - edit.1.0,
                    global_y - edit.1.1,
                ));
                if handle != viewer_tools::annotate::TextDragHandle::None {
                    return handle.cursor();
                }
            }
            if self.text_mode
                && self.selection_rect.is_some_and(|(x, y, w, h)| {
                    pos.x >= x && pos.x <= x + w && pos.y >= y && pos.y <= y + h
                })
            {
                return cosmic::iced::mouse::Interaction::Text;
            }
        }

        let mut children: Vec<&Element<'_, Msg>> = vec![
            &self.bg_element,
            &self.fg_element,
            &self.shapes_element,
            &self.menu_element,
        ];
        if let Some(ref selector) = self.shape_popup_element {
            children.push(selector);
        }
        if let Some(ref popup) = self.redact_popup_element {
            children.push(popup);
        }
        if let Some(ref popup) = self.pencil_popup_element {
            children.push(popup);
        }
        if let Some(ref popup) = self.magnifier_popup_element {
            children.push(popup);
        }

        let layout_children = layout.children().collect::<Vec<_>>();

        // First check popups (indices > 3): they overlay everything
        for (i, (child_layout, child)) in layout_children
            .iter()
            .zip(children.iter())
            .enumerate()
            .rev()
            .skip_while(|(i, _)| *i <= 3)
        // Skip bg, fg, shapes, menu: check popups first
        {
            let tree = &state.children[i];
            let interaction = child.as_widget().mouse_interaction(
                tree,
                *child_layout,
                cursor,
                viewport,
                renderer,
            );
            if cursor.is_over(child_layout.bounds()) {
                return interaction;
            }
        }

        // The toolbar, before the drawing modes, so it keeps a normal arrow.
        if layout_children.len() > 3 {
            let menu_layout = layout_children[3];
            if cursor.is_over(menu_layout.bounds()) {
                let tree = &state.children[3];
                let interaction = children[3].as_widget().mouse_interaction(
                    tree,
                    menu_layout,
                    cursor,
                    viewport,
                    renderer,
                );
                return interaction;
            }
        }

        // QR badges may overhang the selection, so check them before the drawing cursor.
        if let Some(pos) = cursor.position()
            && let Some((sel_x, sel_y, sel_w, sel_h)) = self.selection_rect
        {
            let clamp =
                cosmic::iced::core::Rectangle::new((sel_x, sel_y).into(), (sel_w, sel_h).into());
            for (x, y, _half_width, half_height, content) in &self.qr_codes_for_output {
                let badge = crate::widget::overlays::status_overlays::layout_badge(
                    (*x, *y),
                    *half_height,
                    content,
                    clamp,
                );
                if badge.actions.iter().any(|(_, rect)| rect.contains(pos)) {
                    return cosmic::iced::mouse::Interaction::Pointer;
                }
                if badge.bounds.contains(pos) {
                    return cosmic::iced::mouse::Interaction::default();
                }
            }
        }

        // A popover floats outside the toolbar's bounds.
        if self.popover_open {
            return cosmic::iced::mouse::Interaction::default();
        }

        // If in drawing mode (annotation active), show appropriate cursor based on position
        if self.is_any_drawing_mode()
            && let Some(cursor_pos) = cursor.position()
        {
            // Check if cursor is inside the selection region
            if let Some((x, y, w, h)) = self.selection_rect {
                let selection_bounds = cosmic::iced::core::Rectangle {
                    x,
                    y,
                    width: w,
                    height: h,
                };
                if selection_bounds.contains(cursor_pos) {
                    return cosmic::iced::mouse::Interaction::Crosshair;
                }
                return cosmic::iced::mouse::Interaction::NotAllowed;
            }
            // No selection, show crosshair everywhere
            return cosmic::iced::mouse::Interaction::Crosshair;
        }

        // Then check fg_element (RectangleSelection) for move/resize cursors
        // This is index 1
        if layout_children.len() > 1 {
            let fg_layout = layout_children[1];
            let tree = &state.children[1];
            let interaction = children[1]
                .as_widget()
                .mouse_interaction(tree, fg_layout, cursor, viewport, renderer);
            if cursor.is_over(fg_layout.bounds())
                && interaction != cosmic::iced::mouse::Interaction::default()
            {
                return interaction;
            }
        }

        // Default to crosshair for rectangle selection area
        cosmic::iced::mouse::Interaction::Crosshair
    }

    fn overlay<'b>(
        &'b mut self,
        state: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &cosmic::iced::core::Rectangle,
        translation: iced::Vector,
    ) -> Option<overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>> {
        let mut elements: Vec<&mut Element<'_, Msg>> = vec![
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.shapes_element,
            &mut self.menu_element,
        ];
        if let Some(ref mut selector) = self.shape_popup_element {
            elements.push(selector);
        }
        if let Some(ref mut popup) = self.redact_popup_element {
            elements.push(popup);
        }
        if let Some(ref mut popup) = self.pencil_popup_element {
            elements.push(popup);
        }
        if let Some(ref mut popup) = self.magnifier_popup_element {
            elements.push(popup);
        }

        let children = elements
            .into_iter()
            .zip(&mut state.children)
            .zip(layout.children())
            .filter_map(|((child, state), layout)| {
                child
                    .as_widget_mut()
                    .overlay(state, layout, renderer, viewport, translation)
            })
            .collect::<Vec<_>>();

        (!children.is_empty()).then(move || overlay::Group::with_children(children).overlay())
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn cosmic::widget::Operation,
    ) {
        let layout = layout.children().collect::<Vec<_>>();
        let mut children: Vec<&mut Element<'_, Msg>> = vec![
            &mut self.bg_element,
            &mut self.fg_element,
            &mut self.shapes_element,
            &mut self.menu_element,
        ];
        if let Some(ref mut selector) = self.shape_popup_element {
            children.push(selector);
        }
        if let Some(ref mut popup) = self.redact_popup_element {
            children.push(popup);
        }
        if let Some(ref mut popup) = self.pencil_popup_element {
            children.push(popup);
        }
        if let Some(ref mut popup) = self.magnifier_popup_element {
            children.push(popup);
        }
        for (i, (layout, child)) in layout.into_iter().zip(children).enumerate().rev() {
            let tree = &mut tree.children[i];
            child
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
    }

    fn tag(&self) -> cosmic::iced::core::widget::tree::Tag {
        cosmic::iced::core::widget::tree::Tag::of::<InteractionState>()
    }

    fn state(&self) -> cosmic::iced::core::widget::tree::State {
        cosmic::iced::core::widget::tree::State::new(InteractionState::default())
    }

    fn id(&self) -> Option<cosmic::widget::Id> {
        Some(self.id.clone())
    }

    fn set_id(&mut self, id: cosmic::widget::Id) {
        self.id = id;
    }

    fn drag_destinations(
        &self,
        state: &Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        dnd_rectangles: &mut cosmic::iced::core::clipboard::DndDestinationRectangles,
    ) {
        let mut children: Vec<&Element<'_, Msg>> =
            vec![&self.bg_element, &self.fg_element, &self.menu_element];
        if let Some(ref selector) = self.shape_popup_element {
            children.push(selector);
        }
        if let Some(ref popup) = self.redact_popup_element {
            children.push(popup);
        }
        if let Some(ref popup) = self.pencil_popup_element {
            children.push(popup);
        }
        if let Some(ref popup) = self.magnifier_popup_element {
            children.push(popup);
        }
        for (i, (layout, child)) in layout.children().zip(children).enumerate() {
            let state = &state.children[i];
            child
                .as_widget()
                .drag_destinations(state, layout, renderer, dnd_rectangles);
        }
    }
}

impl<'a> From<ScreenshotSelectionWidget<'a>> for Element<'a, Msg> {
    fn from(widget: ScreenshotSelectionWidget<'a>) -> Self {
        Self::new(widget)
    }
}

// Geometry helpers for the selection widget.

/// Filter QR codes for a specific output
pub fn filter_qr_codes_for_output(
    qr_codes: &[DetectedQrCode],
    output_name: &str,
) -> Vec<(f32, f32, f32, f32, String)> {
    qr_codes
        .iter()
        .filter(|qr| qr.output_name == output_name)
        .map(|qr| {
            (
                qr.center_x,
                qr.center_y,
                qr.half_width,
                qr.half_height,
                qr.content.clone(),
            )
        })
        .collect()
}

/// Filter OCR overlays for a specific output
pub fn filter_ocr_overlays_for_output(
    ocr_overlays: &[OcrTextOverlay],
    output_name: &str,
) -> Vec<(f32, f32, f32, f32, i32)> {
    ocr_overlays
        .iter()
        .filter(|o| o.output_name == output_name)
        .map(|o| (o.left, o.top, o.width, o.height, o.block_num))
        .collect()
}

/// The selection as `(x, y, w, h)` in output-local coordinates, if any.
pub fn calculate_selection_rect(
    choice: &Choice,
    output_rect: Rect,
    output_logical_size: (u32, u32),
) -> Option<(f32, f32, f32, f32)> {
    match choice {
        Choice::Rectangle(r, _) => {
            if let Some(intersection) = r.intersect(output_rect) {
                let x = (intersection.left - output_rect.left) as f32;
                let y = (intersection.top - output_rect.top) as f32;
                let w = intersection.width() as f32;
                let h = intersection.height() as f32;
                if w > 0.0 && h > 0.0 {
                    Some((x, y, w, h))
                } else {
                    None
                }
            } else {
                None
            }
        }
        // A chosen screen or every screen selects this output whole.
        Choice::Output(Some(_)) | Choice::AllScreens => Some((
            0.0,
            0.0,
            output_logical_size.0 as f32,
            output_logical_size.1 as f32,
        )),
        _ => None,
    }
}

/// Create an output rect from logical position and size
pub const fn create_output_rect(logical_pos: (i32, i32), logical_size: (u32, u32)) -> Rect {
    Rect {
        left: logical_pos.0,
        top: logical_pos.1,
        right: logical_pos.0 + logical_size.0 as i32,
        bottom: logical_pos.1 + logical_size.1 as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::DragState;

    const OUTPUT: Rect = Rect {
        left: 1920,
        top: 0,
        right: 3840,
        bottom: 1080,
    };

    #[test]
    fn chosen_output_fills_selection() {
        let rect =
            calculate_selection_rect(&Choice::Output(Some("DP-1".into())), OUTPUT, (1920, 1080));
        assert_eq!(rect, Some((0.0, 0.0, 1920.0, 1080.0)));
    }

    #[test]
    fn all_screens_fills_selection() {
        let rect = calculate_selection_rect(&Choice::AllScreens, OUTPUT, (1920, 1080));
        assert_eq!(rect, Some((0.0, 0.0, 1920.0, 1080.0)));
    }

    #[test]
    fn picking_output_is_no_selection() {
        assert_eq!(
            calculate_selection_rect(&Choice::Output(None), OUTPUT, (1920, 1080)),
            None
        );
    }

    #[test]
    fn rectangle_clipped_to_output() {
        let dragged = Rect {
            left: 1820,
            top: 100,
            right: 2020,
            bottom: 300,
        };
        let rect = calculate_selection_rect(
            &Choice::Rectangle(dragged, DragState::None),
            OUTPUT,
            (1920, 1080),
        );
        assert_eq!(rect, Some((0.0, 100.0, 100.0, 200.0)));
    }
}
