// SPDX-License-Identifier: GPL-3.0-only

//! The text tool: a live `TextPreview` editor and the actions that drive it.

use cosmic::iced::alignment::Horizontal;
use cosmic::iced::{Point, Size};
use viewer_tools::ToolOperation;
use viewer_tools::annotate::tool::text::{FONT_SIZE_PRESETS_PT, pt_to_px};
use viewer_tools::annotate::{KeyOutcome, TextFormat, TextOperation, TextPreview, TextStyle};

use viewer_tools::annotate::AnnotateColor;

/// The conjoined bold, italic and underline buttons used by the format popup.
#[must_use]
pub fn text_style_model() -> cosmic::widget::segmented_button::MultiSelectModel {
    use cosmic::widget::{icon, segmented_button};

    segmented_button::Model::builder()
        .insert(|button| {
            button
                .icon(icon::from_name("format-text-bold-symbolic").icon())
                .data(viewer_tools::annotate::TextStyle::Bold)
        })
        .insert(|button| {
            button
                .icon(icon::from_name("format-text-italic-symbolic").icon())
                .data(viewer_tools::annotate::TextStyle::Italic)
        })
        .insert(|button| {
            button
                .icon(icon::from_name("format-text-underline-symbolic").icon())
                .data(viewer_tools::annotate::TextStyle::Underline)
        })
        .build()
}

/// The conjoined, single-select alignment buttons used by the format popup.
#[must_use]
pub fn text_align_model() -> cosmic::widget::segmented_button::SingleSelectModel {
    use cosmic::widget::{icon, segmented_button};

    segmented_button::Model::builder()
        .insert(|button| {
            button
                .icon(icon::from_name("format-justify-left-symbolic").icon())
                .data(Horizontal::Left)
                .activate()
        })
        .insert(|button| {
            button
                .icon(icon::from_name("format-justify-center-symbolic").icon())
                .data(Horizontal::Center)
        })
        .insert(|button| {
            button
                .icon(icon::from_name("format-justify-right-symbolic").icon())
                .data(Horizontal::Right)
        })
        .build()
}

/// Make the segmented controls reflect the text format that edits actually use.
pub fn sync_format_models(app: &mut crate::app::App) {
    let Some(format) = app.capture.as_ref().map(|capture| capture.ui.text_format) else {
        return;
    };

    sync_model_state(&mut app.text_style_model, &mut app.text_align_model, format);
}

fn sync_model_state(
    style_model: &mut cosmic::widget::segmented_button::MultiSelectModel,
    align_model: &mut cosmic::widget::segmented_button::SingleSelectModel,
    format: TextFormat,
) {
    use viewer_tools::annotate::TextStyle;

    let style_entities: Vec<_> = style_model.iter().collect();
    for entity in style_entities {
        let active = match style_model.data::<TextStyle>(entity).copied() {
            Some(TextStyle::Bold) => format.bold,
            Some(TextStyle::Italic) => format.italic,
            Some(TextStyle::Underline) => format.underline,
            None => continue,
        };

        if active && !style_model.is_active(entity) {
            style_model.activate(entity);
        } else if !active && style_model.is_active(entity) {
            style_model.deactivate(entity);
        }
    }

    let align_entity = align_model
        .iter()
        .find(|&entity| align_model.data::<Horizontal>(entity).copied() == Some(format.alignment));
    if let Some(entity) = align_entity {
        align_model.activate(entity);
    }
}

/// An open text edit. The preview works in selection-relative coordinates and
/// clamps to the selection. `origin` maps it back to global coordinates.
pub struct TextEdit {
    pub preview: TextPreview,
    /// Selection top-left in global logical coordinates
    pub origin: (f32, f32),
    /// Size of the selection, which the box is clamped to
    pub bounds: Size,
    /// The operation being re-edited, replaced in place on commit.
    pub editing: Option<usize>,
}

impl TextEdit {
    /// Open a new edit with the given styling.
    #[must_use]
    pub fn new(color: AnnotateColor, format: TextFormat, origin: (f32, f32), bounds: Size) -> Self {
        Self {
            preview: TextPreview::with_format(color.into(), &format, crate::fl!("type-here")),
            origin,
            bounds,
            editing: None,
        }
    }

    /// A global point in the preview's own coordinates.
    #[must_use]
    pub fn local(&self, x: f32, y: f32) -> Point {
        Point::new(x - self.origin.0, y - self.origin.1)
    }

    /// Whether the caret is in the box, as opposed to the box still being placed.
    #[must_use]
    pub fn is_editing(&self) -> bool {
        self.preview.is_editing()
    }

