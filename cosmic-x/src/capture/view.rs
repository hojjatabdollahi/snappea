// SPDX-License-Identifier: GPL-3.0-only

//! The overlay view for a capture in progress.

use crate::app::App;
use crate::capture::msg::Msg;
use crate::geometry::Choice;
use cosmic::iced::core::Length;
use cosmic::iced::window;

pub fn view(app: &App, id: window::Id) -> cosmic::Element<'_, Msg> {
    use crate::widget::selection::{OutputContext, ScreenshotSelectionWidget};

    let Some((i, output)) = app.outputs.iter().enumerate().find(|(_idx, o)| o.id == id) else {
        return cosmic::iced::widget::space()
            .width(Length::Fixed(1.0))
            .into();
    };
    let Some(capture) = app.capture.as_ref() else {
        return cosmic::iced::widget::space()
            .width(Length::Fixed(1.0))
            .into();
    };
    let Some(img) = capture.output_images.get(&output.name) else {
        return cosmic::iced::widget::space()
            .width(Length::Fixed(1.0))
            .into();
    };

    let theme = app.core.system_theme().cosmic();

    // Calculate derived state
    let has_any_annotations = capture.annotations.has_shapes();
    let has_any_redactions = capture.annotations.has_redactions();
    let has_ocr_text = capture.detection.ocr_text.is_some();

    let is_active_output = {
        let output_name = &output.name;
        match &capture.selection.choice {
            Choice::Rectangle(_, _) => true,
            Choice::AllScreens => true,
            Choice::Output(None) => true,
            Choice::Output(Some(selected)) => output_name == selected,
        }
    };

    let has_confirmed_selection = matches!(&capture.selection.choice, Choice::Output(Some(_)));

    let output_ctx = OutputContext {
        output_count: app.outputs.len(),
        focused_output_index: capture.selection.focused_output_index,
        current_output_index: i,
        is_active_output,
        has_confirmed_selection,
        has_mouse_entered: capture.selection.has_mouse_entered,
    };

    // Build widget with grouped state and single event handler
    ScreenshotSelectionWidget::new(
        capture.selection.choice.clone(),
        img,
        output,
        id,
        theme.spacing,
        i as u128,
        &capture.annotations,
        &capture.detection,
        &capture.ui,
        &app.color_picker,
        &app.font_families,
        &app.text_style_model,
        &app.text_align_model,
        app.text.as_ref(),
        output_ctx,
        has_any_annotations,
        has_any_redactions,
        has_ocr_text,
    )
    .into()
}
