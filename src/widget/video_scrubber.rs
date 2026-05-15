#![allow(clippy::type_complexity)]

use cosmic::iced::Color;
use cosmic::iced::core::{
    Background, Border, Element, Layout, Length, Pixels, Point, Rectangle, Shell, Size, Widget,
    event::Event,
    keyboard, layout,
    mouse::{self, Cursor},
    renderer::{self, Quad, Renderer as RendererTrait},
    text::{Renderer as TextRenderer, Text},
    touch,
    widget::tree::{self, Tree},
};
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
const FOLLOW_MARGIN: f32 = 0.15;

const HANDLE_WIDTH: f32 = 10.0;
const HANDLE_HEIGHT: f32 = 10.0;
const HANDLE_HIT_RADIUS: f32 = 8.0;
const EDGE_SNAP_THRESHOLD: f64 = 0.005;

#[derive(Debug, Clone, Copy)]
enum DragTarget {
    Playhead,
    Cut(usize),
    EdgeStart,
    EdgeEnd,
}

struct ScrubberState {
    drag: Option<DragTarget>,
    drag_time: f64,
    zoom: f32,
    scroll_offset: f32,
    keyboard_modifiers: keyboard::Modifiers,
}

impl Default for ScrubberState {
    fn default() -> Self {
        Self {
            drag: None,
            drag_time: 0.0,
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
    on_cut_moved: Option<Box<dyn Fn(usize, f64) -> Message + 'a>>,
    on_cut_added: Option<Box<dyn Fn(f64) -> Message + 'a>>,
    on_cut_removed: Option<Box<dyn Fn(usize) -> Message + 'a>>,
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
            on_cut_moved: None,
            on_cut_added: None,
            on_cut_removed: None,
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

    pub fn on_cut_moved(mut self, f: impl Fn(usize, f64) -> Message + 'a) -> Self {
        self.on_cut_moved = Some(Box::new(f));
        self
    }

    pub fn on_cut_added(mut self, f: impl Fn(f64) -> Message + 'a) -> Self {
        self.on_cut_added = Some(Box::new(f));
        self
    }

    pub fn on_cut_removed(mut self, f: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_cut_removed = Some(Box::new(f));
        self
    }
}

static EMPTY_SET: std::sync::LazyLock<HashSet<usize>> = std::sync::LazyLock::new(HashSet::new);

fn time_to_screen_x(time: f64, duration: f64, zoom: f32, scroll_offset: f32, width: f32) -> f32 {
    if duration <= 0.0 {
        return 0.0;
    }
    let virtual_width = width * zoom;
    let virtual_x = (time / duration) as f32 * virtual_width;
    virtual_x - scroll_offset
}

fn screen_x_to_time(sx: f32, duration: f64, zoom: f32, scroll_offset: f32, width: f32) -> f64 {
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
        format!(
            "{}:{:0>width$.prec$}",
            mins,
            secs,
            width = 3 + decimals,
            prec = decimals
        )
    }
}

fn clamp_scroll(scroll: f32, zoom: f32, width: f32) -> f32 {
    let max_scroll = (width * zoom - width).max(0.0);
    scroll.clamp(0.0, max_scroll)
}

fn ensure_visible(position: f64, duration: f64, zoom: f32, scroll: f32, width: f32) -> f32 {
    if zoom <= MIN_ZOOM || duration <= 0.0 {
        return scroll;
    }
    let ph_x = time_to_screen_x(position, duration, zoom, scroll, width);
    let margin = width * FOLLOW_MARGIN;
    if ph_x < margin {
        clamp_scroll(scroll - (margin - ph_x), zoom, width)
    } else if ph_x > width - margin {
        clamp_scroll(scroll + (ph_x - (width - margin)), zoom, width)
    } else {
        scroll
    }
}

fn hit_test_boundary(
    local_x: f32,
    duration: f64,
    cuts: &[f64],
    zoom: f32,
    scroll: f32,
    width: f32,
) -> Option<DragTarget> {
    let start_x = time_to_screen_x(0.0, duration, zoom, scroll, width);
    if (local_x - start_x).abs() < HANDLE_HIT_RADIUS {
        return Some(DragTarget::EdgeStart);
    }
    let end_x = time_to_screen_x(duration, duration, zoom, scroll, width);
    if (local_x - end_x).abs() < HANDLE_HIT_RADIUS {
        return Some(DragTarget::EdgeEnd);
    }
    for (i, &cut) in cuts.iter().enumerate() {
        let cx = time_to_screen_x(cut, duration, zoom, scroll, width);
        if (local_x - cx).abs() < HANDLE_HIT_RADIUS {
            return Some(DragTarget::Cut(i));
        }
    }
    None
}

fn draw_handle(renderer: &mut cosmic::Renderer, center_x: f32, bottom_y: f32, color: Color) {
    let hx = center_x - HANDLE_WIDTH / 2.0;
    let hy = bottom_y - HANDLE_HEIGHT;
    // Tab body
    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x: hx,
                y: hy,
                width: HANDLE_WIDTH,
                height: HANDLE_HEIGHT,
            },
            border: Border {
                radius: [3.0, 3.0, 0.0, 0.0].into(),
                ..Border::default()
            },
            ..Quad::default()
        },
        Background::Color(color),
    );
    // Small triangle pointer at top (diamond-ish tip)
    let tip_w = 4.0;
    let tip_h = 4.0;
    renderer.fill_quad(
        Quad {
            bounds: Rectangle {
                x: center_x - tip_w / 2.0,
                y: hy - tip_h + 1.0,
                width: tip_w,
                height: tip_h,
            },
            border: Border {
                radius: [2.0, 2.0, 0.0, 0.0].into(),
                ..Border::default()
            },
            ..Quad::default()
        },
        Background::Color(color),
    );
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

        if state.drag.is_none() && state.zoom > MIN_ZOOM {
            state.scroll_offset = ensure_visible(
                self.position,
                self.duration,
                state.zoom,
                state.scroll_offset,
                width,
            );
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(pos) = cursor.position_over(bounds) {
                    let local_x = pos.x - bounds.x;
                    let time = screen_x_to_time(
                        local_x,
                        self.duration,
                        state.zoom,
                        state.scroll_offset,
                        width,
                    );

                    if let Some(target) = hit_test_boundary(
                        local_x,
                        self.duration,
                        self.cuts,
                        state.zoom,
                        state.scroll_offset,
                        width,
                    ) {
                        state.drag = Some(target);
                        state.drag_time = match target {
                            DragTarget::EdgeStart => 0.0,
                            DragTarget::EdgeEnd => self.duration,
                            DragTarget::Cut(i) => self.cuts[i],
                            DragTarget::Playhead => time,
                        };
                    } else {
                        state.drag = Some(DragTarget::Playhead);
                        let chunk = chunk_index_at_time(time, self.cuts);
                        if let Some(ref cb) = self.on_select_chunk {
                            shell.publish(cb(Some(chunk)));
                        }
                        if let Some(ref cb) = self.on_seek {
                            shell.publish(cb(time));
                        }
                    }
                    shell.capture_event();
                }
            }

            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
            | Event::Touch(touch::Event::FingerLost { .. }) => {
                if let Some(target) = state.drag.take() {
                    let drag_time = state.drag_time;
                    match target {
                        DragTarget::Playhead => {
                            if let Some(msg) = self.on_release.clone() {
                                shell.publish(msg);
                            }
                        }
                        DragTarget::Cut(i) => {
                            if drag_time <= EDGE_SNAP_THRESHOLD * self.duration
                                || drag_time >= self.duration * (1.0 - EDGE_SNAP_THRESHOLD)
                            {
                                if let Some(ref cb) = self.on_cut_removed {
                                    shell.publish(cb(i));
                                }
                            } else if let Some(ref cb) = self.on_cut_moved {
                                shell.publish(cb(i, drag_time));
                            }
                        }
                        DragTarget::EdgeStart | DragTarget::EdgeEnd => {
                            let near_start = drag_time <= EDGE_SNAP_THRESHOLD * self.duration;
                            let near_end = drag_time >= self.duration * (1.0 - EDGE_SNAP_THRESHOLD);
                            if !near_start && !near_end {
                                if let Some(ref cb) = self.on_cut_added {
                                    shell.publish(cb(drag_time));
                                }
                            }
                        }
                    }
                }
            }

            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if let Some(target) = state.drag {
                    if let Some(pos) = cursor.land().position() {
                        let local_x = (pos.x - bounds.x).clamp(0.0, width);
                        let time = screen_x_to_time(
                            local_x,
                            self.duration,
                            state.zoom,
                            state.scroll_offset,
                            width,
                        );
                        match target {
                            DragTarget::Playhead => {
                                if let Some(ref cb) = self.on_seek {
                                    shell.publish(cb(time));
                                }
                            }
                            DragTarget::Cut(_) | DragTarget::EdgeStart | DragTarget::EdgeEnd => {
                                state.drag_time = time;
                            }
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
                        let new_vx = (time_at_cursor / self.duration) as f32 * width * state.zoom;
                        state.scroll_offset = clamp_scroll(new_vx - mouse_x, state.zoom, width);
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

        let color_bounds = Rectangle {
            x: bounds.x,
            y: bounds.y,
            width,
            height: color_h,
        };
        let label_bounds = Rectangle {
            x: bounds.x,
            y: bounds.y + color_h,
            width,
            height: LABEL_HEIGHT,
        };

        // Background
        renderer.fill_quad(
            Quad {
                bounds: color_bounds,
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(Color::from_rgba(0.15, 0.15, 0.15, 1.0)),
        );

        // Color columns
        if !self.colors.is_empty() && self.duration > 0.0 {
            for i in 0..width as usize {
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
                            width: 1.0,
                            height: color_h,
                        },
                        ..Quad::default()
                    },
                    Background::Color(Color::from_rgb8(r, g, b)),
                );
            }
        }

        // Deleted chunk overlays
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
                    Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.75)),
                );
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
                    Background::Color(Color::from_rgba(0.6, 0.1, 0.1, 0.4)),
                );
                let del_border = Color::from_rgba(0.8, 0.2, 0.2, 0.8);
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
                        Background::Color(del_border),
                    );
                }
                let stripe_spacing = 10.0;
                let stripe_w = 2.0;
                let mut sx = cx - color_h;
                while sx < cx + cw {
                    let x_mid = bounds.x + (sx + color_h / 2.0).max(cx).min(cx + cw);
                    if x_mid >= bounds.x + cx && x_mid <= bounds.x + cx + cw {
                        renderer.fill_quad(
                            Quad {
                                bounds: Rectangle {
                                    x: x_mid - stripe_w / 2.0,
                                    y: bounds.y,
                                    width: stripe_w,
                                    height: color_h,
                                },
                                ..Quad::default()
                            },
                            Background::Color(Color::from_rgba(0.9, 0.2, 0.2, 0.35)),
                        );
                    }
                    sx += stripe_spacing;
                }
            }
        }

        // Selected chunk highlight
        if let Some(sel) = self.selected_chunk {
            if sel < num_chunks {
                let (t_start, t_end) = chunk_time_range(sel, self.cuts, self.duration);
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
                        Background::Color(Color::from_rgba(0.3, 0.6, 1.0, 0.25)),
                    );
                    let bc = Color::from_rgba(0.3, 0.6, 1.0, 0.7);
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
                            Background::Color(bc),
                        );
                    }
                }
            }
        }

        // Cut lines + handles
        let cut_color = Color::from_rgba(1.0, 0.85, 0.0, 0.9);
        let handle_bottom = bounds.y + color_h;

        // Determine effective cut positions (accounting for active drag)
        let effective_cuts: Vec<(f64, bool)> = self
            .cuts
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                if let Some(DragTarget::Cut(di)) = state.drag {
                    if di == i {
                        return (state.drag_time, true);
                    }
                }
                (c, false)
            })
            .collect();

        for (cut_time, is_dragging) in &effective_cuts {
            let cx = time_to_screen_x(*cut_time, self.duration, zoom, scroll, width);
            if cx >= 0.0 && cx <= width {
                let lc = if *is_dragging {
                    Color::from_rgba(1.0, 1.0, 0.4, 1.0)
                } else {
                    cut_color
                };
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
                    Background::Color(lc),
                );
                draw_handle(renderer, bounds.x + cx, handle_bottom, lc);
            }
        }

        // Edge handles (always visible)
        let edge_color = Color::from_rgba(0.7, 0.7, 0.7, 0.8);
        let start_x = time_to_screen_x(0.0, self.duration, zoom, scroll, width);
        let end_x = time_to_screen_x(self.duration, self.duration, zoom, scroll, width);

        if start_x >= -HANDLE_WIDTH && start_x <= width + HANDLE_WIDTH {
            draw_handle(renderer, bounds.x + start_x, handle_bottom, edge_color);
        }
        if end_x >= -HANDLE_WIDTH && end_x <= width + HANDLE_WIDTH {
            draw_handle(renderer, bounds.x + end_x, handle_bottom, edge_color);
        }

        // Preview line for edge drag
        if let Some(DragTarget::EdgeStart | DragTarget::EdgeEnd) = state.drag {
            let dt = state.drag_time;
            if dt > EDGE_SNAP_THRESHOLD * self.duration
                && dt < self.duration * (1.0 - EDGE_SNAP_THRESHOLD)
            {
                let px = time_to_screen_x(dt, self.duration, zoom, scroll, width);
                if px >= 0.0 && px <= width {
                    let preview_color = Color::from_rgba(1.0, 0.85, 0.0, 0.5);
                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle {
                                x: bounds.x + px - 1.0,
                                y: bounds.y,
                                width: 2.0,
                                height: color_h,
                            },
                            ..Quad::default()
                        },
                        Background::Color(preview_color),
                    );
                    draw_handle(
                        renderer,
                        bounds.x + px,
                        handle_bottom,
                        Color::from_rgba(1.0, 0.85, 0.0, 0.7),
                    );
                }
            }
        }

        // Playhead
        if self.duration > 0.0 {
            let ph_x = time_to_screen_x(self.position, self.duration, zoom, scroll, width);
            if ph_x >= 0.0 && ph_x <= width {
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
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x: bounds.x + ph_x - PLAYHEAD_TRI_SIZE / 2.0,
                            y: bounds.y,
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

        // Time labels
        renderer.fill_quad(
            Quad {
                bounds: label_bounds,
                ..Quad::default()
            },
            Background::Color(Color::from_rgba(0.1, 0.1, 0.1, 1.0)),
        );

        let visible_duration = self.duration / zoom as f64;
        let (tick_interval, decimals) = pick_tick_interval(visible_duration);
        let visible_start = screen_x_to_time(0.0, self.duration, zoom, scroll, width);
        let visible_end = screen_x_to_time(width, self.duration, zoom, scroll, width);
        let first_tick = (visible_start / tick_interval).floor() as i64;
        let last_tick = (visible_end / tick_interval).ceil() as i64;
        let tick_color = Color::from_rgba(0.5, 0.5, 0.5, 1.0);
        let label_color = Color::from_rgba(0.7, 0.7, 0.7, 1.0);
        let label_y = bounds.y + color_h;

        for i in first_tick..=last_tick {
            let time = i as f64 * tick_interval;
            if time < 0.0 || time > self.duration {
                continue;
            }
            let tx = time_to_screen_x(time, self.duration, zoom, scroll, width);
            if tx < 0.0 || tx > width {
                continue;
            }

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

            let label = format_time(time, decimals);
            renderer.fill_text(
                Text {
                    content: label,
                    bounds: Size::new(60.0, LABEL_HEIGHT - TICK_HEIGHT),
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
                label_bounds,
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
        match state.drag {
            Some(DragTarget::Cut(_) | DragTarget::EdgeStart | DragTarget::EdgeEnd) => {
                mouse::Interaction::ResizingHorizontally
            }
            Some(DragTarget::Playhead) => mouse::Interaction::Grabbing,
            None => {
                if let Some(pos) = cursor.position_over(layout.bounds()) {
                    let local_x = pos.x - layout.bounds().x;
                    if hit_test_boundary(
                        local_x,
                        self.duration,
                        self.cuts,
                        state.zoom,
                        state.scroll_offset,
                        layout.bounds().width,
                    )
                    .is_some()
                    {
                        mouse::Interaction::ResizingHorizontally
                    } else {
                        mouse::Interaction::Pointer
                    }
                } else {
                    mouse::Interaction::default()
                }
            }
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
