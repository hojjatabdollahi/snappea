// SPDX-License-Identifier: GPL-3.0-only

//! The floating toolbar.

use std::rc::Rc;
use std::time::{Duration, Instant};

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::iced::core::{Background, Border, Color};
use cosmic::iced::widget::row;
use cosmic::widget::{button, icon, text, tooltip};

/// Icon size inside a toolbar button. Smaller than the button so the padding
/// separates the tools.
const BTN_SIZE: f32 = 16.0;
/// Source size for button icons. Larger than the box so they stay sharp when
/// the output is scaled.
const ICON_PX: u16 = 32;
/// Section divider height. A full button tall, so it separates groups visually.
const DIVIDER_H: f32 = TOOLBAR_H;
/// Thickness of the toolbar panel's edge.
const TOOLBAR_BORDER: f32 = 1.0;

/// The three-dots menu icon, drawn smaller so its edge-to-edge dots match the rest.
const MENU_ICON: f32 = BTN_SIZE - 3.0;

/// The capture button is larger than the tools and fills its whole slot.
const CAPTURE_BTN_SIZE: f32 = crate::widget::capture_button::SIZE;
/// Fixed toolbar height, so swapping sections never resizes the bar.
const TOOLBAR_H: f32 = BTN_SIZE + (BTN_PADDING as f32) * 2.0;

/// Every button occupies the same square, whatever is drawn inside it, or the
/// row's height would depend on which one is showing.
const _: () = {
    assert!(CAPTURE_BTN_SIZE + (CAPTURE_PADDING as f32) * 2.0 == TOOLBAR_H);
    assert!(DELAY_FACE_W >= BTN_SIZE + DELAY_LABEL_GAP as f32 + (DELAY_LABEL_TEXT as f32) * 1.2);
    assert!(CAPTURE_BTN_SIZE > BTN_SIZE * 1.5);
};
/// Padding around the capture button. `CAPTURE_BTN_SIZE` plus twice this is `TOOLBAR_H`.
const CAPTURE_PADDING: u16 = 4;

/// Padding around every toolbar button. With `BTN_SIZE` this makes each one `TOOLBAR_H` square.
const BTN_PADDING: u16 = 14;

/// Zero-width strut holding the row at [`TOOLBAR_H`] without stretching the buttons.
fn toolbar_strut<'a, Msg: 'a>() -> Element<'a, Msg> {
    cosmic::iced::widget::space()
        .width(Length::Fixed(0.0))
        .height(Length::Fixed(TOOLBAR_H))
        .into()
}
/// The chevron on a dropdown trigger. Smaller than the tool icon it follows,
/// so it reads as a menu indicator.
const CHEVRON: f32 = 12.0;
/// Horizontal padding on a dropdown trigger. Less than `BTN_PADDING` since the chevron adds width.
const DROPDOWN_PADDING: u16 = 8;

/// Width of the elapsed readout. Fits `00:00` through `999:59`.
const ELAPSED_FACE_W: f32 = 40.0;

/// Width of a value face, fixed so the toolbar does not shift as a number changes.
const VALUE_FACE_W: f32 = 34.0;

/// Gap between buttons inside a group.
const BTN_GAP: u16 = 2;

/// Gap either side of a section divider, which does want to read as a break.
const SECTION_GAP: u16 = 8;

/// A divider carrying the section gap, so groups stay close-packed but read as separate.
fn group_divider<'a, Msg: 'a>() -> Element<'a, Msg> {
    cosmic::widget::container(
        cosmic::widget::divider::vertical::default().height(Length::Fixed(DIVIDER_H)),
    )
    .padding([0, SECTION_GAP - BTN_GAP])
    .into()
}

/// Text size of the seconds on the armed delay button.
const DELAY_LABEL_TEXT: u16 = 11;
/// Width of the delay face, fixed whether or not a delay is armed.
const DELAY_FACE_W: f32 = 33.0;
/// Gap between the clock and the seconds it is counting.
const DELAY_LABEL_GAP: u16 = 3;

/// Color swatch diameter in the annotation section.
const SWATCH: f32 = 16.0;
/// The slot a swatch sits in. The selection ring is drawn at its edge.
const SWATCH_SLOT: f32 = 24.0;
/// Width of the selection ring.
const RING_WIDTH: f32 = 2.0;

/// The translucent fill between a chosen swatch and its ring.
const fn ring_fill() -> Color {
    Color::from_rgba(
        0x4d as f32 / 255.0,
        0x4d as f32 / 255.0,
        0x4d as f32 / 255.0,
        0.3,
    )
}

/// A filled color disc the size of a swatch.
fn swatch_disc<'a, Msg: 'a>(fill: Color) -> Element<'a, Msg> {
    cosmic::widget::container(
        cosmic::iced::widget::space()
            .width(Length::Fixed(SWATCH))
            .height(Length::Fixed(SWATCH)),
    )
    .class(cosmic::theme::Container::Custom(Box::new(move |_theme| {
        cosmic::iced::widget::container::Style {
            background: Some(Background::Color(fill)),
            border: Border {
                radius: (SWATCH / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })))
    .into()
}

/// `content` centered in a swatch slot.
fn centered<'a, Msg: 'a>(content: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    cosmic::widget::container(content)
        .width(Length::Fixed(SWATCH_SLOT))
        .height(Length::Fixed(SWATCH_SLOT))
        .align_x(cosmic::iced::core::alignment::Horizontal::Center)
        .align_y(cosmic::iced::core::alignment::Vertical::Center)
        .into()
}

/// The slot's disc: translucent, with a solid ring in `ring` when given.
fn ring_disc<'a, Msg: 'a>(ring: Option<Color>) -> Element<'a, Msg> {
    cosmic::widget::container(
        cosmic::iced::widget::space()
            .width(Length::Fixed(SWATCH_SLOT))
            .height(Length::Fixed(SWATCH_SLOT)),
    )
    .class(cosmic::theme::Container::Custom(Box::new(move |_theme| {
        cosmic::iced::widget::container::Style {
            background: Some(Background::Color(ring_fill())),
            border: Border {
                radius: (SWATCH_SLOT / 2.0).into(),
                width: if ring.is_some() { RING_WIDTH } else { 0.0 },
                color: ring.unwrap_or(Color::TRANSPARENT),
            },
            ..Default::default()
        }
    })))
    .into()
}

/// A swatch in its slot, ringed in `selected` when it is the chosen color.
fn swatch_slot<'a, Msg: 'a>(face: Element<'a, Msg>, selected: Option<Color>) -> Element<'a, Msg> {
    match selected {
        Some(accent) => cosmic::iced::widget::stack![ring_disc(Some(accent)), centered(face)]
            .width(Length::Fixed(SWATCH_SLOT))
            .height(Length::Fixed(SWATCH_SLOT))
            .into(),
        None => centered(face),
    }
}

/// Helper to create an SVG icon at a fixed pixel size
fn toolbar_icon(icon_name: &str, size: u16) -> cosmic::iced::widget::svg::Svg<'_, cosmic::Theme> {
    let icon_handle = icon::Icon::from(icon::from_name(icon_name).size(size))
        .into_svg_handle()
        .expect("Icon should be SVG");

    cosmic::iced::widget::svg::Svg::new(icon_handle).symbolic(true)
}

/// Helper to create an SVG icon tinted with a custom color
pub fn toolbar_icon_colored(
    icon_name: &str,
    size: u16,
    color: Color,
) -> cosmic::iced::widget::svg::Svg<'static, cosmic::Theme> {
    let icon_handle = icon::Icon::from(icon::from_name(icon_name).size(size))
        .into_svg_handle()
        .expect("Icon should be SVG");

    cosmic::iced::widget::svg::Svg::new(icon_handle)
        .symbolic(true)
        .class(cosmic::theme::Svg::Custom(Rc::new(move |_theme| {
            cosmic::iced::widget::svg::Style { color: Some(color) }
        })))
}

use crate::config::{Container, SaveLocationChoice, ShapeTool, VideoSaveLocationChoice};
use crate::fl;
use crate::geometry::{Choice, DragState, Rect};
use crate::widget::capture_button::{Mode as CaptureMode, capture_button};
use crate::widget::collapse::Collapse;
use viewer_tools::annotate::{AnnotateColor, STROKE_PRESETS_PX};
use viewer_widgets::dashed_shape::DashedBorder;

