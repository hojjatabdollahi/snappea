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

/// Width of a dropdown trigger. The recording toolbar sizes its input zone from this.
const fn dropdown_width() -> f32 {
    DROPDOWN_PADDING as f32 * 2.0 + BTN_SIZE + BTN_GAP as f32 + CHEVRON
}

/// Width of the elapsed readout. Fits `00:00` through `999:59`.
const ELAPSED_FACE_W: f32 = 40.0;

/// Width of a value face, fixed so the toolbar does not shift as a number changes.
const VALUE_FACE_W: f32 = 34.0;

/// Width of a value dropdown. See [`dropdown_width`].
const fn dropdown_value_width() -> f32 {
    DROPDOWN_PADDING as f32 * 2.0 + VALUE_FACE_W + BTN_GAP as f32 + CHEVRON
}

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

use crate::config::{
    Container, RedactTool, SaveLocationChoice, ShapeTool, VideoSaveLocationChoice,
};
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

    // Color swatches, always visible. The chosen one gets a ring around the
    // swatch with a translucent gap between them.
    let accent: Color = cosmic::theme::active().cosmic().accent_color().into();
    let mut swatches = row![]
        .spacing(0)
        .align_y(cosmic::iced::core::Alignment::Center);
    for preset in &AnnotateColor::PRESETS {
        let preset = *preset;
        let is_selected = preset == annotate.color;
        swatches = swatches.push(
            button::custom(swatch_slot(
                swatch_disc(preset.into()),
                is_selected.then_some(accent),
            ))
            .padding(0)
            .class(cosmic::theme::Button::Icon)
            .on_press((annotate.on_select_color)(preset)),
        );
    }

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
            None,
            on_select,
        ));
    }

    rows.push(menu_row(
        fl!("save-to-custom-folder"),
        false,
        None,
        on_browse,
    ));
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

/// A menu row: a check column, a label, and an optional trailing value.
fn menu_row<'a, Msg: Clone + 'static>(
    label: String,
    checked: bool,
    trailing: Option<String>,
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

    let mut item = row![check, text::body(label).width(Length::Fill)]
        .spacing(8)
        .align_y(cosmic::iced::core::Alignment::Center);

    if let Some(value) = trailing {
        item = item.push(
            text::body(value)
                .width(Length::Fixed(MENU_VALUE_WIDTH))
                .align_x(cosmic::iced::core::alignment::Horizontal::Right)
                .wrapping(cosmic::iced::core::text::Wrapping::Word)
                .ellipsize(cosmic::iced::core::text::Ellipsize::End(
                    cosmic::iced::core::text::EllipsizeHeightLimit::Lines(2),
                )),
        );
        item = item.push(
            toolbar_icon("pan-down-symbolic", 24)
                .width(Length::Fixed(12.0))
                .height(Length::Fixed(12.0)),
        );
    }

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

/// Maximum width of a row's trailing value. Longer encoder names wrap and ellipsize.
const MENU_VALUE_WIDTH: f32 = 132.0;

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
    pub on_screenshot_cursor: Msg,
    pub on_default_portal: Msg,
    pub on_recognize_qr_codes: Msg,
    pub on_save_pictures: Msg,
    pub on_save_documents: Msg,
    pub on_save_custom: Msg,
    pub on_save_videos: Msg,
    pub on_save_video_custom: Msg,
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
            None,
            settings.on_show_cursor.clone(),
        ));
        list = list.push(menu_divider());
        list = list.push(menu_row(
            fl!("save-to-videos"),
            settings.video_save_location == VideoSaveLocationChoice::Videos,
            None,
            settings.on_save_videos.clone(),
        ));
        for row in custom_folder_rows(
            settings.video_custom_save_path,
            settings.video_save_location == VideoSaveLocationChoice::Custom,
            settings.on_save_video_custom.clone(),
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
            None,
            settings.on_magnifier.clone(),
        ));
        // Asked separately from the recording toolbar's cursor setting.
        list = list.push(menu_row(
            fl!("show-cursor"),
            settings.show_cursor,
            None,
            settings.on_screenshot_cursor.clone(),
        ));
        list = list.push(menu_divider());
        // One exclusive choice: clipboard only, or a folder (which also copies).
        list = list.push(menu_row(
            fl!("save-to-clipboard"),
            settings.save_location == SaveLocationChoice::Clipboard,
            None,
            settings.on_save_clipboard.clone(),
        ));
        list = list.push(menu_row(
            fl!("save-to-pictures"),
            settings.save_location == SaveLocationChoice::Pictures,
            None,
            settings.on_save_pictures.clone(),
        ));
        list = list.push(menu_row(
            fl!("save-to-documents"),
            settings.save_location == SaveLocationChoice::Documents,
            None,
            settings.on_save_documents.clone(),
        ));
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
            None,
            settings.on_recognize_qr_codes.clone(),
        ));
    }

    list = list.push(menu_divider());
    // Named for the state, since it is a ticked toggle like the other rows.
    list = list.push(menu_row(
        fl!("default-screenshot-app"),
        settings.is_default_portal,
        None,
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

/// The preset color swatches, ringed in the accent color when active.
fn color_swatches<'a, Msg: Clone + 'static>(
    current: AnnotateColor,
    on_select: &dyn Fn(AnnotateColor) -> Msg,
    spacing: u16,
) -> cosmic::iced::widget::Row<'a, Msg, cosmic::Theme, cosmic::Renderer> {
    let accent: Color = cosmic::theme::active().cosmic().accent_color().into();
    let mut swatches = row![]
        .spacing(spacing)
        .align_y(cosmic::iced::core::Alignment::Center);

    for preset in &AnnotateColor::PRESETS {
        let preset = *preset;
        let is_selected = preset == current;
        let fill: Color = preset.into();
        swatches = swatches.push(
            button::custom(
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
                            width: if is_selected { 2.0 } else { 0.0 },
                            color: accent,
                        },
                        ..Default::default()
                    }
                }))),
            )
            .padding(2)
            .class(cosmic::theme::Button::Icon)
            .on_press(on_select(preset)),
        );
    }
    swatches
}

