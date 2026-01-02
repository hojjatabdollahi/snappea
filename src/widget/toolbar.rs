//! Toolbar widget for screenshot actions

use std::rc::Rc;

use cosmic::iced::Length;
use cosmic::iced_core::{Background, Border};
use cosmic::iced_widget::{column, row};
use cosmic::widget::{button, icon};
use cosmic::Element;

use crate::screenshot::{Choice, DetectedQrCode, Rect, ToolbarPosition};
use super::rectangle_selection::DragState;
use super::toolbar_position_selector::ToolbarPositionSelector;

/// Build the screenshot toolbar element
#[allow(clippy::too_many_arguments)]
pub fn build_toolbar<'a, Msg: Clone + 'static>(
    choice: Choice,
    output_name: String,
    toolbar_position: ToolbarPosition,
    has_selection: bool,
    has_ocr_text: bool,
    qr_codes: &[DetectedQrCode],
    arrow_mode: bool,
    space_s: u16,
    space_xs: u16,
    space_xxs: u16,
    on_choice_change: impl Fn(Choice) -> Msg + 'static + Clone,
    on_copy_to_clipboard: Msg,
    on_save_to_pictures: Msg,
    on_arrow_toggle: Msg,
    on_ocr: Msg,
    on_ocr_copy: Msg,
    on_qr: Msg,
    on_qr_copy: Msg,
    on_cancel: Msg,
    on_toolbar_position: &(impl Fn(ToolbarPosition) -> Msg + 'a),
) -> Element<'a, Msg> {
    use cosmic::widget::divider::vertical;
    
    let active_icon = cosmic::theme::Svg::Custom(Rc::new(|theme| cosmic::iced_widget::svg::Style {
        color: Some(theme.cosmic().accent_color().into()),
    }));
    
    // Position selector - custom widget with triangular hit regions
    let position_selector: Element<'_, Msg> = ToolbarPositionSelector::new(
        40.0, // size of the selector widget
        toolbar_position,
        on_toolbar_position(ToolbarPosition::Top),
        on_toolbar_position(ToolbarPosition::Bottom),
        on_toolbar_position(ToolbarPosition::Left),
        on_toolbar_position(ToolbarPosition::Right),
    ).into();
    
    // Common buttons
    let btn_region = button::custom(
        icon::Icon::from(icon::from_name("screenshot-selection-symbolic").size(64))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
            .class(if matches!(choice, Choice::Rectangle(..)) { active_icon.clone() } else { cosmic::theme::Svg::default() })
    )
    .selected(matches!(choice, Choice::Rectangle(..)))
    .class(cosmic::theme::Button::Icon)
    .on_press(on_choice_change(Choice::Rectangle(Rect::default(), DragState::None)))
    .padding(space_xs);
    
    let btn_window = button::custom(
        icon::Icon::from(icon::from_name("screenshot-window-symbolic").size(64))
            .class(if matches!(choice, Choice::Window(..)) { active_icon.clone() } else { cosmic::theme::Svg::default() })
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
    )
    .selected(matches!(choice, Choice::Window(..)))
    .class(cosmic::theme::Button::Icon)
    .on_press(on_choice_change(Choice::Window(output_name.clone(), None)))
    .padding(space_xs);
    
    let btn_screen = button::custom(
        icon::Icon::from(icon::from_name("screenshot-screen-symbolic").size(64))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
            .class(if matches!(choice, Choice::Output(..)) { active_icon.clone() } else { cosmic::theme::Svg::default() })
    )
    .selected(matches!(choice, Choice::Output(..)))
    .class(cosmic::theme::Button::Icon)
    .on_press(on_choice_change(Choice::Output(output_name.clone())))
    .padding(space_xs);
    
    // Copy to clipboard button
    let btn_copy = button::custom(
        icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
    )
    .class(cosmic::theme::Button::Icon)
    .on_press_maybe(has_selection.then_some(on_copy_to_clipboard))
    .padding(space_xs);
    
    // Save to pictures button
    let btn_save = button::custom(
        icon::Icon::from(icon::from_name("document-save-symbolic").size(64))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
    )
    .class(cosmic::theme::Button::Icon)
    .on_press_maybe(has_selection.then_some(on_save_to_pictures))
    .padding(space_xs);
    
    // Arrow drawing button
    let btn_arrow = button::custom(
        icon::Icon::from(icon::from_name("arrow-symbolic").size(64))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
    )
    .class(if arrow_mode { cosmic::theme::Button::Suggested } else { cosmic::theme::Button::Icon })
    .on_press_maybe(has_selection.then_some(on_arrow_toggle.clone()))
    .padding(space_xs);
    
    // OCR button
    let btn_ocr = if has_ocr_text {
        button::custom(
            icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
        )
        .class(cosmic::theme::Button::Suggested)
        .on_press_maybe(has_selection.then_some(on_ocr_copy.clone()))
        .padding(space_xs)
    } else {
        button::custom(
            icon::Icon::from(icon::from_name("ocr-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
        )
        .class(cosmic::theme::Button::Icon)
        .on_press_maybe(has_selection.then_some(on_ocr.clone()))
        .padding(space_xs)
    };
    
    // QR button
    let has_qr_codes = !qr_codes.is_empty();
    let btn_qr = if has_qr_codes {
        button::custom(
            icon::Icon::from(icon::from_name("edit-copy-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
        )
        .class(cosmic::theme::Button::Suggested)
        .on_press_maybe(has_selection.then_some(on_qr_copy.clone()))
        .padding(space_xs)
    } else {
        button::custom(
            icon::Icon::from(icon::from_name("qr-symbolic").size(64))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
        )
        .class(cosmic::theme::Button::Icon)
        .on_press_maybe(has_selection.then_some(on_qr.clone()))
        .padding(space_xs)
    };
    
    let btn_close = button::custom(
        icon::Icon::from(icon::from_name("window-close-symbolic").size(63))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
    )
    .class(cosmic::theme::Button::Icon)
    .on_press(on_cancel);
    
    let is_vertical = matches!(toolbar_position, ToolbarPosition::Left | ToolbarPosition::Right);
    
    let toolbar_content: Element<'_, Msg> = if is_vertical {
        // Vertical layout for left/right positions
        use cosmic::widget::divider::horizontal;
        if has_selection {
            column![
                position_selector,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_region, btn_window, btn_screen].spacing(space_s).align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_arrow, btn_ocr, btn_qr].spacing(space_s).align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_copy, btn_save].spacing(space_s).align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                btn_close,
            ]
            .align_x(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_s, space_xxs, space_s, space_xxs])
            .into()
        } else {
            column![
                position_selector,
                horizontal::light().width(Length::Fixed(64.0)),
                column![btn_region, btn_window, btn_screen].spacing(space_s).align_x(cosmic::iced_core::Alignment::Center),
                horizontal::light().width(Length::Fixed(64.0)),
                btn_close,
            ]
            .align_x(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_s, space_xxs, space_s, space_xxs])
            .into()
        }
    } else {
        // Horizontal layout for top/bottom positions
        if has_selection {
            row![
                position_selector,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_region, btn_window, btn_screen].spacing(space_s).align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_arrow, btn_ocr, btn_qr].spacing(space_s).align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_copy, btn_save].spacing(space_s).align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                btn_close,
            ]
            .align_y(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_xxs, space_s, space_xxs, space_s])
            .into()
        } else {
            row![
                position_selector,
                vertical::light().height(Length::Fixed(64.0)),
                row![btn_region, btn_window, btn_screen].spacing(space_s).align_y(cosmic::iced_core::Alignment::Center),
                vertical::light().height(Length::Fixed(64.0)),
                btn_close,
            ]
            .align_y(cosmic::iced_core::Alignment::Center)
            .spacing(space_s)
            .padding([space_xxs, space_s, space_xxs, space_s])
            .into()
        }
    };
    
    cosmic::widget::container(toolbar_content)
        .class(cosmic::theme::Container::Custom(Box::new(|theme| {
            let theme = theme.cosmic();
            cosmic::iced::widget::container::Style {
                background: Some(Background::Color(theme.background.component.base.into())),
                text_color: Some(theme.background.component.on.into()),
                border: Border {
                    radius: theme.corner_radii.radius_s.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })))
        .into()
}