/// Inputs for the annotation section of the toolbar.
pub struct AnnotateSection<'a, Msg> {
    /// Whether the toolbar is showing the annotation tools
    pub active: bool,
    /// What each shared slot is set to. The slots keep their own settings, so
    /// picking a highlighter does not reset the shapes slot to a rectangle.
    pub shape_choice: ShapeTool,
    pub freehand_choice: ShapeTool,
    /// The tool actually drawing, if any. `tool` is only the last one chosen.
    pub active_tool: Option<ShapeTool>,
    pub color: AnnotateColor,
    pub thickness: f32,
    pub highlighter_thickness: f32,
    pub pixelate_block_size: f32,
    /// Magnifier zoom factor, edited by the same dropdown as the thicknesses
    pub magnification: f32,
    pub shape_popup_open: bool,
    /// Whether the pen/highlighter dropdown is open
    pub freehand_popup_open: bool,
    /// Text: the font, size and style popup
    pub text_format_popup_open: bool,
    /// Font family, size and format controls, built by the caller: `dropdown` wants
    /// a `Send + Sync` closure this generic module cannot provide.
    pub font_family_picker: Element<'a, Msg>,
    pub font_size_picker: Element<'a, Msg>,
    pub text_style_control: Element<'a, Msg>,
    pub text_align_control: Element<'a, Msg>,
    pub on_text_format_popup: Msg,
    pub stroke_popup_open: bool,
    /// Whether there is anything to take back, and anything to put back
    pub can_undo: bool,
    pub can_redo: bool,
    pub on_undo: Msg,
    pub on_redo: Msg,
    /// Whether clicking picks annotations up instead of drawing
    pub move_mode: bool,
    /// Whether there is anything on the capture that could be picked up
    pub can_move: bool,
    pub on_move: Msg,
    /// How far the delete button is out, 0 when nothing is picked out.
    pub delete_progress: f32,
    pub on_delete_selected: Msg,
    pub on_toggle: Msg,
    pub on_shape_popup: Msg,
    pub on_freehand_popup: Msg,
    pub on_stroke_popup: Msg,
    pub on_select_tool: Box<dyn Fn(ShapeTool) -> Msg + 'a>,
    pub on_select_color: Box<dyn Fn(AnnotateColor) -> Msg + 'a>,
    pub on_select_thickness: Box<dyn Fn(f32) -> Msg + 'a>,
    /// Color applied by the custom picker, if any
    pub custom_color: Option<Color>,
    /// The picker panel, built by the caller for the same reason. `Some` only while open.
    pub color_picker_panel: Option<Element<'a, Msg>>,
    pub on_color_picker_toggle: Msg,
}

/// Chrome for a dropdown panel: opaque background, hairline border, rounded to
/// match the toolbar.
fn popup_panel<'a, Msg: Clone + 'static>(
    content: impl Into<Element<'a, Msg>>,
) -> cosmic::widget::container::Container<'a, Msg, cosmic::Theme, cosmic::Renderer> {
    // Swallow presses on the panel itself. A popover would otherwise close on them.
    cosmic::widget::container(cosmic::widget::mouse_area(content.into()))
        .padding(8)
        .class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let cosmic = theme.cosmic();
            let component = &cosmic.background(false).component;
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(component.base.into())),
                border: Border {
                    radius: cosmic.radius_s().map(|r| r + 1.0).into(),
                    width: 1.0,
                    color: component.divider.into(),
                },
                ..Default::default()
            }
        })))
}

/// A dropdown trigger: the current value's icon with a small chevron beside it.
fn dropdown_trigger<Msg: Clone + 'static>(
    icon_name: &str,
    selected: bool,
    on_press: Msg,
) -> cosmic::widget::Button<'_, Msg> {
    dropdown_trigger_maybe(icon_name, selected, Some(on_press))
}

/// A dropdown face: a tool icon at `BTN_SIZE`, then a chevron.
fn dropdown_face<'a, Msg: 'a>(icon_name: &'a str) -> Element<'a, Msg> {
    row![
        toolbar_icon(icon_name, ICON_PX)
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE)),
        toolbar_icon("pan-down-symbolic", 24)
            .width(Length::Fixed(CHEVRON))
            .height(Length::Fixed(CHEVRON)),
    ]
    .spacing(BTN_GAP)
    .align_y(cosmic::iced::core::Alignment::Center)
    .into()
}

/// A dropdown face showing the value it sets: the magnifier's zoom or pixelation's block size.
fn dropdown_value_face<'a, Msg: 'a>(label: String) -> Element<'a, Msg> {
    row![
        text::body(label)
            .size(DELAY_LABEL_TEXT)
            .align_x(cosmic::iced::core::alignment::Horizontal::Center)
            .align_y(cosmic::iced::core::alignment::Vertical::Center)
            .width(Length::Fixed(VALUE_FACE_W))
            .height(Length::Fixed(BTN_SIZE)),
        toolbar_icon("pan-down-symbolic", 24)
            .width(Length::Fixed(CHEVRON))
            .height(Length::Fixed(CHEVRON)),
    ]
    .spacing(BTN_GAP)
    .align_y(cosmic::iced::core::Alignment::Center)
    .into()
}

/// A value dropdown's trigger, dead when the armed tool has no number to set.
fn dropdown_value_trigger_maybe<'a, Msg: Clone + 'static>(
    label: String,
    selected: bool,
    on_press: Option<Msg>,
) -> cosmic::widget::Button<'a, Msg> {
    button::custom(dropdown_value_face(label))
        .selected(selected)
        .class(if selected {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .padding([BTN_PADDING, DROPDOWN_PADDING])
        .on_press_maybe(on_press)
}

/// A dropdown trigger, dead when the armed tool has nothing for it to set.
fn dropdown_trigger_maybe<Msg: Clone + 'static>(
    icon_name: &str,
    selected: bool,
    on_press: Option<Msg>,
) -> cosmic::widget::Button<'_, Msg> {
    button::custom(dropdown_face(icon_name))
        .selected(selected)
        .class(if selected {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        // Vertically the same as a plain button, so the row height does not
        // depend on which tool is armed.
        .padding([BTN_PADDING, DROPDOWN_PADDING])
        .on_press_maybe(on_press)
}

/// One row of a dropdown list: icon, label, and a check when it is the current value.
fn dropdown_row<Msg: Clone + 'static>(
    icon_name: Option<&str>,
    label: String,
    selected: bool,
    on_press: Msg,
) -> Element<'_, Msg> {
    let mut item = row![]
        .spacing(8)
        .align_y(cosmic::iced::core::Alignment::Center);
    if let Some(name) = icon_name {
        item = item.push(
            toolbar_icon(name, ICON_PX)
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0)),
        );
    }
    item = item.push(text::body(label).width(Length::Fill));
    item = item.push(if selected {
        Element::from(
            toolbar_icon("object-select-symbolic", ICON_PX)
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0)),
        )
    } else {
        Element::from(cosmic::iced::widget::space().width(Length::Fixed(16.0)))
    });

    button::custom(item)
        .class(cosmic::theme::Button::Icon)
        .width(Length::Fill)
        .padding(4)
        .on_press(on_press)
        .into()
}

/// Pixelation block sizes.
const PIXELATE_PRESETS: &[f32] = &[8.0, 12.0, 16.0, 24.0, 32.0, 48.0];
/// Magnifier zoom factors.
const MAGNIFIER_PRESETS: &[f32] = &[1.5, 2.0, 2.5, 3.0, 4.0, 6.0, 8.0, 10.0];