/// How long a freehand stroke lingers before fading, in seconds.
const FADE_PRESETS: &[f32] = &[1.0, 2.0, 3.0, 5.0, 10.0];

/// State and callbacks for the toolbar shown while recording.
pub struct RecordingToolbar<'a, Msg> {
    /// The video toolbar is still visible at 0 and has fully collapsed at 1.
    pub transition: f32,
    /// Selection shown by the departing target controls.
    pub outgoing_choice: Choice,
    /// How long the recording has been running.
    pub elapsed: Duration,
    pub pencil_active: bool,
    pub color: AnnotateColor,
    pub fade_duration: f32,
    pub fade_popup_open: bool,
    pub thickness: f32,
    pub thickness_popup_open: bool,
    pub on_drag_start: Msg,
    pub on_drag_end: Msg,
    /// Harmless action used by departing controls during their brief exit.
    pub on_transition_noop: Msg,
    pub on_pencil_toggle: Msg,
    pub on_stop: Msg,
    pub on_fade_popup: Msg,
    pub on_select_color: Box<dyn Fn(AnnotateColor) -> Msg + 'a>,
    pub on_select_fade: Box<dyn Fn(f32) -> Msg + 'a>,
    pub on_thickness_popup: Msg,
    pub on_select_thickness: Box<dyn Fn(f32) -> Msg + 'a>,
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

/// Size of the recording toolbar, for the surface's input zone, which is fixed at
/// creation. Derived from the constants `build_recording_toolbar` uses.
#[must_use]
pub fn recording_toolbar_size(pencil_active: bool) -> (f32, f32) {
    const DIVIDER_W: f32 = 1.0;
    let outer_pad = f32::from(SECTION_GAP);
    let button = f32::from(BTN_PADDING).mul_add(2.0, BTN_SIZE);
    let dropdown = dropdown_width();
    let stop = f32::from(CAPTURE_PADDING).mul_add(2.0, CAPTURE_BTN_SIZE);

    // Outer padding, grip, pen section, elapsed readout and stop section. Each
    // section owns a leading gap, divider and trailing gap.
    let section_chrome = outer_pad + DIVIDER_W + outer_pad;
    let mut width = outer_pad.mul_add(2.0, BTN_SIZE)
        + section_chrome
        + button
        + section_chrome
        + ELAPSED_FACE_W
        + section_chrome
        + stop;

    if pencil_active {
        let count = AnnotateColor::PRESETS.len() as f32;
        let swatches = (count - 1.0).mul_add(f32::from(BTN_GAP), count * (SWATCH + 4.0));
        // thickness (a value), fade (an icon), an internal divider, swatches
        let settings = dropdown
            + dropdown_value_width()
            + (DIVIDER_W + f32::from((SECTION_GAP - BTN_GAP) * 2))
            + swatches
            + f32::from(BTN_GAP * 3);
        width += section_chrome + settings;
    }

    // The panel's own edge, on both sides. A zone short of the toolbar leaves
    // the buttons at its edge unclickable.
    width = TOOLBAR_BORDER.mul_add(2.0, width);
    let vertical_pad = f32::from(cosmic::theme::active().cosmic().spacing.space_xxs);
    (
        width,
        TOOLBAR_BORDER.mul_add(2.0, vertical_pad.mul_add(2.0, TOOLBAR_H)),
    )
}

