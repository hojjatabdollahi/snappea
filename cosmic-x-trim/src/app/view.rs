// SPDX-License-Identifier: GPL-3.0-only

use super::{Editor, Message, SPEED_OPTIONS};
use crate::export::{self, Format};
use crate::fl;
use crate::widgets::track_bar::{TRACK_HEIGHT, track_bar};
use cosmic::iced::{Alignment, ContentFit, Length};
use cosmic::widget::{self, icon};
use iced_video_player::VideoPlayer;

const PLAYBACK_SPEED_ICON: &[u8] =
    include_bytes!("../../res/icons/scalable/actions/playback-speed-symbolic.svg");

pub fn view(editor: &Editor) -> cosmic::Element<'_, Message> {
    let preview = widget::container(
        VideoPlayer::new(&editor.video)
            .on_new_frame(Message::NewFrame)
            .on_end_of_stream(Message::EndOfStream)
            .on_duration_changed(Message::DurationChanged)
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Contain),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center);

    widget::column::with_children(vec![preview.into(), controls(editor), actions(editor)])
        .spacing(8)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A blocking modal
pub fn dialog(editor: &Editor) -> Option<cosmic::Element<'_, Message>> {
    let export = editor.export.as_ref()?;
    let progress = cosmic::iced::widget::progress_bar(0.0..=1.0, export.fraction)
        .girth(Length::Fixed(8.0))
        .length(Length::Fill);
    Some(
        widget::dialog()
            .title(fl!("converting"))
            .body(format!("{}%", (export.fraction * 100.0).round() as u32))
            .control(progress)
            .primary_action(widget::button::standard(fl!("cancel")).on_press(Message::CancelExport))
            .into(),
    )
}

fn controls(editor: &Editor) -> cosmic::Element<'_, Message> {
    let play = widget::button::custom(
        icon::from_name(if editor.playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        })
        .size(16)
        .icon(),
    )
    .class(cosmic::theme::Button::Icon)
    .on_press(Message::TogglePlay);

    // center the buttons against the colored track only, ignore the label heigh
    let play = widget::container(play)
        .height(Length::Fixed(TRACK_HEIGHT))
        .align_y(Alignment::Center);
    let speed = widget::container(speed_menu(editor))
        .height(Length::Fixed(TRACK_HEIGHT))
        .align_y(Alignment::Center);

    widget::row::with_children(vec![
        play.into(),
        speed.into(),
        track_bar(
            editor.duration,
            editor.trim_start,
            editor.trim_end,
            editor.position,
        )
        .minimum_span(editor.frame_duration())
        .colors(&editor.frame_colors)
        .on_seek(Message::Seek)
        .on_seek_release(Message::SeekReleased)
        .on_trim(Message::TrimChanged)
        .on_release(Message::TrimReleased)
        .into(),
    ])
    .spacing(8)
    .align_y(Alignment::Start)
    .into()
}

fn speed_menu(editor: &Editor) -> cosmic::Element<'_, Message> {
    let trigger = widget::button::custom(
        icon::from_svg_bytes(PLAYBACK_SPEED_ICON)
            .symbolic(true)
            .icon()
            .size(16),
    )
    .class(cosmic::theme::Button::Icon)
    .on_press(Message::ToggleSpeedMenu);

    let mut menu = widget::popover(trigger);
    if !editor.speed_menu_open {
        return menu.into();
    }

    let items: Vec<cosmic::Element<'_, Message>> = SPEED_OPTIONS
        .iter()
        .enumerate()
        .map(|(index, &option)| {
            let active = (editor.speed - option).abs() < 0.01;
            let mark: cosmic::Element<'_, Message> = if active {
                icon::from_name("object-select-symbolic")
                    .size(16)
                    .icon()
                    .into()
            } else {
                widget::container(cosmic::iced::widget::space().width(Length::Fixed(16.0))).into()
            };
            let label = widget::text(format!("{option}x"));
            let label = if active {
                label.font(cosmic::font::bold())
            } else {
                label
            };

            let row = widget::row::with_children(vec![mark, label.into()])
                .spacing(8)
                .align_y(Alignment::Center);
            let content: cosmic::Element<'_, Message> = if active {
                widget::container(row)
                    .class(cosmic::theme::Container::custom(|theme| {
                        let accent: cosmic::iced::Color = theme.cosmic().accent_color().into();
                        cosmic::iced::widget::container::Style {
                            icon_color: Some(accent),
                            text_color: Some(accent),
                            ..Default::default()
                        }
                    }))
                    .into()
            } else {
                row.into()
            };

            widget::button::custom(content)
                .width(Length::Fill)
                .padding([12.0f32, 8.0f32])
                .class(cosmic::theme::Button::MenuItem)
                .on_press(Message::SetSpeed(index))
                .into()
        })
        .collect();

    menu = menu
        .popup(
            widget::container(widget::column::with_children(items))
                .padding(4)
                .width(Length::Fixed(120.0))
                .class(cosmic::theme::Container::Dialog(true)),
        )
        .position(widget::popover::Position::Top)
        .on_close(Message::ToggleSpeedMenu);
    menu.into()
}