/// Build the annotation tools, dropdowns and color swatches.
fn build_annotate_row<'a, Msg: Clone + 'static>(
    annotate: AnnotateSection<'a, Msg>,
    space_xxs: u16,
) -> Element<'a, Msg> {
    let tool_btn = |tool: ShapeTool, msg: Msg| -> Element<'a, Msg> {
        let selected = annotate.active_tool == Some(tool);
        tooltip(
            button::custom(
                toolbar_icon(tool.icon_name(), ICON_PX)
                    .width(Length::Fixed(BTN_SIZE))
                    .height(Length::Fixed(BTN_SIZE)),
            )
            .selected(selected)
            .class(if selected {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Icon
            })
            .padding(BTN_PADDING)
            .on_press(msg),
            text::body(tool.label()),
            tooltip::Position::Bottom,
        )
        .into()
    };

    // Undo and redo lead the row, as in cosmic-viewer.
    let history_btn = |icon: &'static str, label: String, enabled: bool, msg: Msg| {
        tooltip(
            button::custom(
                toolbar_icon(icon, ICON_PX)
                    .width(Length::Fixed(BTN_SIZE))
                    .height(Length::Fixed(BTN_SIZE)),
            )
            .class(cosmic::theme::Button::Icon)
            .padding(BTN_PADDING)
            .on_press_maybe(enabled.then_some(msg)),
            text::body(label),
            tooltip::Position::Bottom,
        )
    };
    let btn_undo = history_btn(
        "edit-undo-symbolic",
        fl!("undo"),
        annotate.can_undo,
        annotate.on_undo.clone(),
    );
    let btn_redo = history_btn(
        "edit-redo-symbolic",
        fl!("redo"),
        annotate.can_redo,
        annotate.on_redo.clone(),
    );

    // Pen and highlighter share a slot, like the shapes do, to keep the
    // toolbar narrow.
    let freehand_tools = ShapeTool::FREEHAND;
    let shown_freehand = annotate.freehand_choice;
    let freehand_active = annotate
        .active_tool
        .is_some_and(|t| freehand_tools.contains(&t));
    let btn_freehand: Element<'a, Msg> = {
        let mut pop = cosmic::widget::popover(dropdown_trigger(
            shown_freehand.icon_name(),
            freehand_active,
            annotate.on_freehand_popup.clone(),
        ));
        if annotate.freehand_popup_open {
            let mut list = cosmic::iced::widget::column![].spacing(2);
            for &tool in freehand_tools {
                list = list.push(dropdown_row(
                    Some(tool.icon_name()),
                    tool.label(),
                    shown_freehand == tool,
                    (annotate.on_select_tool)(tool),
                ));
            }
            pop = pop
                .popup(popup_panel(list).width(Length::Fixed(180.0)))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(annotate.on_freehand_popup.clone());
        }
        pop.into()
    };
    let btn_pixelate = tool_btn(
        ShapeTool::Pixelate,
        (annotate.on_select_tool)(ShapeTool::Pixelate),
    );
    let btn_magnifier = tool_btn(
        ShapeTool::Magnifier,
        (annotate.on_select_tool)(ShapeTool::Magnifier),
    );
    let btn_text = tool_btn(ShapeTool::Text, (annotate.on_select_tool)(ShapeTool::Text));

    // Move is disabled, not hidden, so the buttons beside it do not shift.
    let btn_move = tooltip(
        button::custom(
            toolbar_icon("select-object-symbolic", ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .selected(annotate.move_mode)
        .class(if annotate.move_mode {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .padding(BTN_PADDING)
        .on_press_maybe(annotate.can_move.then_some(annotate.on_move.clone())),
        text::body(fl!("select-annotations")),
        tooltip::Position::Bottom,
    );

    // Eased in and out so a button never appears under the pointer between clicks.
    let btn_delete: Element<'_, Msg> = Collapse::new(
        gap_before(
            tooltip(
                button::custom(
                    toolbar_icon("user-trash-symbolic", ICON_PX)
                        .width(Length::Fixed(BTN_SIZE))
                        .height(Length::Fixed(BTN_SIZE)),
                )
                .class(cosmic::theme::Button::Icon)
                .padding(BTN_PADDING)
                .on_press(annotate.on_delete_selected.clone()),
                text::body(fl!("delete-annotation")),
                tooltip::Position::Bottom,
            ),
            BTN_GAP,
        ),
        annotate.delete_progress,
    )
    .into();

    // Shapes dropdown: the geometric tools share one slot, showing whichever
    // was picked last.
    let shape_tools = ShapeTool::SHAPES;
    let shown_shape = annotate.shape_choice;
    let shape_active = annotate
        .active_tool
        .is_some_and(|t| shape_tools.contains(&t));
    let mut shapes = cosmic::widget::popover(dropdown_trigger(
        shown_shape.icon_name(),
        shape_active,
        annotate.on_shape_popup.clone(),
    ));
    if annotate.shape_popup_open {
        let mut list = cosmic::iced::widget::column![].spacing(2);
        for &tool in shape_tools {
            list = list.push(dropdown_row(
                Some(tool.icon_name()),
                tool.label(),
                shown_shape == tool,
                (annotate.on_select_tool)(tool),
            ));
        }
        shapes = shapes
            .popup(popup_panel(list).width(Length::Fixed(180.0)))
            .position(cosmic::widget::popover::Position::Top)
            .on_close(annotate.on_shape_popup.clone());
    }

    // A filled shape has no outline to widen, so the width dropdown is disabled for it.
    let width_applies = annotate
        .active_tool
        .and_then(ShapeTool::shape_kind)
        .is_none_or(|k| !k.is_filled());
    // The size dropdown edits whichever number the armed tool has. The trigger shows it.
    let px: fn(f32) -> String = |v| format!("{}px", v as u32);
    let (presets, current, unit): (_, _, fn(f32) -> String) = match annotate.active_tool {
        Some(ShapeTool::Highlighter) => (
            STROKE_PRESETS_PX.as_slice(),
            annotate.highlighter_thickness,
            px,
        ),
        Some(ShapeTool::Pixelate) => (PIXELATE_PRESETS, annotate.pixelate_block_size, px),
        Some(ShapeTool::Magnifier) => (MAGNIFIER_PRESETS, annotate.magnification, |v| {
            format!("{v:.1}x")
        }),
        _ => (STROKE_PRESETS_PX.as_slice(), annotate.thickness, px),
    };
    let size_dropdown: Element<'a, Msg> = {
        let mut pop = cosmic::widget::popover(dropdown_value_trigger_maybe(
            unit(current),
            annotate.stroke_popup_open,
            width_applies.then(|| annotate.on_stroke_popup.clone()),
        ));
        if annotate.stroke_popup_open && width_applies {
            let mut list = cosmic::iced::widget::column![].spacing(2);
            for &size in presets {
                list = list.push(dropdown_row(
                    None,
                    unit(size),
                    (current - size).abs() < f32::EPSILON,
                    (annotate.on_select_thickness)(size),
                ));
            }
            pop = pop
                .popup(popup_panel(list).width(Length::Fixed(110.0)))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(annotate.on_stroke_popup.clone());
        }
        pop.into()
    };

    // Font, size and style, in the slot the stroke width uses for every other
    // tool. A label has no stroke, and a shape has no font.
    let text_armed = annotate.active_tool == Some(ShapeTool::Text);
    let text_format_dropdown: Element<'a, Msg> = {
        let trigger = tooltip(
            button::custom(
                row![
                    cosmic::widget::container(text::body("Aa"))
                        .width(Length::Fixed(BTN_SIZE))
                        .height(Length::Fixed(BTN_SIZE))
                        .align_x(cosmic::iced::core::alignment::Horizontal::Center)
                        .align_y(cosmic::iced::core::alignment::Vertical::Center),
                    toolbar_icon("pan-down-symbolic", 24)
                        .width(Length::Fixed(CHEVRON))
                        .height(Length::Fixed(CHEVRON)),
                ]
                .spacing(BTN_GAP)
                .align_y(cosmic::iced::core::Alignment::Center),
            )
            .class(if annotate.text_format_popup_open {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Icon
            })
            .padding([BTN_PADDING, DROPDOWN_PADDING])
            .on_press(annotate.on_text_format_popup.clone()),
            text::body(fl!("text-format")),
            tooltip::Position::Bottom,
        );

        let mut pop = cosmic::widget::popover(trigger);
        if annotate.text_format_popup_open {
            let font_row = row![annotate.font_family_picker, annotate.font_size_picker]
                .spacing(space_xxs)
                .align_y(cosmic::iced::core::Alignment::Center);

            let panel = cosmic::iced::widget::column![
                font_row,
                annotate.text_style_control,
                annotate.text_align_control,
            ]
            .spacing(space_xxs);
            pop = pop
                .popup(popup_panel(panel).width(Length::Shrink))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(annotate.on_text_format_popup.clone());
        }
        pop.into()
    };

    let accent: Color = cosmic::theme::active().cosmic().accent_color().into();
    let mut swatches = preset_swatches(annotate.color, &*annotate.on_select_color);

    // The custom color swatch: a dashed ring around a plus until a color is
    // applied. Then the color, marked like the presets but with a dashed ring.
    let custom_color = annotate.custom_color;
    let is_custom = !AnnotateColor::PRESETS.contains(&annotate.color);
    let custom_face: Element<'a, Msg> = if is_custom && let Some(c) = custom_color {
        cosmic::iced::widget::stack![
            ring_disc(None),
            cosmic::iced::widget::canvas(
                DashedBorder::circle(accent, RING_WIDTH).dash_pattern(8.0, 2.0)
            )
            .width(Length::Fixed(SWATCH_SLOT))
            .height(Length::Fixed(SWATCH_SLOT)),
            centered(swatch_disc(c)),
        ]
        .width(Length::Fixed(SWATCH_SLOT))
        .height(Length::Fixed(SWATCH_SLOT))
        .into()
    } else {
        let neutral: Color = cosmic::theme::active().cosmic().palette.neutral_5.into();
        let plus = cosmic::iced::widget::stack![
            cosmic::iced::widget::canvas(DashedBorder::circle(neutral, 1.0).dash_pattern(4.0, 2.0))
                .width(Length::Fixed(SWATCH))
                .height(Length::Fixed(SWATCH)),
            centered(
                toolbar_icon("list-add-symbolic", ICON_PX)
                    .width(Length::Fixed(12.0))
                    .height(Length::Fixed(12.0)),
            ),
        ]
        .width(Length::Fixed(SWATCH))
        .height(Length::Fixed(SWATCH));
        swatch_slot(plus.into(), None)
    };

    let picker_trigger = button::custom(custom_face)
        .padding(0)
        .class(cosmic::theme::Button::Icon)
        .on_press(annotate.on_color_picker_toggle.clone());

    let mut picker = cosmic::widget::popover(picker_trigger);
    if let Some(panel) = annotate.color_picker_panel {
        picker = picker
            .popup(popup_panel(panel).width(Length::Fixed(264.0)))
            .position(cosmic::widget::popover::Position::Top)
            .on_close(annotate.on_color_picker_toggle.clone());
    }
    swatches = swatches.push(Element::from(picker));

    row![
        btn_undo,
        btn_redo,
        group_divider(),
        btn_text,
        btn_freehand,
        btn_pixelate,
        btn_magnifier,
        Element::from(shapes),
        group_divider(),
        btn_move,
        btn_delete,
        group_divider(),
        if text_armed {
            text_format_dropdown
        } else {
            size_dropdown
        },
        group_divider(),
        swatches,
    ]
    .spacing(BTN_GAP)
    .align_y(cosmic::iced::core::Alignment::Center)
    .into()
}

/// Framerates offered for a recording.
pub const FRAMERATE_OPTIONS: &[u32] = &[24, 30, 60];

/// The custom-folder rows: the picker, plus a ticked row naming the folder while it is in use.
fn custom_folder_rows<'a, Msg: Clone + 'static>(
    path: &str,
    selected: bool,
    on_select: Msg,
    on_browse: Msg,
) -> Vec<Element<'a, Msg>> {
    let mut rows = Vec::new();

    if selected
        && let Some(folder) = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    {
        rows.push(menu_row(
            fl!("save-to-folder", folder = folder),
            true,
            on_select,
        ));
    }

    rows.push(menu_row(fl!("save-to-custom-folder"), false, on_browse));
    rows
}

