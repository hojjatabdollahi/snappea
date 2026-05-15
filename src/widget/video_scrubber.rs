#![allow(clippy::type_complexity)]

use cosmic::iced::core::{
    Background, Border, Element, Layout, Length, Pixels, Point, Rectangle, Shell, Size, Widget,
    event::Event,
    keyboard,
    layout,
    mouse::{self, Cursor},
    renderer::{self, Quad, Renderer as RendererTrait},
    text::{Renderer as TextRenderer, Text},
    touch,
    widget::tree::{self, Tree},
};
use cosmic::iced::Color;
use std::collections::HashSet;

const COLOR_AREA_HEIGHT: f32 = 40.0;
const LABEL_HEIGHT: f32 = 18.0;
const TICK_HEIGHT: f32 = 5.0;
const PLAYHEAD_WIDTH: f32 = 2.0;
const CUT_LINE_WIDTH: f32 = 2.0;
const PLAYHEAD_TRI_SIZE: f32 = 6.0;
const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 100.0;
const ZOOM_SPEED: f32 = 0.15;

struct ScrubberState {
    dragging: bool,
    zoom: f32,
    scroll_offset: f32,
    keyboard_modifiers: keyboard::Modifiers,
}

impl Default for ScrubberState {
    fn default() -> Self {
        Self {
            dragging: false,
            zoom: MIN_ZOOM,
            scroll_offset: 0.0,
            keyboard_modifiers: keyboard::Modifiers::default(),
        }
    }
}

pub struct VideoScrubber<'a, Message> {
    duration: f64,
    position: f64,
    colors: &'a [[u8; 3]],
    cuts: &'a [f64],
    deleted_chunks: &'a HashSet<usize>,
    selected_chunk: Option<usize>,
    height: f32,
    on_seek: Option<Box<dyn Fn(f64) -> Message + 'a>>,
    on_release: Option<Message>,
    on_select_chunk: Option<Box<dyn Fn(Option<usize>) -> Message + 'a>>,
}