    /// The label as a committed operation in global coordinates, if there is
    /// anything in it.
    #[must_use]
    pub fn commit(&self) -> Option<Box<dyn ToolOperation>> {
        if self.preview.is_empty() {
            return None;
        }
        let mut op = self.preview.commit()?;
        op.translate(self.origin.0, self.origin.1);
        Some(op)
    }

    /// Re-open a committed label for editing.
    #[must_use]
    pub fn reopen(index: usize, text: &TextOperation, origin: (f32, f32), bounds: Size) -> Self {
        let mut preview = text.to_preview();
        preview.bounding_box.x -= origin.0;
        preview.bounding_box.y -= origin.1;
        Self {
            preview,
            origin,
            bounds,
            editing: Some(index),
        }
    }
}

use crate::app::{App, Msg};
use crate::capture::msg::TextAction;
use viewer_tools::annotate::TextDragHandle;

/// The selection being captured, as an origin and a size in logical units.
fn selection(app: &App) -> Option<((f32, f32), Size)> {
    use crate::geometry::Choice;

    let capture = app.capture.as_ref()?;
    let rect = match &capture.selection.choice {
        Choice::Rectangle(r, _) => r.dimensions().map(|_| *r)?,
        Choice::Output(Some(name)) => app
            .outputs
            .iter()
            .find(|o| &o.name == name)
            .map(crate::app::OutputState::rect)?,
        Choice::AllScreens => app
            .outputs
            .iter()
            .map(crate::app::OutputState::rect)
            .reduce(crate::geometry::Rect::union)?,
        Choice::Output(None) => return None,
    };
    Some((
        (rect.left as f32, rect.top as f32),
        Size::new(rect.width() as f32, rect.height() as f32),
    ))
}

/// Finish the open edit. Called by every path that ends an edit.
pub fn commit(app: &mut App) {
    let Some(edit) = app.text.take() else {
        return;
    };
    let Some(capture) = app.capture.as_mut() else {
        return;
    };
    match (edit.commit(), edit.editing) {
        // A re-edit replaces the label it came from, keeping its place in the
        // draw order.
        (Some(op), Some(index)) => {
            if let Some(slot) = capture.annotations.stack.operations_mut().get_mut(index) {
                *slot = op;
            } else {
                capture.annotations.commit(op);
            }
        }
        (Some(op), None) => capture.annotations.commit(op),
        // An existing label emptied while re-editing is deleted.
        (None, Some(index)) => {
            capture.annotations.stack.remove(index);
        }
        (None, None) => {}
    }
}

/// Handle one text action.
pub fn handle(app: &mut App, action: TextAction) -> cosmic::Task<Msg> {
    match action {
        TextAction::Press(x, y) => press(app, x, y),
        TextAction::Drag(x, y) => {
            if let Some(edit) = app.text.as_mut() {
                let (p, b) = (edit.local(x, y), edit.bounds);
                edit.preview.on_drag(p, b);
            }
        }
        TextAction::Release(x, y) => {
            if let Some(edit) = app.text.as_mut() {
                let (p, b) = (edit.local(x, y), edit.bounds);
                edit.preview.on_release(p, b);
            }
        }
        TextAction::Key(key, modifiers, text) => {
            let Some(edit) = app.text.as_mut() else {
                return cosmic::Task::none();
            };
            match edit.preview.handle_key(&key, modifiers, text.as_deref()) {
                KeyOutcome::Commit => commit(app),
                KeyOutcome::FormatChanged => adopt_format(app),
                KeyOutcome::Copy(selection) => return cosmic::iced::clipboard::write(selection),
                KeyOutcome::Paste => {
                    return cosmic::iced::clipboard::read().map(|clip| {
                        Msg::Screenshot(crate::capture::msg::Msg::text(TextAction::Paste(
                            clip.unwrap_or_default(),
                        )))
                    });
                }
                KeyOutcome::Consumed | KeyOutcome::Ignored => {}
            }
        }
        TextAction::Paste(clip) => {
            if let Some(edit) = app.text.as_mut() {
                edit.preview.paste(&clip);
            }
        }
        TextAction::Commit => commit(app),
        TextAction::Family(index) => {
            let Some(&family) = app.font_families.get(index) else {
                return cosmic::Task::none();
            };
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.text_format.font_family = family;
            }
            if let Some(edit) = app.text.as_mut() {
                edit.preview.font_family = family;
                edit.preview
                    .apply_attr_to_selection(|attrs| attrs.family(cosmic_family(family)));
            }
        }
        TextAction::Size(index) => {
            let Some(&pt) = FONT_SIZE_PRESETS_PT.get(index) else {
                return cosmic::Task::none();
            };
            let px = pt_to_px(pt);
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.text_format.font_size = px;
            }
            if let Some(edit) = app.text.as_mut() {
                edit.preview.update_font_size(px);
            }
        }
        TextAction::StyleActivated(entity) => {
            let Some(style) = app.text_style_model.data::<TextStyle>(entity).copied() else {
                return cosmic::Task::none();
            };
            style_toggle(app, style);
        }
        TextAction::AlignActivated(entity) => {
            let Some(horizontal) = app.text_align_model.data::<Horizontal>(entity).copied() else {
                return cosmic::Task::none();
            };
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.text_format.alignment = horizontal;
            }
            if let Some(edit) = app.text.as_mut() {
                edit.preview.set_line_alignment(horizontal);
            }
            sync_format_models(app);
        }
        TextAction::TogglePopup => {
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.text_format_popup_open = !capture.ui.text_format_popup_open;
            }
        }
    }
    cosmic::Task::none()
}