/// What a container is called in the menu.
pub const fn container_name(container: Container) -> &'static str {
    match container {
        Container::Mp4 => "MP4",
        Container::Mkv => "MKV",
        Container::Webm => "WebM",
    }
}

/// Width of the control in a [`setting_row`], shared so the video settings line up.
const SETTING_CONTROL_WIDTH: f32 = 140.0;

/// A settings row: label left, control right. Reserves the check column like [`menu_row`].
fn setting_row<Msg: Clone + 'static>(label: String, control: Element<'_, Msg>) -> Element<'_, Msg> {
    cosmic::widget::container(
        row![
            cosmic::iced::widget::space().width(Length::Fixed(16.0)),
            text::body(label),
            cosmic::iced::widget::space().width(Length::Fill),
            cosmic::widget::container(control).width(Length::Fixed(SETTING_CONTROL_WIDTH)),
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::core::Alignment::Center)
        .spacing(8),
    )
    .width(Length::Fill)
    .padding([0, 8])
    .into()
}

/// A menu row: a check column and a label.
fn menu_row<'a, Msg: Clone + 'static>(
    label: String,
    checked: bool,
    on_press: Msg,
) -> Element<'a, Msg> {
    let accent: Color = cosmic::theme::active().cosmic().accent_color().into();

    let check: Element<'a, Msg> = if checked {
        toolbar_icon_colored("object-select-symbolic", ICON_PX, accent)
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(16.0))
            .into()
    } else {
        cosmic::iced::widget::space()
            .width(Length::Fixed(16.0))
            .into()
    };

    let item = row![check, text::body(label).width(Length::Fill)]
        .spacing(8)
        .align_y(cosmic::iced::core::Alignment::Center);

    button::custom(item)
        .class(cosmic::theme::Button::Icon)
        .width(Length::Fill)
        .padding([4, 8])
        .on_press(on_press)
        .into()
}

/// Horizontal rule between menu sections.
fn menu_divider<'a, Msg: 'a>() -> Element<'a, Msg> {
    cosmic::widget::divider::horizontal::default()
        .width(Length::Fill)
        .into()
}

/// Delays offered by the capture-delay dropdown.
const DELAY_PRESETS: &[u32] = &[0, 1, 2, 3, 5, 10, 20];

/// State and callbacks for the toolbar's settings menu.
pub struct SettingsMenu<'a, Msg> {
    pub open: bool,
    pub magnifier_enabled: bool,
    pub save_location: SaveLocationChoice,
    /// Where a custom folder points, so the row can be named after it
    pub custom_save_path: &'a str,
    pub video_custom_save_path: &'a str,
    /// Opens the folder picker
    pub on_browse_custom: Msg,
    pub on_browse_video_custom: Msg,
    pub video_save_location: VideoSaveLocationChoice,
    pub video_show_cursor: bool,
    pub video_region_border: bool,
    pub video_system_audio: bool,
    pub video_microphone: bool,
    /// Whether a screenshot includes the pointer.
    pub show_cursor: bool,
    pub is_default_portal: bool,
    pub recognize_qr_codes: bool,
    /// The three video dropdowns, built by the caller for the same reason as the font picker.
    pub encoder_picker: Element<'a, Msg>,
    pub format_picker: Element<'a, Msg>,
    pub framerate_picker: Element<'a, Msg>,
    pub on_toggle: Msg,
    pub on_magnifier: Msg,
    pub on_save_clipboard: Msg,
    pub on_show_cursor: Msg,
    pub on_region_border: Msg,
    pub on_system_audio: Msg,
    pub on_microphone: Msg,
    pub on_screenshot_cursor: Msg,
    pub on_default_portal: Msg,
    pub on_recognize_qr_codes: Msg,
    pub on_save_pictures: Msg,
    pub on_save_documents: Msg,
    pub on_save_screenshots: Msg,
    pub on_save_custom: Msg,
    pub on_video_location: Box<dyn Fn(VideoSaveLocationChoice) -> Msg + 'a>,
}

/// Build the settings menu contents for the current capture mode.
fn build_settings_menu<Msg: Clone + 'static>(
    settings: SettingsMenu<'_, Msg>,
    is_video_mode: bool,
) -> Element<'_, Msg> {
    let mut list = cosmic::iced::widget::column![].spacing(2);

    if is_video_mode {
        list = list.push(menu_row(
            fl!("show-cursor"),
            settings.video_show_cursor,
            settings.on_show_cursor.clone(),
        ));
        list = list.push(menu_row(
            fl!("show-region-border"),
            settings.video_region_border,
            settings.on_region_border.clone(),
        ));
        list = list.push(menu_row(
            fl!("system-audio"),
            settings.video_system_audio,
            settings.on_system_audio.clone(),
        ));
        list = list.push(menu_row(
            fl!("microphone"),
            settings.video_microphone,
            settings.on_microphone.clone(),
        ));

        list = list.push(menu_divider());
        for (label, choice) in [
            (
                fl!("save-to-screencasts"),
                VideoSaveLocationChoice::Screencasts,
            ),
            (fl!("save-to-videos"), VideoSaveLocationChoice::Videos),
            (fl!("save-to-documents"), VideoSaveLocationChoice::Documents),
        ] {
            list = list.push(menu_row(
                label,
                settings.video_save_location == choice,
                (settings.on_video_location)(choice),
            ));
        }
        for row in custom_folder_rows(
            settings.video_custom_save_path,
            settings.video_save_location == VideoSaveLocationChoice::Custom,
            (settings.on_video_location)(VideoSaveLocationChoice::Custom),
            settings.on_browse_video_custom.clone(),
        ) {
            list = list.push(row);
        }
        list = list.push(menu_divider());

        // Encoder, format and framerate. Each opens its options in place -
        // one at a time, so the menu never grows past what fits.

        list = list.push(setting_row(fl!("encoder"), settings.encoder_picker));
        list = list.push(setting_row(fl!("format"), settings.format_picker));
        list = list.push(setting_row(fl!("framerate"), settings.framerate_picker));
    } else {
        list = list.push(menu_row(
            fl!("selection-edge-magnification"),
            settings.magnifier_enabled,
            settings.on_magnifier.clone(),
        ));
        // Asked separately from the recording toolbar's cursor setting.
        list = list.push(menu_row(
            fl!("show-cursor"),
            settings.show_cursor,
            settings.on_screenshot_cursor.clone(),
        ));
        list = list.push(menu_divider());
        // One exclusive choice: clipboard only, or a folder (which also copies).
        list = list.push(menu_row(
            fl!("save-to-clipboard"),
            settings.save_location == SaveLocationChoice::Clipboard,
            settings.on_save_clipboard.clone(),
        ));
        for (label, choice, on_press) in [
            (
                fl!("save-to-screenshots"),
                SaveLocationChoice::Screenshots,
                &settings.on_save_screenshots,
            ),
            (
                fl!("save-to-pictures"),
                SaveLocationChoice::Pictures,
                &settings.on_save_pictures,
            ),
            (
                fl!("save-to-documents"),
                SaveLocationChoice::Documents,
                &settings.on_save_documents,
            ),
        ] {
            list = list.push(menu_row(
                label,
                settings.save_location == choice,
                on_press.clone(),
            ));
        }
        for row in custom_folder_rows(
            settings.custom_save_path,
            settings.save_location == SaveLocationChoice::Custom,
            settings.on_save_custom.clone(),
            settings.on_browse_custom.clone(),
        ) {
            list = list.push(row);
        }
    }

    // Screenshot mode only. A recording has no still image to scan.
    if !is_video_mode {
        list = list.push(menu_divider());
        list = list.push(menu_row(
            fl!("recognize-qr-codes"),
            settings.recognize_qr_codes,
            settings.on_recognize_qr_codes.clone(),
        ));
    }

    list = list.push(menu_divider());
    // Named for the state, since it is a ticked toggle like the other rows.
    list = list.push(menu_row(
        fl!("default-screenshot-app"),
        settings.is_default_portal,
        settings.on_default_portal.clone(),
    ));

    popup_panel(list).width(Length::Fixed(260.0)).into()
}

/// The toolbar grip. Embedded because the icon theme's `grip-lines-symbolic` is a different glyph.
fn grip_icon() -> cosmic::iced::widget::svg::Svg<'static, cosmic::Theme> {
    const GRIP: &[u8] = include_bytes!("../../data/icons/scalable/actions/grip-lines-symbolic.svg");

    cosmic::iced::widget::svg::Svg::new(cosmic::iced::widget::svg::Handle::from_memory(GRIP))
        .symbolic(true)
        .width(Length::Fixed(BTN_SIZE))
        .height(Length::Fixed(BTN_SIZE))
}

/// Screenshot mode icon, embedded for the same reason.
fn screen_capture_icon() -> cosmic::iced::widget::svg::Svg<'static, cosmic::Theme> {
    const SCREEN_CAPTURE: &[u8] =
        include_bytes!("../../data/icons/scalable/actions/screen-capture-symbolic.svg");

    cosmic::iced::widget::svg::Svg::new(cosmic::iced::widget::svg::Handle::from_memory(
        SCREEN_CAPTURE,
    ))
    .symbolic(true)
    .width(Length::Fixed(BTN_SIZE))
    .height(Length::Fixed(BTN_SIZE))
}

