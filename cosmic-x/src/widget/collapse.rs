// SPDX-License-Identifier: GPL-3.0-only

//! A row section whose width eases between zero and its natural size, so
//! toolbar sections slide in and out instead of popping.

use cosmic::Element;
use cosmic::iced::Size;
use cosmic::iced::core::renderer::Renderer as _;
use cosmic::iced::core::{Layout, Length, Point, Rectangle, layout, mouse, widget::Tree};

/// A row section whose width is a fraction of its contents'.
pub struct Collapse<'a, Msg> {
    content: Element<'a, Msg>,
    progress: f32,
}

impl<'a, Msg> Collapse<'a, Msg> {
    /// Wrap `content`, showing `progress` of its width (0 = gone, 1 = whole).
    pub fn new(content: impl Into<Element<'a, Msg>>, progress: f32) -> Self {
        Self {
            content: content.into(),
            progress: progress.clamp(0.0, 1.0),
        }
    }

    /// Whether the section is fully open and usable.
    fn is_open(&self) -> bool {
        self.progress >= 1.0
    }
}

impl<Msg: Clone + 'static> cosmic::widget::Widget<Msg, cosmic::Theme, cosmic::Renderer>
    for Collapse<'_, Msg>
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &cosmic::iced::Limits,
    ) -> layout::Node {
        // Natural size at any progress. The contents are revealed by clipping and never reflowed.
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let natural = content.size();

        // Held against the right edge, so the contents and everything after them
        // in the row move as one.
        let offset = -natural.width * (1.0 - self.progress);

        layout::Node::with_children(
            Size::new(natural.width * self.progress, natural.height),
            vec![content.move_to(Point::new(offset, 0.0))],
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced::core::renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if bounds.width <= 0.0 {
            return;
        }
        let Some(content) = layout.children().next() else {
            return;
        };

        // Clip to our share of the row: the contents stick out to the left of it
        // for as long as the section is opening.
        renderer.with_layer(bounds, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                content,
                cursor,
                viewport,
            );
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &cosmic::iced::core::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn cosmic::iced::core::Clipboard,
        shell: &mut cosmic::iced::core::Shell<'_, Msg>,
        viewport: &Rectangle,
    ) {
        if !self.is_open() {
            return;
        }
        let Some(content) = layout.children().next() else {
            return;
        };
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        if !self.is_open() {
            return mouse::Interaction::default();
        }
        layout
            .children()
            .next()
            .map_or_else(mouse::Interaction::default, |content| {
                self.content.as_widget().mouse_interaction(
                    &tree.children[0],
                    content,
                    cursor,
                    viewport,
                    renderer,
                )
            })
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn cosmic::iced::core::widget::Operation,
    ) {
        if let Some(content) = layout.children().next() {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                content,
                renderer,
                operation,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        translation: cosmic::iced::Vector,
    ) -> Option<cosmic::iced::core::overlay::Element<'b, Msg, cosmic::Theme, cosmic::Renderer>>
    {
        // A popup anchored to a button that is still sliding in would be left
        // hanging in mid-air, so only an open section gets one.
        if !self.is_open() {
            return None;
        }
        let content = layout.children().next()?;
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            content,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Msg: Clone + 'static> From<Collapse<'a, Msg>> for Element<'a, Msg> {
    fn from(collapse: Collapse<'a, Msg>) -> Self {
        Element::new(collapse)
    }
}