/// Take the open edit's styling as the styling for the next label too.
fn adopt_format(app: &mut App) {
    if let (Some(edit), Some(capture)) = (app.text.as_ref(), app.capture.as_mut()) {
        capture.ui.text_format = edit.preview.format();
    }
    sync_format_models(app);
}

/// A family name as `cosmic_text` wants it.
const fn cosmic_family(
    name: &'static str,
) -> cosmic::iced::advanced::graphics::text::cosmic_text::Family<'static> {
    cosmic::iced::advanced::graphics::text::cosmic_text::Family::Name(name)
}

fn style_toggle(app: &mut App, style: TextStyle) {
    if let Some(edit) = app.text.as_mut() {
        edit.preview.toggle_style(style);
        adopt_format(app);
        return;
    }
    if let Some(capture) = app.capture.as_mut() {
        let format = &mut capture.ui.text_format;
        match style {
            TextStyle::Bold => format.bold = !format.bold,
            TextStyle::Italic => format.italic = !format.italic,
            TextStyle::Underline => format.underline = !format.underline,
        }
    }
    sync_format_models(app);
}

fn press(app: &mut App, x: f32, y: f32) {
    // On the box being edited: let the preview place the caret, start a
    // selection, or grab a resize handle.
    if let Some(edit) = app.text.as_mut() {
        let point = edit.local(x, y);
        if edit.preview.handle_at(point) != TextDragHandle::None {
            let bounds = edit.bounds;
            edit.preview.on_press(point, bounds);
            return;
        }
        // Clicking away commits this label and nothing else. The next click
        // starts a new label, so a commit never also creates a stray one.
        commit(app);
        return;
    }

    let Some((origin, bounds)) = selection(app) else {
        return;
    };
    let Some(capture) = app.capture.as_ref() else {
        return;
    };
    let format = capture.ui.text_format;
    let color = capture.ui.shape_color;

    // On an existing label, re-open it instead of starting a new one.
    let existing = capture
        .annotations
        .operations()
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, op)| {
            let text = op.as_any().downcast_ref::<TextOperation>()?;
            text.hit_test(Point::new(x, y)).then(|| (i, text.clone()))
        });

    let mut edit = match existing {
        Some((index, text)) => TextEdit::reopen(index, &text, origin, bounds),
        None => TextEdit::new(color, format, origin, bounds),
    };
    let point = edit.local(x, y);
    edit.preview.on_press(point, bounds);
    app.text = Some(edit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_follow_format() {
        let mut styles = text_style_model();
        let mut alignment = text_align_model();
        let format = TextFormat {
            bold: true,
            italic: false,
            underline: true,
            alignment: Horizontal::Right,
            ..TextFormat::default()
        };

        sync_model_state(&mut styles, &mut alignment, format);

        let active_styles: Vec<_> = styles
            .iter()
            .filter(|&entity| styles.is_active(entity))
            .filter_map(|entity| styles.data::<TextStyle>(entity).copied())
            .collect();
        assert_eq!(active_styles, [TextStyle::Bold, TextStyle::Underline]);

        let active_alignment: Vec<_> = alignment
            .iter()
            .filter(|&entity| alignment.is_active(entity))
            .filter_map(|entity| alignment.data::<Horizontal>(entity).copied())
            .collect();
        assert_eq!(active_alignment, [Horizontal::Right]);
    }

    #[test]
    fn reset_clears_style_segments() {
        let mut styles = text_style_model();
        let mut alignment = text_align_model();
        let selected = TextFormat {
            bold: true,
            italic: true,
            underline: true,
            alignment: Horizontal::Center,
            ..TextFormat::default()
        };
        sync_model_state(&mut styles, &mut alignment, selected);
        sync_model_state(&mut styles, &mut alignment, TextFormat::default());

        assert!(styles.iter().all(|entity| !styles.is_active(entity)));
        let active_alignment = alignment
            .iter()
            .find(|&entity| alignment.is_active(entity))
            .and_then(|entity| alignment.data::<Horizontal>(entity).copied());
        assert_eq!(active_alignment, Some(Horizontal::Left));
    }
}