/// A run of buttons behind its own divider, shown `progress` of the way. The
/// divider collapses with it.
fn section<'a, Msg: Clone + 'static>(
    content: impl Into<Element<'a, Msg>>,
    progress: f32,
) -> Element<'a, Msg> {
    use cosmic::widget::divider::vertical;

    Collapse::new(
        row![
            vertical::default().height(Length::Fixed(DIVIDER_H)),
            content.into(),
        ]
        .spacing(SECTION_GAP)
        .padding([0, 0, 0, SECTION_GAP])
        .align_y(cosmic::iced::core::Alignment::Center),
        progress,
    )
    .into()
}

/// A section with no divider before it.
fn joined_section<'a, Msg: Clone + 'static>(
    content: impl Into<Element<'a, Msg>>,
    progress: f32,
) -> Element<'a, Msg> {
    Collapse::new(
        row![content.into()]
            .padding([0, 0, 0, SECTION_GAP])
            .align_y(cosmic::iced::core::Alignment::Center),
        progress,
    )
    .into()
}

/// A button carrying the gap that separates it from what precedes it.
fn gap_before<'a, Msg: 'a>(content: impl Into<Element<'a, Msg>>, space: u16) -> Element<'a, Msg> {
    cosmic::widget::container(content.into())
        .padding([0, 0, 0, space])
        .into()
}

/// A button carrying the gap that separates it from what follows it.
fn gap_after<'a, Msg: 'a>(content: impl Into<Element<'a, Msg>>, space: u16) -> Element<'a, Msg> {
    cosmic::widget::container(content.into())
        .padding([0, space, 0, 0])
        .into()
}

/// The toolbar's panel: opaque background, rounded to the theme.
fn toolbar_surface<'a, Msg: Clone + 'static>(
    content: impl Into<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    // No max_height: TOOLBAR_H excludes the row padding, so capping at it stretched the row.
    cosmic::widget::container(content)
        .class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(
                    theme.background(false).component.base.into(),
                )),
                text_color: Some(theme.background(false).component.on.into()),
                border: Border {
                    radius: theme.corner_radii.radius_s.into(),
                    // Hairline edge, so the bar ends against a light screenshot.
                    width: TOOLBAR_BORDER,
                    color: theme.background(false).component.divider.into(),
                },
                ..Default::default()
            }
        })))
        .into()
}

/// The drag grip, shared by the capture and recording toolbars.
fn grip_button<'a, Msg: Clone + 'static>(on_press: Msg, on_release: Msg) -> Element<'a, Msg> {
    cosmic::widget::mouse_area(tooltip(
        cosmic::widget::container(grip_icon())
            .padding([0, 0, 0, SECTION_GAP])
            .width(Length::Fixed(BTN_SIZE + f32::from(SECTION_GAP)))
            .height(Length::Fixed(TOOLBAR_H))
            .align_x(cosmic::iced::core::alignment::Horizontal::Center)
            .align_y(cosmic::iced::core::alignment::Vertical::Center),
        text::body(fl!("move-toolbar")),
        tooltip::Position::Bottom,
    ))
    .on_press(on_press)
    .on_release(on_release)
    .interaction(cosmic::iced::core::mouse::Interaction::Grab)
    .into()
}

/// The preset color swatches. The chosen one gets a ring around the swatch
/// with a translucent gap between them.
fn preset_swatches<'a, Msg: Clone + 'static>(
    current: AnnotateColor,
    on_select: &dyn Fn(AnnotateColor) -> Msg,
) -> cosmic::iced::widget::Row<'a, Msg, cosmic::Theme, cosmic::Renderer> {
    let accent: Color = cosmic::theme::active().cosmic().accent_color().into();
    let mut swatches = row![]
        .spacing(0)
        .align_y(cosmic::iced::core::Alignment::Center);
    for &preset in &AnnotateColor::PRESETS {
        swatches = swatches.push(
            button::custom(swatch_slot(
                swatch_disc(preset.into()),
                (preset == current).then_some(accent),
            ))
            .padding(0)
            .class(cosmic::theme::Button::Icon)
            .on_press(on_select(preset)),
        );
    }
    swatches
}

/// How long a freehand stroke lingers before fading, in seconds.
const FADE_PRESETS: &[f32] = &[1.0, 2.0, 3.0, 5.0, 10.0];

/// The microphone audio pill
const PILL_H: f32 = 32.0;
/// Vertical padding that keeps the pill at [`PILL_H`] around a [`BTN_SIZE`] icon.
const PILL_PAD_V: u16 = 8;
const _: () = assert!(PILL_PAD_V as f32 * 2.0 + BTN_SIZE == PILL_H);
const PILL_PAD_H: u16 = 10;
const PILL_GAP: u16 = 7;
/// Bar width, the gap between bars, and the strip's height, in pixels.
const BAR_W: f32 = 2.0;
const BAR_GAP: f32 = 1.0;
const BARS_H: f32 = 20.0;
const BARS_W: f32 = crate::recording::AUDIO_HISTORY as f32 * (BAR_W + BAR_GAP) - BAR_GAP;
/// Levels at or above these are loud (-20 dBFS) and too loud (-9 dBFS), on the gauge's 0 to 1 scale.
const LOUD: f32 = 45.0 / 60.0;
const TOO_LOUD: f32 = 55.0 / 60.0;
/// How faint the oldest bar is drawn.
const OLDEST_ALPHA: f32 = 0.2;

/// 0 good, 1 loud, 2 too loud.
fn zone(level: f32) -> usize {
    usize::from(level >= LOUD) + usize::from(level >= TOO_LOUD)
}

/// Green, yellow and red
/// TODO:
const fn meter_colors(dark: bool) -> [Color; 3] {
    if dark {
        [
            Color::from_rgb8(0x4c, 0xaf, 0x50),
            Color::from_rgb8(0xf2, 0xc2, 0x30),
            Color::from_rgb8(0xe0, 0x4a, 0x4a),
        ]
    } else {
        [
            Color::from_rgb8(0x2e, 0x7d, 0x32),
            Color::from_rgb8(0xb8, 0x86, 0x0b),
            Color::from_rgb8(0xc6, 0x28, 0x28),
        ]
    }
}

type PillStyle = Box<dyn Fn(bool, &cosmic::Theme) -> button::Style>;

fn pill_style(muted: bool, alpha: f32) -> PillStyle {
    Box::new(move |_focused, theme| {
        let cosmic = theme.cosmic();
        let on: Color = cosmic.background(false).component.on.into();
        let mut style = button::Style::new();
        style.background = Some(Background::Color(if muted {
            Color {
                a: 0.14,
                ..crate::widget::capture_button::RECORD_RED
            }
        } else {
            Color { a: alpha, ..on }
        }));
        style.border_radius = cosmic.radius_xl().into();
        style
    })
}

fn audio_pill<'a, Msg: Clone + 'static>(
    levels: Vec<f32>,
    muted: bool,
    on_press: Msg,
) -> Element<'a, Msg> {
    let icon: Element<'a, Msg> = if muted {
        toolbar_icon_colored(
            "audio-volume-muted-symbolic",
            ICON_PX,
            crate::widget::capture_button::RECORD_RED,
        )
        .width(Length::Fixed(BTN_SIZE))
        .height(Length::Fixed(BTN_SIZE))
        .into()
    } else {
        toolbar_icon("audio-volume-high-symbolic", ICON_PX)
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE))
            .into()
    };
    let bars = cosmic::iced::widget::canvas::Canvas::new(LevelBars { levels, muted })
        .width(Length::Fixed(BARS_W))
        .height(Length::Fixed(BARS_H));
    tooltip(
        button::custom(
            row![icon, bars]
                .spacing(PILL_GAP)
                .align_y(cosmic::iced::core::Alignment::Center),
        )
        .class(cosmic::theme::Button::Custom {
            active: pill_style(muted, 0.10),
            disabled: Box::new(move |theme| pill_style(muted, 0.10)(false, theme)),
            hovered: pill_style(muted, 0.16),
            pressed: pill_style(muted, 0.20),
        })
        .padding([PILL_PAD_V, PILL_PAD_H])
        .on_press(on_press),
        text::body(if muted {
            fl!("unmute-audio")
        } else {
            fl!("mute-audio")
        }),
        tooltip::Position::Bottom,
    )
    .into()
}

struct LevelBars {
    levels: Vec<f32>,
    muted: bool,
}

impl<Msg> cosmic::iced::widget::canvas::Program<Msg, cosmic::Theme, cosmic::Renderer>
    for LevelBars
{
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        theme: &cosmic::Theme,
        bounds: cosmic::iced::core::Rectangle,
        _cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<cosmic::iced::widget::canvas::Geometry> {
        use cosmic::iced::core::{Point, Size};
        use cosmic::iced::widget::canvas::{Frame, Path};

        let cosmic = theme.cosmic();
        let colors = meter_colors(cosmic.is_dark);
        let on: Color = cosmic.background(false).component.on.into();
        let mut frame = Frame::new(renderer, bounds.size());
        let n = crate::recording::AUDIO_HISTORY;
        for i in 0..n {
            let level = self.levels.get(i).copied().unwrap_or(0.0);
            // Older bars fade, so the strip reads left to right as time.
            let age = (n - 1 - i) as f32 / (n - 1) as f32;
            let alpha = (1.0 - OLDEST_ALPHA).mul_add(-age, 1.0);
            let color = if self.muted {
                Color {
                    a: 0.32 * alpha,
                    ..on
                }
            } else {
                Color {
                    a: alpha,
                    ..colors[zone(level)]
                }
            };
            // Never shorter than a dot, so silence shows as a row of dots.
            let h = (level * BARS_H).max(BAR_W);
            let bar = Path::rounded_rectangle(
                Point::new(i as f32 * (BAR_W + BAR_GAP), (BARS_H - h) / 2.0),
                Size::new(BAR_W, h),
                (BAR_W / 2.0).into(),
            );
            frame.fill(&bar, color);
        }
        vec![frame.into_geometry()]
    }
}