/// The toolbar shown while recording: grip, drawing tools, stop.
pub fn build_recording_toolbar<Msg: Clone + 'static>(
    t: RecordingToolbar<'_, Msg>,
) -> Element<'_, Msg> {
    let transition = t.transition.clamp(0.0, 1.0);
    let departing = 1.0 - transition;

    // The departing capture controls, inert, for the handoff animation.
    let btn_photo_mode = tooltip(
        button::custom(screen_capture_icon())
            .class(cosmic::theme::Button::Icon)
            .padding(BTN_PADDING)
            .on_press(t.on_transition_noop.clone()),
        text::body(fl!("screenshot-mode")),
        tooltip::Position::Bottom,
    );
    let btn_video_mode = tooltip(
        button::custom(
            toolbar_icon("camera-video-symbolic", ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .selected(true)
        .class(cosmic::theme::Button::Suggested)
        .padding(BTN_PADDING)
        .on_press(t.on_transition_noop.clone()),
        text::body(fl!("video-mode")),
        tooltip::Position::Bottom,
    );

    let region_selected = matches!(t.outgoing_choice, Choice::Rectangle(..));
    let btn_region = button::custom(
        toolbar_icon("screenshot-selection-symbolic", ICON_PX)
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE)),
    )
    .selected(region_selected)
    .class(if region_selected {
        cosmic::theme::Button::Suggested
    } else {
        cosmic::theme::Button::Icon
    })
    .padding(BTN_PADDING)
    .on_press(t.on_transition_noop.clone());
    let screen_selected = matches!(t.outgoing_choice, Choice::Output(..));
    let btn_screen = button::custom(
        toolbar_icon("screenshot-screen-symbolic", ICON_PX)
            .width(Length::Fixed(BTN_SIZE))
            .height(Length::Fixed(BTN_SIZE)),
    )
    .selected(screen_selected)
    .class(if screen_selected {
        cosmic::theme::Button::Suggested
    } else {
        cosmic::theme::Button::Icon
    })
    .padding(BTN_PADDING)
    .on_press(t.on_transition_noop.clone());

    let btn_settings = button::custom(
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
    .class(cosmic::theme::Button::Icon)
    .padding(BTN_PADDING)
    .on_press(t.on_transition_noop.clone());
    let btn_close = close_button(t.on_transition_noop.clone(), fl!("cancel-escape"));

    let btn_pencil = tooltip(
        button::custom(
            toolbar_icon(ShapeTool::Pen.icon_name(), ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .selected(t.pencil_active)
        .class(if t.pencil_active {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .padding(BTN_PADDING)
        .on_press(t.on_pencil_toggle),
        text::body(fl!("pen")),
        tooltip::Position::Bottom,
    );

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
    );

    // Pen settings appear with the pen, as they do in the annotation section.
    let recording_settings: Option<Element<'_, Msg>> = if t.pencil_active {
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

        Some(
            row![
                Element::from(thickness),
                Element::from(fade),
                group_divider(),
                color_swatches(t.color, &*t.on_select_color, BTN_GAP),
            ]
            .spacing(BTN_GAP)
            .align_y(cosmic::iced::core::Alignment::Center)
            .into(),
        )
    } else {
        None
    };

    let spacing = cosmic::theme::active().cosmic().spacing;
    let body: Element<'_, Msg> = row![
        toolbar_strut(),
        grip_button(t.on_drag_start, t.on_drag_end),
        section(
            row![btn_photo_mode, btn_video_mode]
                .spacing(BTN_GAP)
                .align_y(cosmic::iced::core::Alignment::Center),
            departing,
        ),
        section(
            row![btn_region, btn_screen]
                .spacing(BTN_GAP)
                .align_y(cosmic::iced::core::Alignment::Center),
            departing,
        ),
        section(btn_pencil, transition),
    ]
    .push_maybe(recording_settings.map(|settings| section(settings, 1.0)))
    // Beside the stop button, because the two answer the same question: how
    // long has this been going, and how do I end it.
    .push(section(
        row![elapsed_face(t.elapsed)].align_y(cosmic::iced::core::Alignment::Center),
        transition,
    ))
    .push(section(btn_stop, 1.0))
    .push(joined_section(
        row![btn_settings, btn_close]
            .spacing(BTN_GAP)
            .align_y(cosmic::iced::core::Alignment::Center),
        departing,
    ))
    .align_y(cosmic::iced::core::Alignment::Center)
    .spacing(0)
    .padding([spacing.space_xxs, SECTION_GAP, spacing.space_xxs, 0])
    .into();

    toolbar_surface(body)
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

/// The X at the end of the toolbar. The tooltip says what it cancels.
fn close_button<'a, Msg: Clone + 'static>(on_press: Msg, tip: String) -> Element<'a, Msg> {
    tooltip(
        button::custom(
            toolbar_icon("window-close-symbolic", ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .class(cosmic::theme::Button::Icon)
        .on_press(on_press)
        .padding(BTN_PADDING),
        text::body(tip),
        tooltip::Position::Bottom,
    )
    .into()
}

/// Everything the countdown pill needs.
pub struct CountdownToolbar<Msg> {
    /// The armed-delay toolbar is still visible at 0 and has pared itself down
    /// to the countdown toolbar at 1.
    pub transition: f32,
    /// How much of the annotation entry point was visible when capture began.
    pub outgoing_tools: f32,
    /// When the wait began and how long it runs. Handed to the shutter, which
    /// closes its ring against the same clock the capture fires on.
    pub started: Instant,
    pub duration: Duration,
    /// Whole seconds left, for the delay button.
    pub remaining: u32,
    pub on_drag_start: Msg,
    pub on_drag_end: Msg,
    /// Harmless action used by controls during their brief exit.
    pub on_transition_noop: Msg,
    pub on_cancel: Msg,
}

/// Size of the countdown pill, for the surface's input zone.
#[must_use]
pub fn countdown_toolbar_size() -> (f32, f32) {
    const DIVIDER_W: f32 = 1.0;
    let spacing = cosmic::theme::active().cosmic().spacing;
    let space_s = f32::from(spacing.space_s);
    let space_xxs = f32::from(spacing.space_xxs);
    let gap = f32::from(SECTION_GAP);
    let pad = f32::from(BTN_PADDING);

    let button = pad.mul_add(2.0, BTN_SIZE);
    let delay = pad.mul_add(2.0, DELAY_FACE_W);
    let shutter = f32::from(CAPTURE_PADDING).mul_add(2.0, CAPTURE_BTN_SIZE);

    let width = TOOLBAR_BORDER.mul_add(
        2.0,
        gap                       // the row's own padding, left
        + BTN_SIZE                        // grip
        + gap + DIVIDER_W + gap           // the capture section's rule
        + delay + space_s + shutter       // delay, then the shutter it belongs to
        + gap + button + gap,
    );

    (
        width,
        TOOLBAR_BORDER.mul_add(2.0, space_xxs.mul_add(2.0, TOOLBAR_H)),
    )
}

/// Size of the armed toolbar on the first frame of the handoff, so the input zone
/// covers the whole animation.
#[must_use]
pub fn countdown_toolbar_transition_size(outgoing_tools: f32) -> (f32, f32) {
    const DIVIDER_W: f32 = 1.0;
    let (final_width, height) = countdown_toolbar_size();
    let button = f32::from(BTN_PADDING).mul_add(2.0, BTN_SIZE);
    let tool_section = f32::from(SECTION_GAP).mul_add(2.0, DIVIDER_W) + button;
    let settings = button + f32::from(BTN_GAP);

    (
        final_width + tool_section * outgoing_tools.clamp(0.0, 1.0) + settings,
        height,
    )
}

/// The toolbar shown while a delayed capture waits: grip, delay, shutter and X.
pub fn build_countdown_toolbar<'a, Msg: Clone + 'static>(
    t: CountdownToolbar<Msg>,
) -> Element<'a, Msg> {
    let spacing = cosmic::theme::active().cosmic().spacing;
    let transition = t.transition.clamp(0.0, 1.0);
    let departing = 1.0 - transition;

    // The shutter button, counting down. It is inert here. The X button cancels
    // the countdown.
    let shutter = cosmic::widget::container(
        capture_button(CaptureMode::Counting {
            started: t.started,
            duration: t.duration,
        })
        .transition_from(CaptureMode::Photo, transition)
        // Keep the outgoing shutter at full strength on the first frame. The
        // message is deliberately inert, and disappears once the handoff ends.
        .on_press_maybe((transition < 1.0).then_some(t.on_transition_noop.clone())),
    )
    .padding(CAPTURE_PADDING);

    let annotate = tooltip(
        button::custom(
            toolbar_icon("markup-symbolic", ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .class(cosmic::theme::Button::Icon)
        .padding(BTN_PADDING)
        .on_press(t.on_transition_noop.clone()),
        text::body(fl!("annotate")),
        tooltip::Position::Bottom,
    );

    let settings = button::custom(
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
    .class(cosmic::theme::Button::Icon)
    .padding(BTN_PADDING)
    .on_press(t.on_transition_noop);

    // The delay value, shown as a label while counting down, since there is nothing to press.
    let delay = cosmic::widget::container(delay_face(t.remaining)).padding(BTN_PADDING);

    let body: Element<'_, Msg> = row![
        toolbar_strut(),
        grip_button(t.on_drag_start, t.on_drag_end),
        section(annotate, departing * t.outgoing_tools.clamp(0.0, 1.0)),
        section(
            row![
                gap_after(Element::from(delay), spacing.space_s),
                Element::from(shutter),
            ]
            .align_y(cosmic::iced::core::Alignment::Center),
            1.0,
        ),
        joined_section(
            row![
                Element::from(Collapse::new(gap_after(settings, BTN_GAP), departing,)),
                close_button(t.on_cancel, fl!("countdown-cancel")),
            ]
            .align_y(cosmic::iced::core::Alignment::Center),
            1.0,
        ),
    ]
    .align_y(cosmic::iced::core::Alignment::Center)
    .spacing(0)
    .padding([spacing.space_xxs, SECTION_GAP, spacing.space_xxs, 0])
    .into();

    toolbar_surface(body)
}

/// Build the screenshot toolbar element
#[allow(clippy::too_many_arguments)]
pub fn build_toolbar<'a, Msg: Clone + 'static>(
    choice: Choice,
    _output_name: String,
    has_selection: bool,
    _primary_redact_tool: RedactTool,
    _redact_mode_active: bool,
    _redact_popup_open: bool,
    space_s: u16,
    _space_xs: u16,
    space_xxs: u16,
    on_choice_change: impl Fn(Choice) -> Msg + 'static + Clone,
    on_screen_mode: Msg,
    on_save_to_pictures: Msg,
    on_delayed_capture: Msg,
    on_delay_popup: Msg,
    on_select_delay: Box<dyn Fn(u32) -> Msg + 'a>,
    delay_popup_open: bool,
    capture_delay_secs: u32,
    // Decided by the session so the toolbar and the button agree.
    delayed: bool,
    on_record_region: Msg,
    _on_stop_recording: Msg,
    _on_toggle_recording_annotation: Msg,
    _on_pencil_right_click: Msg,
    _on_redact_press: Msg,
    _on_redact_right_click: Msg,
    on_cancel: Msg,
    on_toolbar_drag_start: Msg,
    on_toolbar_drag_end: Msg,
    output_count: usize,
    // Whether OCR has produced text that is waiting to be copied.
    has_ocr_text: bool,
    on_ocr: Msg,
    on_ocr_copy: Msg,
    is_video_mode: bool,
    is_recording: bool,
    // How far each varying section is open.
    progress: crate::capture::ToolbarProgress,
    _recording_annotation_mode: bool,
    _pencil_popup_open: bool,
    on_capture_mode_photo: Msg,
    on_capture_mode_video: Msg,
    annotate: AnnotateSection<'a, Msg>,
    settings: SettingsMenu<'a, Msg>,
) -> Element<'a, Msg> {
    // Grip: press and hold to drag. The parent widget tracks the motion.
    let grip = grip_button(on_toolbar_drag_start, on_toolbar_drag_end);

    // Capture mode: screenshot or video, as two ordinary toolbar buttons
    let btn_photo_mode = tooltip(
        button::custom(screen_capture_icon())
            .selected(!is_video_mode)
            .class(if is_video_mode {
                cosmic::theme::Button::Icon
            } else {
                cosmic::theme::Button::Suggested
            })
            .on_press(on_capture_mode_photo)
            .padding(BTN_PADDING),
        text::body(fl!("screenshot-mode")),
        tooltip::Position::Bottom,
    );

    let btn_video_mode = tooltip(
        button::custom(
            toolbar_icon("camera-video-symbolic", ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .selected(is_video_mode)
        .class(if is_video_mode {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_capture_mode_video)
        .padding(BTN_PADDING),
        text::body(fl!("video-mode")),
        tooltip::Position::Bottom,
    );

    // Common buttons with tooltips
    let is_region_selected = matches!(choice, Choice::Rectangle(..));
    let btn_region = tooltip(
        button::custom(
            toolbar_icon("screenshot-selection-symbolic", ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .selected(is_region_selected)
        .class(if is_region_selected {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_choice_change(Choice::Rectangle(
            Rect::default(),
            DragState::None,
        )))
        .padding(BTN_PADDING),
        text::body(fl!("select-region")),
        tooltip::Position::Bottom,
    );

    let is_screen_selected = matches!(choice, Choice::Output(..));
    let btn_screen = tooltip(
        button::custom(
            toolbar_icon("screenshot-screen-symbolic", ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .selected(is_screen_selected)
        .class(if is_screen_selected {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .on_press(on_screen_mode) // Uses proper screen mode handler with output index
        .padding(BTN_PADDING),
        text::body(fl!("select-screen")),
        tooltip::Position::Bottom,
    );

    // All-screens is its own button. No choice no longer means every screen.
    let save_tooltip = match &choice {
        Choice::Rectangle(r, _) if r.dimensions().is_some() => fl!("save-selected-region"),
        Choice::Output(Some(_)) => fl!("save-selected-screen"),
        Choice::AllScreens => fl!("save-all-screens"),
        _ => fl!("select-something-first"),
    };

    // One button captures to the configured folder and the clipboard. Only the
    // clipboard row skips the file.
    //
    // The same button in both modes, at the same size.
    let (capture_action, capture_tooltip) = if is_video_mode {
        (
            has_selection.then_some(on_record_region),
            if has_selection {
                fl!("record-selection")
            } else {
                fl!("record-disabled")
            },
        )
    } else {
        (
            // The delay dropdown sets the delay and the capture button applies it.
            has_selection.then(|| {
                // A delayed capture re-photographs the screen with the same selection.
                if delayed {
                    on_delayed_capture.clone()
                } else {
                    on_save_to_pictures.clone()
                }
            }),
            if delayed && has_selection {
                fl!("capture-after-delay", secs = capture_delay_secs)
            } else {
                save_tooltip
            },
        )
    };

    let btn_capture = tooltip(
        cosmic::widget::container(
            capture_button(CaptureMode::of(is_video_mode, is_recording))
                .on_press_maybe(capture_action),
        )
        .padding(CAPTURE_PADDING),
        text::body(capture_tooltip),
        tooltip::Position::Bottom,
    );

    // All-screens button, only meaningful with more than one output
    let is_all_screens = matches!(choice, Choice::AllScreens);
    // Hidden while recording, since the recorder takes one region on one output.
    let btn_all_screens: Option<Element<'_, Msg>> =
        (output_count > 1 && !is_video_mode).then(|| {
            tooltip(
                button::custom(
                    toolbar_icon("screenshot-all-screens-symbolic", ICON_PX)
                        .width(Length::Fixed(BTN_SIZE))
                        .height(Length::Fixed(BTN_SIZE)),
                )
                .selected(is_all_screens)
                .class(if is_all_screens {
                    cosmic::theme::Button::Suggested
                } else {
                    cosmic::theme::Button::Icon
                })
                .on_press(on_choice_change(Choice::AllScreens))
                .padding(BTN_PADDING),
                text::body(fl!("select-all-screens")),
                tooltip::Position::Bottom,
            )
            .into()
        });

    // Delayed screenshot button: left-click captures after a delay, right-click
    // cycles the delay. Shown in screenshot mode regardless of selection.
    let btn_delay: Element<'_, Msg> = {
        // An armed delay shows its count on the button.
        let delay_set = capture_delay_secs > 0;

        // Not highlighted. An armed delay is a setting and does not need a lit button.
        let trigger = tooltip(
            button::custom(delay_face(capture_delay_secs))
                .class(if delay_popup_open {
                    cosmic::theme::Button::Suggested
                } else {
                    cosmic::theme::Button::Icon
                })
                .on_press(on_delay_popup.clone())
                .padding(BTN_PADDING),
            // A delay of zero means no delay. The button opens the menu to set one.
            text::body(if delay_set {
                fl!("delayed-screenshot", secs = capture_delay_secs)
            } else {
                fl!("delay-menu")
            }),
            tooltip::Position::Bottom,
        );

        let mut pop = cosmic::widget::popover(trigger);
        if delay_popup_open {
            let mut list = cosmic::iced::widget::column![].spacing(2);
            for secs in DELAY_PRESETS.iter().copied() {
                let label = if secs == 0 {
                    fl!("no-delay")
                } else {
                    fl!("delay-seconds", secs = secs)
                };
                list = list.push(menu_row(
                    label,
                    secs == capture_delay_secs,
                    None,
                    on_select_delay(secs),
                ));
            }
            pop = pop
                .popup(popup_panel(list).width(Length::Fixed(150.0)))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(on_delay_popup);
        }
        pop.into()
    };

    // OCR: one button with two faces, read then copy.
    //
    // Shown only with a region selected and tesseract installed.
    let btn_ocr: Element<'_, Msg> = if has_ocr_text {
        tooltip(
            button::custom(
                toolbar_icon("edit-copy-symbolic", ICON_PX)
                    .width(Length::Fixed(BTN_SIZE))
                    .height(Length::Fixed(BTN_SIZE)),
            )
            .class(cosmic::theme::Button::Suggested)
            .padding(BTN_PADDING)
            .on_press(on_ocr_copy),
            text::body(fl!("copy-ocr-text")),
            tooltip::Position::Bottom,
        )
        .into()
    } else {
        tooltip(
            button::custom(
                toolbar_icon("recognize-text-symbolic", ICON_PX)
                    .width(Length::Fixed(BTN_SIZE))
                    .height(Length::Fixed(BTN_SIZE)),
            )
            .class(cosmic::theme::Button::Icon)
            .padding(BTN_PADDING)
            .on_press(on_ocr),
            text::body(fl!("recognize-text")),
            tooltip::Position::Bottom,
        )
        .into()
    };

    // Annotation entry point. Pressing it swaps the middle of the toolbar for
    // the drawing tools. It stays visible and selected while they are up.
    let btn_annotate: Element<'_, Msg> = tooltip(
        button::custom(
            toolbar_icon("markup-symbolic", ICON_PX)
                .width(Length::Fixed(BTN_SIZE))
                .height(Length::Fixed(BTN_SIZE)),
        )
        .selected(annotate.active)
        .class(if annotate.active {
            cosmic::theme::Button::Suggested
        } else {
            cosmic::theme::Button::Icon
        })
        .padding(BTN_PADDING)
        .on_press_maybe(has_selection.then(|| annotate.on_toggle.clone())),
        text::body(fl!("annotate")),
        tooltip::Position::Bottom,
    )
    .into();

    // Settings menu: three dots opening a popover, matching the tool dropdowns
    let btn_settings: Element<'_, Msg> = {
        let trigger = tooltip(
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
            .class(if settings.open {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Icon
            })
            .on_press(settings.on_toggle.clone())
            .padding(BTN_PADDING),
            text::body(fl!("settings")),
            tooltip::Position::Bottom,
        );

        let mut pop = cosmic::widget::popover(trigger);
        if settings.open {
            // The menu owns the three pickers, so it takes the whole struct.
            let on_close = settings.on_toggle.clone();
            pop = pop
                .popup(build_settings_menu(settings, is_video_mode))
                .position(cosmic::widget::popover::Position::Top)
                .on_close(on_close);
        }
        pop.into()
    };

    let btn_close = close_button(on_cancel, fl!("cancel-escape"));

    // One row for every mode. Section widths are eased 0..1 by the session
    // state, so a mode change slides buttons instead of rebuilding the row.
    //
    // No spacing on the outer row: each section carries its own leading gap and
    // divider, so a collapsed one leaves no stray gap.
    let toolbar_body_content: Element<'_, Msg> = row![
        toolbar_strut(),
        grip,
        section(
            row![btn_photo_mode, btn_video_mode]
                .spacing(BTN_GAP)
                .align_y(cosmic::iced::core::Alignment::Center),
            progress.capture_mode,
        ),
        section(
            // Region, window, screen, all screens: narrowest target to widest.
            row![btn_region]
                .push(btn_screen)
                .push_maybe(btn_all_screens)
                .spacing(BTN_GAP)
                .align_y(cosmic::iced::core::Alignment::Center),
            progress.target,
        ),
        // Toggles the annotation tools. Stays selected while they show.
        // OCR shares the section, since both act on a selection.
        section(
            row![
                btn_annotate,
                Element::from(Collapse::new(gap_before(btn_ocr, BTN_GAP), progress.ocr)),
            ]
            .align_y(cosmic::iced::core::Alignment::Center),
            progress.tools,
        ),
        section(
            build_annotate_row(annotate, space_xxs),
            progress.annotate_row,
        ),
        section(
            row![
                Element::from(Collapse::new(gap_after(btn_delay, space_s), progress.delay,)),
                btn_capture,
            ]
            .align_y(cosmic::iced::core::Alignment::Center),
            1.0,
        ),
        joined_section(
            row![btn_settings, btn_close]
                .spacing(BTN_GAP)
                .align_y(cosmic::iced::core::Alignment::Center),
            1.0,
        ),
    ]
    .align_y(cosmic::iced::core::Alignment::Center)
    .spacing(0)
    .padding([space_xxs, SECTION_GAP, space_xxs, 0])
    .into();

    toolbar_surface(toolbar_body_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(rows: &[Element<'_, ()>]) -> usize {
        rows.len()
    }

    #[test]
    fn custom_folder_row_when_chosen() {
        // A chosen folder gets a row naming it. Otherwise only the picker row is shown.
        let picked: Vec<Element<'_, ()>> = custom_folder_rows("/home/u/Photos 2026", true, (), ());
        assert_eq!(labels(&picked), 2, "the folder, and the picker");

        let not_picked: Vec<Element<'_, ()>> =
            custom_folder_rows("/home/u/Photos 2026", false, (), ());
        assert_eq!(labels(&not_picked), 1, "just the picker");
    }

    #[test]
    fn empty_folder_has_no_row() {
        // A path that ends in nothing nameable (the root, or an empty setting)
        // has no folder to put in a label.
        let rows: Vec<Element<'_, ()>> = custom_folder_rows("", true, (), ());
        assert_eq!(rows.len(), 1);
        let root: Vec<Element<'_, ()>> = custom_folder_rows("/", true, (), ());
        assert_eq!(root.len(), 1);
    }

    #[test]
    fn dropdown_width_adds_chevron() {
        // Icon at the plain button size. The extra width is the chevron and its gap.
        let plain = f32::from(BTN_PADDING).mul_add(2.0, BTN_SIZE);
        let extra = dropdown_width() - plain;
        assert!(
            extra > 0.0,
            "a dropdown is wider than a plain button, not narrower"
        );
        let padding_saved = 2.0 * f32::from(BTN_PADDING - DROPDOWN_PADDING);
        assert_eq!(extra, CHEVRON + f32::from(BTN_GAP) - padding_saved);
    }

    #[test]
    fn dropdown_height_matches_button() {
        // Both carry BTN_PADDING vertically around a BTN_SIZE box, so which
        // tool is armed cannot change the height of the row.
        assert_eq!(TOOLBAR_H, f32::from(BTN_PADDING).mul_add(2.0, BTN_SIZE));
    }
}