impl<'a, Message> VideoScrubber<'a, Message>
where
    Message: Clone,
{
    pub fn new(duration: f64, position: f64) -> Self {
        Self {
            duration,
            position,
            colors: &[],
            cuts: &[],
            deleted_chunks: &EMPTY_SET,
            selected_chunk: None,
            height: COLOR_AREA_HEIGHT + LABEL_HEIGHT,
            on_seek: None,
            on_release: None,
            on_select_chunk: None,
        }
    }

    pub fn colors(mut self, colors: &'a [[u8; 3]]) -> Self {
        self.colors = colors;
        self
    }

    pub fn cuts(mut self, cuts: &'a [f64]) -> Self {
        self.cuts = cuts;
        self
    }

    pub fn deleted_chunks(mut self, deleted: &'a HashSet<usize>) -> Self {
        self.deleted_chunks = deleted;
        self
    }

    pub fn selected_chunk(mut self, sel: Option<usize>) -> Self {
        self.selected_chunk = sel;
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn on_seek(mut self, f: impl Fn(f64) -> Message + 'a) -> Self {
        self.on_seek = Some(Box::new(f));
        self
    }

    pub fn on_release(mut self, msg: Message) -> Self {
        self.on_release = Some(msg);
        self
    }

    pub fn on_select_chunk(mut self, f: impl Fn(Option<usize>) -> Message + 'a) -> Self {
        self.on_select_chunk = Some(Box::new(f));
        self
    }
}

static EMPTY_SET: std::sync::LazyLock<HashSet<usize>> =
    std::sync::LazyLock::new(HashSet::new);

fn time_to_screen_x(time: f64, duration: f64, zoom: f32, scroll_offset: f32, width: f32) -> f32 {
    if duration <= 0.0 {
        return 0.0;
    }
    let virtual_width = width * zoom;
    let virtual_x = (time / duration) as f32 * virtual_width;
    virtual_x - scroll_offset
}

fn screen_x_to_time(
    sx: f32,
    duration: f64,
    zoom: f32,
    scroll_offset: f32,
    width: f32,
) -> f64 {
    let virtual_width = width * zoom;
    if virtual_width <= 0.0 {
        return 0.0;
    }
    let virtual_x = sx + scroll_offset;
    let t = (virtual_x as f64 / virtual_width as f64) * duration;
    t.clamp(0.0, duration)
}

fn chunk_index_at_time(time: f64, cuts: &[f64]) -> usize {
    match cuts.iter().position(|&c| time < c) {
        Some(i) => i,
        None => cuts.len(),
    }
}

fn chunk_time_range(chunk: usize, cuts: &[f64], duration: f64) -> (f64, f64) {
    let start = if chunk == 0 { 0.0 } else { cuts[chunk - 1] };
    let end = if chunk < cuts.len() {
        cuts[chunk]
    } else {
        duration
    };
    (start, end)
}

fn color_at_time(time: f64, duration: f64, colors: &[[u8; 3]]) -> [u8; 3] {
    if colors.is_empty() || duration <= 0.0 {
        return [40, 40, 40];
    }
    let frac = (time / duration).clamp(0.0, 1.0);
    let idx = ((frac * colors.len() as f64) as usize).min(colors.len() - 1);
    colors[idx]
}

fn pick_tick_interval(visible_duration: f64) -> (f64, usize) {
    const NICE: &[(f64, usize)] = &[
        (0.01, 2),
        (0.02, 2),
        (0.05, 2),
        (0.1, 1),
        (0.2, 1),
        (0.5, 1),
        (1.0, 0),
        (2.0, 0),
        (5.0, 0),
        (10.0, 0),
        (15.0, 0),
        (30.0, 0),
        (60.0, 0),
        (120.0, 0),
        (300.0, 0),
        (600.0, 0),
    ];
    let target_ticks = 6.0;
    for &(interval, decimals) in NICE {
        let count = visible_duration / interval;
        if count <= target_ticks * 2.0 && count >= 2.0 {
            return (interval, decimals);
        }
    }
    (*NICE.last().map(|(i, _)| i).unwrap_or(&10.0), 0)
}

fn format_time(seconds: f64, decimals: usize) -> String {
    let total_secs = seconds.abs();
    let mins = (total_secs / 60.0) as u32;
    let secs = total_secs % 60.0;
    if decimals == 0 {
        format!("{}:{:02}", mins, secs as u32)
    } else {
        format!("{}:{:0>width$.prec$}", mins, secs, width = 3 + decimals, prec = decimals)
    }
}

fn clamp_scroll(scroll: f32, zoom: f32, width: f32) -> f32 {
    let max_scroll = (width * zoom - width).max(0.0);
    scroll.clamp(0.0, max_scroll)
}

impl<Message: Clone + 'static> Widget<Message, cosmic::Theme, cosmic::Renderer>
    for VideoScrubber<'_, Message>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ScrubberState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ScrubberState::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, self.height)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn cosmic::iced::core::Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<ScrubberState>();
        let bounds = layout.bounds();
        let width = bounds.width;

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    let local_x = pos.x - bounds.x;
                    let time =
                        screen_x_to_time(local_x, self.duration, state.zoom, state.scroll_offset, width);

                    let chunk = chunk_index_at_time(time, self.cuts);
                    if let Some(ref cb) = self.on_select_chunk {
                        shell.publish(cb(Some(chunk)));
                    }

                    if let Some(ref cb) = self.on_seek {
                        shell.publish(cb(time));
                    }

                    state.dragging = true;
                    shell.capture_event();
                }
            }

            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
            | Event::Touch(touch::Event::FingerLost { .. }) => {
                if state.dragging {
                    state.dragging = false;
                    if let Some(msg) = self.on_release.clone() {
                        shell.publish(msg);
                    }
                }
            }

            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.dragging {
                    if let Some(pos) = cursor.land().position() {
                        let local_x = (pos.x - bounds.x).clamp(0.0, width);
                        let time = screen_x_to_time(
                            local_x,
                            self.duration,
                            state.zoom,
                            state.scroll_offset,
                            width,
                        );
                        if let Some(ref cb) = self.on_seek {
                            shell.publish(cb(time));
                        }
                        shell.capture_event();
                    }
                }
            }

            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let (dx, dy) = match delta {
                        mouse::ScrollDelta::Lines { x, y } => (*x * 20.0, *y),
                        mouse::ScrollDelta::Pixels { x, y } => (*x, *y / 20.0),
                    };

                    if state.keyboard_modifiers.control() {
                        let mouse_x = cursor
                            .position()
                            .map(|p| p.x - bounds.x)
                            .unwrap_or(width / 2.0);

                        let time_at_cursor = screen_x_to_time(
                            mouse_x,
                            self.duration,
                            state.zoom,
                            state.scroll_offset,
                            width,
                        );

                        let factor = (1.0 + ZOOM_SPEED * dy).max(0.5).min(2.0);
                        state.zoom = (state.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);

                        let new_virtual_x =
                            (time_at_cursor / self.duration) as f32 * width * state.zoom;
                        state.scroll_offset =
                            clamp_scroll(new_virtual_x - mouse_x, state.zoom, width);
                    } else if state.zoom > MIN_ZOOM {
                        state.scroll_offset =
                            clamp_scroll(state.scroll_offset - dx, state.zoom, width);
                    }

                    shell.capture_event();
                    shell.request_redraw();
                }
            }

            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                state.keyboard_modifiers = *m;
            }

            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        _theme: &cosmic::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<ScrubberState>();
        let bounds = layout.bounds();
        let width = bounds.width;
        let color_h = self.height - LABEL_HEIGHT;

        let zoom = state.zoom;
        let scroll = state.scroll_offset;

        // Background
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: bounds.y,
                    width,
                    height: color_h,
                },
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(Color::from_rgba(0.15, 0.15, 0.15, 1.0)),
        );

        // Draw color columns
        if !self.colors.is_empty() && self.duration > 0.0 {
            let col_width = 1.0_f32;
            let cols = width as usize;
            for i in 0..cols {
                let sx = i as f32;
                let time = screen_x_to_time(sx, self.duration, zoom, scroll, width);
                if time < 0.0 || time > self.duration {
                    continue;
                }
                let [r, g, b] = color_at_time(time, self.duration, self.colors);
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: bounds.x + sx,
                            y: bounds.y,
                            width: col_width,
                            height: color_h,
                        },
                        ..Quad::default()
                    },
                    Background::Color(Color::from_rgb8(r, g, b)),
                );
            }
        }

        // Draw deleted chunk overlays
        let num_chunks = self.cuts.len() + 1;
        for chunk_idx in 0..num_chunks {
            if !self.deleted_chunks.contains(&chunk_idx) {
                continue;
            }
            let (t_start, t_end) = chunk_time_range(chunk_idx, self.cuts, self.duration);
            let x_start = time_to_screen_x(t_start, self.duration, zoom, scroll, width);
            let x_end = time_to_screen_x(t_end, self.duration, zoom, scroll, width);
            let cx = x_start.max(0.0);
            let cw = (x_end.min(width) - cx).max(0.0);
            if cw > 0.0 {
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: bounds.x + cx,
                            y: bounds.y,
                            width: cw,
                            height: color_h,
                        },
                        ..Quad::default()
                    },
                    Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.6)),
                );
                // Draw diagonal lines pattern for deleted
                let stripe_spacing = 8.0;
                let mut sx = cx - color_h;
                while sx < cx + cw {
                    let x1 = sx.max(cx);
                    let x2 = (sx + color_h).min(cx + cw);
                    if x2 > x1 {
                        renderer.fill_quad(
                            Quad {
                                bounds: Rectangle {
                                    x: bounds.x + (x1 + x2) / 2.0 - 0.5,
                                    y: bounds.y,
                                    width: 1.0,
                                    height: color_h,
                                },
                                ..Quad::default()
                            },
                            Background::Color(Color::from_rgba(1.0, 0.3, 0.3, 0.3)),
                        );
                    }
                    sx += stripe_spacing;
                }
            }
        }

        // Draw selected chunk highlight
        if let Some(sel) = self.selected_chunk {
            if sel < num_chunks {
                let (t_start, t_end) = chunk_time_range(sel, self.cuts, self.duration);
                let x_start = time_to_screen_x(t_start, self.duration, zoom, scroll, width);
                let x_end = time_to_screen_x(t_end, self.duration, zoom, scroll, width);
                let cx = x_start.max(0.0);
                let cw = (x_end.min(width) - cx).max(0.0);
                if cw > 0.0 {
                    let sel_color = Color::from_rgba(0.3, 0.6, 1.0, 0.25);
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle {
                                x: bounds.x + cx,
                                y: bounds.y,
                                width: cw,
                                height: color_h,
                            },
                            ..Quad::default()
                        },
                        Background::Color(sel_color),
                    );
                    // Border top and bottom
                    let border_color = Color::from_rgba(0.3, 0.6, 1.0, 0.7);
                    for y_off in [0.0, color_h - 2.0] {
                        renderer.fill_quad(
                            Quad {
                                bounds: Rectangle {
                                    x: bounds.x + cx,
                                    y: bounds.y + y_off,
                                    width: cw,
                                    height: 2.0,
                                },
                                ..Quad::default()
                            },
                            Background::Color(border_color),
                        );
                    }
                }
            }
        }

        // Draw cut markers
        let cut_color = Color::from_rgba(1.0, 0.85, 0.0, 0.9);
        for &cut_time in self.cuts {
            let cx = time_to_screen_x(cut_time, self.duration, zoom, scroll, width);
            if cx >= -CUT_LINE_WIDTH && cx <= width + CUT_LINE_WIDTH {
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: bounds.x + cx - CUT_LINE_WIDTH / 2.0,
                            y: bounds.y,
                            width: CUT_LINE_WIDTH,
                            height: color_h,
                        },
                        ..Quad::default()
                    },
                    Background::Color(cut_color),
                );
            }
        }

        // Draw playhead
        if self.duration > 0.0 {
            let ph_x = time_to_screen_x(self.position, self.duration, zoom, scroll, width);
            if ph_x >= -PLAYHEAD_WIDTH && ph_x <= width + PLAYHEAD_WIDTH {
                // Vertical line
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: bounds.x + ph_x - PLAYHEAD_WIDTH / 2.0,
                            y: bounds.y,
                            width: PLAYHEAD_WIDTH,
                            height: color_h,
                        },
                        ..Quad::default()
                    },
                    Background::Color(Color::WHITE),
                );
                // Triangle at top
                let tri_y = bounds.y;
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: bounds.x + ph_x - PLAYHEAD_TRI_SIZE / 2.0,
                            y: tri_y,
                            width: PLAYHEAD_TRI_SIZE,
                            height: PLAYHEAD_TRI_SIZE,
                        },
                        border: Border {
                            radius: 1.0.into(),
                            ..Border::default()
                        },
                        ..Quad::default()
                    },
                    Background::Color(Color::WHITE),
                );
            }
        }

        // Draw time labels and ticks
        let label_y = bounds.y + color_h;
        let visible_duration = self.duration / zoom as f64;
        let (tick_interval, decimals) = pick_tick_interval(visible_duration);

        // Label area background
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: label_y,
                    width,
                    height: LABEL_HEIGHT,
                },
                ..Quad::default()
            },
            Background::Color(Color::from_rgba(0.1, 0.1, 0.1, 1.0)),
        );

        let visible_start = screen_x_to_time(0.0, self.duration, zoom, scroll, width);
        let visible_end = screen_x_to_time(width, self.duration, zoom, scroll, width);
        let first_tick = (visible_start / tick_interval).floor() as i64;
        let last_tick = (visible_end / tick_interval).ceil() as i64;

        let tick_color = Color::from_rgba(0.5, 0.5, 0.5, 1.0);
        let label_color = Color::from_rgba(0.7, 0.7, 0.7, 1.0);

        for i in first_tick..=last_tick {
            let time = i as f64 * tick_interval;
            if time < 0.0 || time > self.duration {
                continue;
            }
            let tx = time_to_screen_x(time, self.duration, zoom, scroll, width);
            if tx < -50.0 || tx > width + 50.0 {
                continue;
            }

            // Tick mark
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: bounds.x + tx - 0.5,
                        y: label_y,
                        width: 1.0,
                        height: TICK_HEIGHT,
                    },
                    ..Quad::default()
                },
                Background::Color(tick_color),
            );

            // Label
            let label = format_time(time, decimals);
            let label_w = 60.0_f32;
            renderer.fill_text(
                Text {
                    content: label,
                    bounds: Size::new(label_w, LABEL_HEIGHT),
                    size: Pixels(10.0),
                    line_height: cosmic::iced::core::text::LineHeight::Relative(1.0),
                    font: cosmic::iced::Font::default(),
                    align_x: cosmic::iced::alignment::Horizontal::Center.into(),
                    align_y: cosmic::iced::alignment::Vertical::Top,
                    shaping: cosmic::iced::core::text::Shaping::Basic,
                    wrapping: cosmic::iced::core::text::Wrapping::None,
                    ellipsize: cosmic::iced::core::text::Ellipsize::default(),
                },
                Point::new(bounds.x + tx, label_y + TICK_HEIGHT),
                label_color,
                Rectangle {
                    x: bounds.x,
                    y: label_y,
                    width,
                    height: LABEL_HEIGHT,
                },
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        _viewport: &Rectangle,
        _renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<ScrubberState>();
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message: Clone + 'static> From<VideoScrubber<'a, Message>>
    for Element<'a, Message, cosmic::Theme, cosmic::Renderer>
{
    fn from(scrubber: VideoScrubber<'a, Message>) -> Self {
        Element::new(scrubber)
    }
}

pub fn video_scrubber<'a, Message: Clone + 'static>(
    duration: f64,
    position: f64,
) -> VideoScrubber<'a, Message> {
    VideoScrubber::new(duration, position)
}