/// Elapsed readout, `mm:ss` with minutes unwrapped, at a fixed width.
fn elapsed_face<'a, Msg: 'a>(elapsed: Duration) -> Element<'a, Msg> {
    let secs = elapsed.as_secs();
    text::body(format!("{:02}:{:02}", secs / 60, secs % 60))
        .size(DELAY_LABEL_TEXT)
        .align_x(cosmic::iced::core::alignment::Horizontal::Center)
        .align_y(cosmic::iced::core::alignment::Vertical::Center)
        .width(Length::Fixed(ELAPSED_FACE_W))
        .height(Length::Fixed(BTN_SIZE))
        .into()
}

/// The delay face: the clock, and the seconds beside it once armed. Shared with the countdown.
fn delay_face<'a, Msg: 'a>(secs: u32) -> Element<'a, Msg> {
    let mut face = row![
        toolbar_icon("delay-symbolic", ICON_PX)
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE))
    ]
    .spacing(DELAY_LABEL_GAP)
    .align_y(cosmic::iced::core::Alignment::Center);
    if secs > 0 {
        face = face.push(
            text::body(fl!("delay-armed", secs = secs))
                .size(DELAY_LABEL_TEXT)
                // The number's own line box is taller than the icon, so
                // centring the row is not enough to line the two up.
                .height(Length::Fixed(BTN_SIZE))
                .align_y(cosmic::iced::core::alignment::Vertical::Center),
        );
    }
    cosmic::widget::container(face)
        .width(Length::Fixed(DELAY_FACE_W))
        .height(Length::Fixed(BTN_SIZE))
        .align_x(cosmic::iced::core::alignment::Horizontal::Center)
        .align_y(cosmic::iced::core::alignment::Vertical::Center)
        .into()
}

/// The delay face as a label, where there is nothing to press.
fn delay_label<'a, Msg: 'a>(secs: u32) -> Element<'a, Msg> {
    cosmic::widget::container(delay_face(secs))
        .padding(BTN_PADDING)
        .into()
}

/// The X at the end of the toolbar. The tooltip says what it cancels.
fn close_button<'a, Msg: Clone + 'static>(on_press: Msg, tip: String) -> Element<'a, Msg> {
    tool_button(
        toolbar_icon("window-close-symbolic", ICON_PX),
        false,
        Some(on_press),
        tip,
    )
}

/// A toolbar button: an icon in the standard square, lit while `selected`.
fn tool_button<Msg: Clone + 'static>(
    icon: cosmic::iced::widget::svg::Svg<'_, cosmic::Theme>,
    selected: bool,
    on_press: Option<Msg>,
    tip: String,
) -> Element<'_, Msg> {
    tooltip(
        button::custom(
            icon.width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .selected(selected)
        .class(if selected {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .padding(BTN_PADDING)
        .on_press_maybe(on_press),
        text::body(tip),
        tooltip::Position::Bottom,
    )
    .into()
}

/// Screenshot or video.
fn mode_buttons<'a, Msg: Clone + 'static>(
    is_video: bool,
    on_photo: Msg,
    on_video: Msg,
) -> Element<'a, Msg> {
    row![
        tool_button(
            screen_capture_icon(),
            !is_video,
            Some(on_photo),
            fl!("screenshot-mode")
        ),
        tool_button(
            toolbar_icon("camera-video-symbolic", ICON_PX),
            is_video,
            Some(on_video),
            fl!("video-mode"),
        ),
    ]
    .spacing(BTN_GAP)
    .align_y(cosmic::iced::core::Alignment::Center)
    .into()
}

/// Region, screen, all screens: narrowest target to widest.
fn target_buttons<'a, Msg: Clone + 'static>(
    choice: &Choice,
    on_region: Msg,
    on_screen: Msg,
    on_all_screens: Option<Msg>,
) -> Element<'a, Msg> {
    let all_screens = on_all_screens.map(|msg| {
        tool_button(
            toolbar_icon("screenshot-all-screens-symbolic", ICON_PX),
            matches!(choice, Choice::AllScreens),
            Some(msg),
            fl!("select-all-screens"),
        )
    });
    row![
        tool_button(
            toolbar_icon("screenshot-selection-symbolic", ICON_PX),
            matches!(choice, Choice::Rectangle(..)),
            Some(on_region),
            fl!("select-region"),
        ),
        tool_button(
            toolbar_icon("screenshot-screen-symbolic", ICON_PX),
            matches!(choice, Choice::Output(..)),
            Some(on_screen),
            fl!("select-screen"),
        ),
    ]
    .push_maybe(all_screens)
    .spacing(BTN_GAP)
    .align_y(cosmic::iced::core::Alignment::Center)
    .into()
}

/// The annotate button, which toggles the drawing tools, and OCR beside it,
/// since both act on a selection. OCR has two faces: read, then copy.
fn tools_buttons<'a, Msg: Clone + 'static>(
    annotating: bool,
    on_annotate: Option<Msg>,
    has_ocr_text: bool,
    on_ocr: Msg,
    on_ocr_copy: Msg,
    ocr_progress: f32,
) -> Element<'a, Msg> {
    let ocr = if has_ocr_text {
        tool_button(
            toolbar_icon("edit-copy-symbolic", ICON_PX),
            true,
            Some(on_ocr_copy),
            fl!("copy-ocr-text"),
        )
    } else {
        tool_button(
            toolbar_icon("recognize-text-symbolic", ICON_PX),
            false,
            Some(on_ocr),
            fl!("recognize-text"),
        )
    };
    row![
        tool_button(
            toolbar_icon("markup-symbolic", ICON_PX),
            annotating,
            on_annotate,
            fl!("annotate"),
        ),
        Element::from(Collapse::new(gap_before(ocr, BTN_GAP), ocr_progress)),
    ]
    .align_y(cosmic::iced::core::Alignment::Center)
    .into()
}

/// The three dots that open the settings menu.
fn settings_button<'a, Msg: Clone + 'static>(open: bool, on_press: Msg) -> Element<'a, Msg> {
    tooltip(
        button::custom(
            cosmic::widget::container(
                toolbar_icon("view-more-symbolic", ICON_PX)
                    .width(Length::Fixed(MENU_ICON))
                    .height(Length::Fixed(MENU_ICON)),
            )
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE))
            .align_x(cosmic::iced::core::alignment::Horizontal::Center)
            .align_y(cosmic::iced::core::alignment::Vertical::Center),
        )
        .class(if open {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_press)
        .padding(BTN_PADDING),
        text::body(fl!("settings")),
        tooltip::Position::Bottom,
    )
    .into()
}

/// Distance between an unmoved toolbar and the bottom of its output.
pub const BOTTOM_MARGIN: f32 = 32.0;

/// Top-left corner of a toolbar of `size` on an output of `output` size: `pos`
/// if it was moved there, kept on the output, otherwise centered along the bottom.
#[must_use]
pub fn toolbar_origin(pos: Option<(f32, f32)>, size: (f32, f32), output: (f32, f32)) -> (f32, f32) {
    let room = ((output.0 - size.0).max(0.0), (output.1 - size.1).max(0.0));
    pos.map_or_else(
        || (room.0 / 2.0, (room.1 - BOTTOM_MARGIN).max(0.0)),
        |(x, y)| (x.clamp(0.0, room.0), y.clamp(0.0, room.1)),
    )
}

/// A toolbar on a full-output surface: at `pos`, or centered along the bottom.
/// `on_resize` reports the size it is laid out at, which the surface's input
/// zone has to cover.
pub fn place_toolbar<'a, Msg: 'a>(
    toolbar: Element<'a, Msg>,
    pos: Option<(f32, f32)>,
    on_resize: impl Fn(f32, f32) -> Msg + 'a,
) -> Element<'a, Msg> {
    use cosmic::iced::widget::{column, space};

    let toolbar =
        cosmic::iced::widget::sensor(toolbar).on_resize(move |s| on_resize(s.width, s.height));
    match pos {
        Some((x, y)) => column![
            space().height(Length::Fixed(y)),
            row![
                space().width(Length::Fixed(x)),
                toolbar,
                space().width(Length::Fill)
            ]
            .width(Length::Fill),
            space().height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        // Aligned rather than placed, so it stays centered while its sections
        // open and close.
        None => cosmic::widget::container(toolbar)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(cosmic::iced::core::alignment::Horizontal::Center)
            .align_y(cosmic::iced::core::alignment::Vertical::Bottom)
            .padding(cosmic::iced::Padding::ZERO.bottom(BOTTOM_MARGIN))
            .into(),
    }
}