/// Side panel: output format, its settings, and the Export action
pub fn export_drawer(editor: &Editor) -> cosmic::app::context_drawer::ContextDrawer<'_, Message> {
    let busy = editor.export.is_some();
    let export = widget::container(
        widget::button::suggested(fl!("export")).on_press_maybe((!busy).then_some(Message::Export)),
    )
    .width(Length::Fill)
    .align_x(Alignment::End);

    cosmic::app::context_drawer::context_drawer(
        format_settings(editor),
        Message::ToggleExportDrawer,
    )
    .title(fl!("export"))
    .footer(export)
}

/// Output format and its per-format settings
fn format_settings(editor: &Editor) -> cosmic::Element<'_, Message> {
    let formats: Vec<&'static str> = Format::ALL.iter().map(|f| f.label()).collect();
    let selected = Format::ALL.iter().position(|&f| f == editor.format);
    let mut section = widget::settings::section().add(widget::settings::item(
        fl!("format"),
        widget::dropdown(formats, selected, Message::SetFormat).width(Length::Fixed(120.0)),
    ));

    match editor.format {
        Format::Mp4 | Format::Mkv | Format::Webm => {
            let encoders: Vec<_> = editor.encoders_for(editor.format).collect();
            let selected = editor
                .encoder
                .as_ref()
                .and_then(|chosen| encoders.iter().position(|e| e.kind == chosen.kind));
            let labels: Vec<String> = encoders
                .iter()
                .map(|e| {
                    let kind = if e.kind.is_hardware() {
                        fl!("hardware")
                    } else {
                        fl!("software")
                    };
                    format!("{} · {} ({kind})", e.kind.codec().label(), e.kind.name())
                })
                .collect();
            let qualities = vec![
                fl!("quality-low"),
                fl!("quality-medium"),
                fl!("quality-high"),
            ];
            section = section
                .add(widget::settings::item(
                    fl!("encoder"),
                    widget::dropdown(labels, selected, Message::SetEncoder)
                        .width(Length::Fixed(220.0)),
                ))
                .add(widget::settings::item(
                    fl!("quality"),
                    widget::dropdown(qualities, Some(editor.quality), Message::SetQuality)
                        .width(Length::Fixed(120.0)),
                ));
        }
        Format::Gif => {
            let fps: Vec<String> = export::GIF_FPS.iter().map(|f| format!("{f} fps")).collect();
            let scales: Vec<String> = export::GIF_SCALE.iter().map(|s| format!("{s}%")).collect();
            section = section
                .add(widget::settings::item(
                    fl!("frame-rate"),
                    widget::dropdown(fps, Some(editor.gif_fps), Message::SetGifFps)
                        .width(Length::Fixed(120.0)),
                ))
                .add(widget::settings::item(
                    fl!("scale"),
                    widget::dropdown(scales, Some(editor.gif_scale), Message::SetGifScale)
                        .width(Length::Fixed(120.0)),
                ));
        }
    }

    section.into()
}

fn actions(editor: &Editor) -> cosmic::Element<'_, Message> {
    let busy = editor.export.is_some();
    let mut children: Vec<cosmic::Element<'_, Message>> = Vec::new();

    if editor.can_discard {
        children.push(
            // A text button, in the theme's destructive color: not a call to action.
            widget::button::custom(widget::text(fl!("discard")).class(
                cosmic::theme::Text::Custom(|theme| cosmic::iced::widget::text::Style {
                    color: Some(theme.cosmic().destructive_color().into()),
                    ..Default::default()
                }),
            ))
            .class(cosmic::theme::Button::Text)
            .on_press(Message::Discard)
            .into(),
        );
    }
    children.push(cosmic::iced::widget::space().width(Length::Fill).into());
    children.push(
        widget::button::standard(fl!("export"))
            .on_press(Message::ToggleExportDrawer)
            .into(),
    );
    children.push(
        widget::button::standard(fl!("save-as"))
            .on_press_maybe((!busy).then_some(Message::SaveAs))
            .into(),
    );
    children.push(
        widget::button::suggested(fl!("save"))
            .on_press_maybe((!busy).then_some(Message::Save))
            .into(),
    );

    widget::container(
        widget::row::with_children(children)
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([8, 12])
    .class(cosmic::theme::Container::Card)
    .into()
}