/// What the toolbar is doing. Every mode lays out the same row of sections and
/// collapses the ones it does not use, so changing mode slides buttons in and
/// out of one toolbar.
pub enum ToolbarMode<'a, Msg> {
    /// Choosing what to capture, and annotating it.
    Capture(CaptureToolbar<'a, Msg>),
    /// Waiting out a capture delay.
    Countdown(CountdownToolbar<Msg>),
    /// Recording, with the live pen.
    Recording(RecordingToolbar<'a, Msg>),
}

/// The capture toolbar as it was when a countdown or recording took over. Its
/// sections close from there, so the change of mode animates.
#[derive(Clone, Debug)]
pub struct Handoff {
    /// 0 as the new mode starts, 1 once the outgoing sections have closed.
    pub transition: f32,
    pub outgoing: crate::capture::ToolbarProgress,
    pub outgoing_choice: Choice,
    /// Whether the target section carried the all-screens button.
    pub all_screens: bool,
}

impl Handoff {
    const fn transition(&self) -> f32 {
        self.transition.clamp(0.0, 1.0)
    }

    /// How far an outgoing section that was `progress` open still is.
    fn departing(&self, progress: f32) -> f32 {
        progress * (1.0 - self.transition())
    }
}

/// State and callbacks for the capture toolbar.
pub struct CaptureToolbar<'a, Msg> {
    pub choice: Choice,
    pub has_selection: bool,
    pub space_s: u16,
    pub space_xxs: u16,
    pub on_choice_change: Box<dyn Fn(Choice) -> Msg + 'a>,
    pub on_screen_mode: Msg,
    pub on_save_to_pictures: Msg,
    pub on_delayed_capture: Msg,
    pub on_delay_popup: Msg,
    pub on_select_delay: Box<dyn Fn(u32) -> Msg + 'a>,
    pub delay_popup_open: bool,
    pub capture_delay_secs: u32,
    /// Decided by the session so the toolbar and the button agree.
    pub delayed: bool,
    pub on_record_region: Msg,
    pub on_cancel: Msg,
    pub on_drag_start: Msg,
    pub on_drag_end: Msg,
    pub output_count: usize,
    /// Whether OCR has produced text that is waiting to be copied.
    pub has_ocr_text: bool,
    pub on_ocr: Msg,
    pub on_ocr_copy: Msg,
    pub is_video_mode: bool,
    pub is_recording: bool,
    /// How far each varying section is open.
    pub progress: crate::capture::ToolbarProgress,
    pub on_capture_mode_photo: Msg,
    pub on_capture_mode_video: Msg,
    pub annotate: AnnotateSection<'a, Msg>,
    pub settings: SettingsMenu<'a, Msg>,
}

/// State and callbacks for the toolbar while a delayed capture waits.
pub struct CountdownToolbar<Msg> {
    pub handoff: Handoff,
    /// When the wait began and how long it runs. Handed to the shutter, which
    /// closes its ring against the same clock the capture fires on.
    pub started: Instant,
    pub duration: Duration,
    /// Whole seconds left, for the delay label.
    pub remaining: u32,
    pub on_drag_start: Msg,
    pub on_drag_end: Msg,
    /// Harmless action for the outgoing controls during their brief exit.
    pub on_transition_noop: Msg,
    pub on_cancel: Msg,
}

/// State and callbacks for the toolbar while recording.
pub struct RecordingToolbar<'a, Msg> {
    pub handoff: Handoff,
    /// How long the recording has been running.
    pub elapsed: Duration,
    pub pencil_active: bool,
    pub color: AnnotateColor,
    pub fade_duration: f32,
    pub fade_popup_open: bool,
    pub thickness: f32,
    pub thickness_popup_open: bool,
    pub audio_muted: Option<bool>,
    pub on_mute_toggle: Msg,
    pub audio_levels: Vec<f32>,
    pub on_drag_start: Msg,
    pub on_drag_end: Msg,
    /// Harmless action for the outgoing controls during their brief exit.
    pub on_transition_noop: Msg,
    pub on_pencil_toggle: Msg,
    pub on_stop: Msg,
    pub on_fade_popup: Msg,
    pub on_select_color: Box<dyn Fn(AnnotateColor) -> Msg + 'a>,
    pub on_select_fade: Box<dyn Fn(f32) -> Msg + 'a>,
    pub on_thickness_popup: Msg,
    pub on_select_thickness: Box<dyn Fn(f32) -> Msg + 'a>,
}

/// The toolbar's sections in order, each with how far it is open. `None` is a
/// section the mode never shows.
struct Sections<'a, Msg> {
    on_drag_start: Msg,
    on_drag_end: Msg,
    capture_mode: Option<(Element<'a, Msg>, f32)>,
    target: Option<(Element<'a, Msg>, f32)>,
    tools: Option<(Element<'a, Msg>, f32)>,
    annotate_row: Option<(Element<'a, Msg>, f32)>,
    pencil: Option<(Element<'a, Msg>, f32)>,
    pen_settings: Option<(Element<'a, Msg>, f32)>,
    /// Beside the stop button, because the two answer the same question: how
    /// long has this been going, and how do I end it.
    elapsed: Option<(Element<'a, Msg>, f32)>,
    /// The delay, which collapses, then the capture button it belongs to.
    delay: (Element<'a, Msg>, f32),
    capture: Element<'a, Msg>,
    /// The settings menu, which collapses, then the X.
    settings: (Element<'a, Msg>, f32),
    close: Element<'a, Msg>,
    /// How far the settings and the X are open together.
    end: f32,
    space_s: u16,
    space_xxs: u16,
}

/// Build the toolbar for `mode`.
pub fn build_toolbar<'a, Msg: Clone + 'static>(mode: ToolbarMode<'a, Msg>) -> Element<'a, Msg> {
    let s = match mode {
        ToolbarMode::Capture(t) => capture_sections(t),
        ToolbarMode::Countdown(t) => countdown_sections(t),
        ToolbarMode::Recording(t) => recording_sections(t),
    };

    // No spacing on the outer row: each section carries its own leading gap and
    // divider, so a collapsed one leaves no stray gap.
    let shown =
        |s: Option<(Element<'a, Msg>, f32)>| s.map(|(content, open)| section(content, open));
    let body = row![toolbar_strut(), grip_button(s.on_drag_start, s.on_drag_end)]
        .push_maybe(shown(s.capture_mode))
        .push_maybe(shown(s.target))
        .push_maybe(shown(s.tools))
        .push_maybe(shown(s.annotate_row))
        .push_maybe(shown(s.pencil))
        .push_maybe(shown(s.pen_settings))
        .push_maybe(shown(s.elapsed))
        .push(section(
            row![
                Element::from(Collapse::new(gap_after(s.delay.0, s.space_s), s.delay.1)),
                s.capture,
            ]
            .align_y(cosmic::iced::core::Alignment::Center),
            1.0,
        ))
        .push(joined_section(
            row![
                Element::from(Collapse::new(
                    gap_after(s.settings.0, BTN_GAP),
                    s.settings.1
                )),
                s.close,
            ]
            .align_y(cosmic::iced::core::Alignment::Center),
            s.end,
        ))
        .align_y(cosmic::iced::core::Alignment::Center)
        .spacing(0)
        .padding([s.space_xxs, SECTION_GAP, s.space_xxs, 0]);

    toolbar_surface(body)
}

fn capture_sections<Msg: Clone + 'static>(t: CaptureToolbar<'_, Msg>) -> Sections<'_, Msg> {
    let p = t.progress;

    // All-screens is its own button. No choice no longer means every screen.
    let save_tooltip = match &t.choice {
        Choice::Rectangle(r, _) if r.dimensions().is_some() => fl!("save-selected-region"),
        Choice::Output(Some(_)) => fl!("save-selected-screen"),
        Choice::AllScreens => fl!("save-all-screens"),
        _ => fl!("select-something-first"),
    };

    // One button captures to the configured folder and the clipboard. Only the
    // clipboard row skips the file.
    //
    // The same button in both modes, at the same size.
    let (capture_action, capture_tooltip) = if t.is_video_mode {
        (
            t.has_selection.then_some(t.on_record_region),
            if t.has_selection {
                fl!("record-selection")
            } else {
                fl!("record-disabled")
            },
        )
    } else {
        (
            // The delay dropdown sets the delay and the capture button applies it.
            t.has_selection.then(|| {
                // A delayed capture re-photographs the screen with the same selection.
                if t.delayed {
                    t.on_delayed_capture.clone()
                } else {
                    t.on_save_to_pictures.clone()
                }
            }),
            if t.delayed && t.has_selection {
                fl!("capture-after-delay", secs = t.capture_delay_secs)
            } else {
                save_tooltip
            },
        )
    };

    let btn_capture = tooltip(
        cosmic::widget::container(
            capture_button(CaptureMode::of(t.is_video_mode, t.is_recording))
                .on_press_maybe(capture_action),
        )
        .padding(CAPTURE_PADDING),
        text::body(capture_tooltip),
        tooltip::Position::Bottom,
    )
    .into();

    // The delay button opens the delay menu. An armed delay shows its count on
    // the button, and is not highlighted: it is a setting, not a lit button.
    let btn_delay: Element<'_, Msg> = {
        let delay_set = t.capture_delay_secs > 0;
        let trigger = tooltip(
            button::custom(delay_face(t.capture_delay_secs))
                .class(if t.delay_popup_open {
                    cosmic::theme::Button::Suggested
                } else {
                    cosmic::theme::Button::Icon
                })
                .on_press(t.on_delay_popup.clone())
                .padding(BTN_PADDING),
            // A delay of zero means no delay. The button opens the menu to set one.
            text::body(if delay_set {
                fl!("delayed-screenshot", secs = t.capture_delay_secs)
            } else {
                fl!("delay-menu")
            }),
            tooltip::Position::Bottom,
        );

        let mut pop = cosmic::widget::popover(trigger);
        if t.delay_popup_open {
            let mut list = cosmic::iced::widget::column![].spacing(2);
            for secs in DELAY_PRESETS.iter().copied() {
                let label = if secs == 0 {
                    fl!("no-delay")
                } else {
                    fl!("delay-seconds", secs = secs)
                };
                list = list.push(menu_row(
                    label,
                    secs == t.capture_delay_secs,
                    (t.on_select_delay)(secs),
                ));
            }
            pop = pop
                .popup(popup_panel(list).width(Length::Fixed(150.0)))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(t.on_delay_popup);
        }
        pop.into()
    };

    // Settings menu: three dots opening a popover, matching the tool dropdowns
    let btn_settings: Element<'_, Msg> = {
        let mut pop = cosmic::widget::popover(settings_button(
            t.settings.open,
            t.settings.on_toggle.clone(),
        ));
        if t.settings.open {
            // The menu owns the three pickers, so it takes the whole struct.
            let on_close = t.settings.on_toggle.clone();
            pop = pop
                .popup(build_settings_menu(t.settings, t.is_video_mode))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(on_close);
        }
        pop.into()
    };

    // Annotation entry point. Pressing it swaps the middle of the toolbar for
    // the drawing tools. It stays visible and selected while they are up.
    let tools = tools_buttons(
        t.annotate.active,
        t.has_selection.then(|| t.annotate.on_toggle.clone()),
        t.has_ocr_text,
        t.on_ocr,
        t.on_ocr_copy,
        p.ocr,
    );

    Sections {
        on_drag_start: t.on_drag_start,
        on_drag_end: t.on_drag_end,
        capture_mode: Some((
            mode_buttons(
                t.is_video_mode,
                t.on_capture_mode_photo,
                t.on_capture_mode_video,
            ),
            p.capture_mode,
        )),
        target: Some((
            target_buttons(
                &t.choice,
                (t.on_choice_change)(Choice::Rectangle(Rect::default(), DragState::None)),
                t.on_screen_mode,
                // Only meaningful with more than one output. Hidden while
                // recording, since the recorder takes one region on one output.
                (t.output_count > 1 && !t.is_video_mode)
                    .then(|| (t.on_choice_change)(Choice::AllScreens)),
            ),
            p.target,
        )),
        tools: Some((tools, p.tools)),
        annotate_row: Some((build_annotate_row(t.annotate, t.space_xxs), p.annotate_row)),
        pencil: None,
        pen_settings: None,
        elapsed: None,
        delay: (btn_delay, p.delay),
        capture: btn_capture,
        settings: (btn_settings, 1.0),
        close: close_button(t.on_cancel, fl!("cancel-escape")),
        end: 1.0,
        space_s: t.space_s,
        space_xxs: t.space_xxs,
    }
}

/// The capture toolbar's choices close, inert, as the countdown takes over.
/// What stays is the delay, counting down, the shutter and the X.
fn countdown_sections<'a, Msg: Clone + 'static>(t: CountdownToolbar<Msg>) -> Sections<'a, Msg> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    let h = &t.handoff;
    let transition = h.transition();
    let noop = t.on_transition_noop;

    // The shutter, counting down. It is inert here. The X button cancels the countdown.
    let shutter = cosmic::widget::container(
        capture_button(CaptureMode::Counting {
            started: t.started,
            duration: t.duration,
        })
        .transition_from(CaptureMode::Photo, transition)
        // Keep the outgoing shutter at full strength on the first frame. The
        // message is deliberately inert, and disappears once the handoff ends.
        .on_press_maybe((transition < 1.0).then_some(noop.clone())),
    )
    .padding(CAPTURE_PADDING)
    .into();

    Sections {
        on_drag_start: t.on_drag_start,
        on_drag_end: t.on_drag_end,
        capture_mode: Some((
            mode_buttons(false, noop.clone(), noop.clone()),
            h.departing(h.outgoing.capture_mode),
        )),
        target: Some((
            target_buttons(
                &h.outgoing_choice,
                noop.clone(),
                noop.clone(),
                h.all_screens.then(|| noop.clone()),
            ),
            h.departing(h.outgoing.target),
        )),
        tools: Some((
            tools_buttons(
                false,
                Some(noop.clone()),
                false,
                noop.clone(),
                noop.clone(),
                h.outgoing.ocr,
            ),
            h.departing(h.outgoing.tools),
        )),
        annotate_row: None,
        pencil: None,
        pen_settings: None,
        elapsed: None,
        delay: (delay_label(t.remaining), 1.0),
        capture: shutter,
        settings: (settings_button(false, noop), 1.0 - transition),
        close: close_button(t.on_cancel, fl!("countdown-cancel")),
        end: 1.0,
        space_s: spacing.space_s,
        space_xxs: spacing.space_xxs,
    }
}

/// The capture toolbar's choices close, inert, as the recording takes over.
/// The pen, the elapsed time and the stop button open in their place.
fn recording_sections<'a, Msg: Clone + 'static>(t: RecordingToolbar<'a, Msg>) -> Sections<'a, Msg> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    let h = &t.handoff;
    let transition = h.transition();
    let noop = t.on_transition_noop;

    // The capture button widget in its recording state.
    let btn_stop = tooltip(
        cosmic::widget::container(
            capture_button(CaptureMode::Recording)
                .transition_from(CaptureMode::Ready, transition)
                .on_press_maybe(Some(t.on_stop)),
        )
        .padding(CAPTURE_PADDING),
        text::body(fl!("stop-recording")),
        tooltip::Position::Bottom,
    )
    .into();

    // Pen settings appear with the pen, as they do in the annotation section.
    let pen_settings: Option<Element<'_, Msg>> = t.pencil_active.then(|| {
        // How long a stroke stays visible before it fades.
        let mut fade = cosmic::widget::popover(tooltip(
            dropdown_trigger(
                "pencil-fade-symbolic",
                t.fade_popup_open,
                t.on_fade_popup.clone(),
            ),
            text::body(fl!("fade-duration", duration = (t.fade_duration as u32))),
            tooltip::Position::Bottom,
        ));
        if t.fade_popup_open {
            let mut list = cosmic::iced::widget::column![].spacing(2);
            for &secs in FADE_PRESETS {
                list = list.push(dropdown_row(
                    None,
                    fl!("fade-duration", duration = (secs as u32)),
                    (t.fade_duration - secs).abs() < f32::EPSILON,
                    (t.on_select_fade)(secs),
                ));
            }
            fade = fade
                .popup(popup_panel(list).width(Length::Fixed(150.0)))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(t.on_fade_popup.clone());
        }

        let mut thickness = cosmic::widget::popover(dropdown_value_trigger_maybe(
            format!("{}px", t.thickness as u32),
            t.thickness_popup_open,
            Some(t.on_thickness_popup.clone()),
        ));
        if t.thickness_popup_open {
            let mut list = cosmic::iced::widget::column![].spacing(2);
            for &size in &STROKE_PRESETS_PX {
                list = list.push(dropdown_row(
                    None,
                    format!("{}px", size as u32),
                    (t.thickness - size).abs() < f32::EPSILON,
                    (t.on_select_thickness)(size),
                ));
            }
            thickness = thickness
                .popup(popup_panel(list).width(Length::Fixed(110.0)))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(t.on_thickness_popup.clone());
        }

        row![
            Element::from(thickness),
            Element::from(fade),
            group_divider(),
            preset_swatches(t.color, &*t.on_select_color),
        ]
        .spacing(BTN_GAP)
        .align_y(cosmic::iced::core::Alignment::Center)
        .into()
    });

    Sections {
        on_drag_start: t.on_drag_start,
        on_drag_end: t.on_drag_end,
        capture_mode: Some((
            mode_buttons(true, noop.clone(), noop.clone()),
            h.departing(h.outgoing.capture_mode),
        )),
        target: Some((
            target_buttons(&h.outgoing_choice, noop.clone(), noop.clone(), None),
            h.departing(h.outgoing.target),
        )),
        // Video mode has no annotate button or OCR.
        tools: None,
        annotate_row: None,
        pencil: Some((
            tool_button(
                toolbar_icon(ShapeTool::Pen.icon_name(), ICON_PX),
                t.pencil_active,
                Some(t.on_pencil_toggle),
                fl!("pen"),
            ),
            transition,
        )),
        pen_settings: pen_settings.map(|settings| (settings, 1.0)),
        elapsed: Some((
            match t.audio_muted {
                Some(muted) => row![
                    elapsed_face(t.elapsed),
                    audio_pill(t.audio_levels, muted, t.on_mute_toggle),
                ]
                .spacing(BTN_GAP)
                .align_y(cosmic::iced::core::Alignment::Center)
                .into(),
                None => elapsed_face(t.elapsed),
            },
            transition,
        )),
        // Video mode has no delay either.
        delay: (delay_label(0), h.departing(h.outgoing.delay)),
        capture: btn_stop,
        settings: (settings_button(false, noop.clone()), 1.0),
        close: close_button(noop, fl!("cancel-escape")),
        end: 1.0 - transition,
        space_s: spacing.space_s,
        space_xxs: spacing.space_xxs,
    }
}
