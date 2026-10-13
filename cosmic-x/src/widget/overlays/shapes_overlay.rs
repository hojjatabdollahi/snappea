// SPDX-License-Identifier: GPL-3.0-only

use cosmic::iced::Vector;
use cosmic::iced::core::Color;
use cosmic::iced::widget::canvas;
use viewer_tools::annotate::{
    HIGHLIGHT_ALPHA, HighlighterOperation, MagnifierOperation, PenOperation, PixelateOperation,
    RedactOperation, ShapeKind, ShapeOperation,
};
use viewer_tools::{SampleSource, ToolOperation};

use crate::geometry::Rect;

/// Width of the highlighter's nib indicator. Kept narrow because its height
/// is what shows the band the highlight will cover.
const HIGHLIGHT_NIB: f32 = 4.0;

/// The thinnest ring drawn for the pen cursor, in logical pixels.
const MIN_RING_STROKE: f32 = 1.0;

const _: () = assert!(HIGHLIGHT_NIB < 8.0);

/// A dark or light edge, whichever the ink is not, so the brush indicator is
/// visible over both a white page and a dark terminal.
fn outline(ink: Color) -> Color {
    let luma = 0.114f32.mul_add(ink.b, 0.299f32.mul_add(ink.r, 0.587 * ink.g));
    if luma > 0.5 {
        Color::from_rgba(0.0, 0.0, 0.0, 0.7)
    } else {
        Color::from_rgba(1.0, 1.0, 1.0, 0.7)
    }
}

/// Clearance between a picked annotation and its selection box.
const SELECTION_PAD: f32 = 4.0;

/// Width of that box's two strokes.
const SELECTION_BORDER: f32 = 1.5;

/// Told which shape, and where the gesture reached, in global coordinates.
pub type ShapeCallback<'a, Message> = Box<dyn Fn(ShapeKind, f32, f32) -> Message + 'a>;

pub struct ShapesOverlay<'a, Message: Clone + 'static> {
    pub selection_rect: Option<(f32, f32, f32, f32)>,
    pub output_rect: Rect,
    pub operations: &'a [Box<dyn ToolOperation>],
    /// This output's pixels, for the tools that sample what is under them
    pub source: SampleSource<'a>,
    /// The operation the cursor tool has picked out, indexed into
    /// [`Self::operations`].
    pub selected: Option<usize>,
    pub redact_drawing: Option<(f32, f32)>,
    pub pixelate_drawing: Option<(f32, f32)>,
    pub magnifier_drawing: Option<(f32, f32)>,
    pub pixelation_block_size: f32,
    pub magnifier_magnification: f32,
    pub shape_mode: Option<ShapeKind>,
    pub text_mode: bool,
    pub pen_mode: bool,
    pub highlighter_mode: bool,
    pub highlighter_thickness: f32,
    pub shape_drawing: Option<(f32, f32)>,
    pub text_edit: Option<(&'a viewer_tools::annotate::TextPreview, (f32, f32))>,
    pub stroke_drawing: Option<Vec<(f32, f32)>>,
    pub on_shape_start: Option<ShapeCallback<'a, Message>>,
    pub on_shape_end: Option<ShapeCallback<'a, Message>>,
    pub on_text_press: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    pub on_text_drag: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    pub on_text_release: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    pub on_pencil_start: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    pub on_pencil_move: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    pub on_pencil_end: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    pub shape_color: viewer_tools::annotate::AnnotateColor,
    pub shape_thickness: f32,
}

/// State for `ShapesOverlay` canvas program
#[derive(Debug, Default)]
pub struct ShapesState {
    /// Whether Ctrl key is currently pressed
    pub ctrl_down: bool,
    /// Whether Ctrl was pressed when drawing started (latched)
    pub ctrl_latched: bool,
    /// Whether the pointer went down on a text label and has not come up. Tracked
    /// here because the view is rebuilt between press and release.
    pub text_pressed: bool,
}

impl ShapesState {
    /// Latch Ctrl state if a drawing is active
    pub const fn latch_ctrl_if_needed(&mut self, drawing_active: bool) {
        if drawing_active && self.ctrl_down {
            self.ctrl_latched = true;
        }
    }
}

/// Whether `op` draws sampled pixels as an image. iced renders a layer's images
/// after its meshes whatever the call order, so the operations after one of
/// these have to go in a layer of their own to land on top of it.
pub fn samples_pixels(op: &dyn ToolOperation) -> bool {
    op.as_any().is::<MagnifierOperation>() || op.as_any().is::<PixelateOperation>()
}

/// Draw `ops[range]` onto `renderer`, each sampling the picture with the
/// operations below it rendered in. Global coordinates, mapped onto `bounds`
/// from `origin`.
pub fn draw_operations(
    renderer: &mut cosmic::Renderer,
    bounds: cosmic::iced::core::Rectangle,
    origin: cosmic::iced::core::Point,
    ops: &[Box<dyn ToolOperation>],
    range: std::ops::Range<usize>,
    source: SampleSource<'_>,
) {
    use cosmic::iced::advanced::graphics::geometry::Renderer as _;
    let mut frame = canvas::Frame::new(renderer, bounds.size());
    frame.push_transform();
    frame.translate(Vector::new(-origin.x, -origin.y));
    for i in range {
        let source = SampleSource {
            below: &ops[..i],
            ..source
        };
        // The picked operation is marked by the overlay's dashed box, not by the tool.
        ops[i].draw_sampled(&mut frame, bounds.size(), 1.0, false, Some(&source));
    }
    frame.pop_transform();
    renderer.draw_geometry(frame.into_geometry());
}

impl<Message: Clone + 'static> ShapesOverlay<'_, Message> {
    /// The picture with every committed operation rendered in, for previews.
    fn source_over_all(&self) -> SampleSource<'_> {
        SampleSource {
            below: self.operations,
            ..self.source
        }
    }

    /// Whether a point is inside the region being captured.
    fn inside_selection(&self, pos: cosmic::iced::core::Point) -> bool {
        self.selection_rect.is_some_and(|(x, y, w, h)| {
            pos.x >= x && pos.x <= x + w && pos.y >= y && pos.y <= y + h
        })
    }

    /// What holding Ctrl means for this shape: areas become square, lines snap to 45°.
    fn constrain(kind: ShapeKind, sx: f32, sy: f32, ex: f32, ey: f32) -> (f32, f32) {
        use ShapeKind as K;
        match kind {
            K::Line | K::Arrow => snap_to_45_degrees(sx, sy, ex, ey),
            _ => Self::constrain_end(sx, sy, ex, ey),
        }
    }

    /// Constrain end point to form a square/circle (when Ctrl is held)
    fn constrain_end(sx: f32, sy: f32, ex: f32, ey: f32) -> (f32, f32) {
        let dx = ex - sx;
        let dy = ey - sy;
        let side = dx.abs().min(dy.abs());
        let sign_x = if dx < 0.0 { -1.0 } else { 1.0 };
        let sign_y = if dy < 0.0 { -1.0 } else { 1.0 };
        (side.mul_add(sign_x, sx), side.mul_add(sign_y, sy))
    }

    /// Whether any drawing gesture handled by this overlay is in progress.
    const fn drawing_active(&self) -> bool {
        self.shape_drawing.is_some() || self.stroke_drawing.is_some()
    }
}

impl<Message: Clone + 'static> canvas::Program<Message, cosmic::Theme, cosmic::Renderer>
    for ShapesOverlay<'_, Message>
{
    type State = ShapesState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &cosmic::iced::Event,
        bounds: cosmic::iced::core::Rectangle,
        cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        use cosmic::iced::core::keyboard;
        use cosmic::iced::core::mouse::{Button, Event as MouseEvent};

        // Gestures are clamped into the selection.
        let clamp_pos = |px: f32, py: f32| -> (f32, f32) {
            match self.selection_rect {
                Some((x, y, w, h)) => (px.clamp(x, x + w), py.clamp(y, y + h)),
                None => (px, py),
            }
        };

        match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) => {
                state.ctrl_down = mods.control();
                state.latch_ctrl_if_needed(self.drawing_active());
                return Some(canvas::Action::capture());
            }
            // Freehand needs every intermediate position to build its polyline.
            canvas::Event::Mouse(MouseEvent::CursorMoved { .. }) => {
                // Only while the button is down: this arm fires on plain hover
                // too, and a stray on_drag in the editing state is not harmless.
                if state.text_pressed
                    && let Some(ref cb) = self.on_text_drag
                    && let Some(pos) = cursor.position_in(bounds)
                {
                    // The preview clamps to the selection itself.
                    let gx = pos.x + self.output_rect.left as f32;
                    let gy = pos.y + self.output_rect.top as f32;
                    return Some(canvas::Action::publish(cb(gx, gy)));
                }
                // Redraw as the pointer moves. The brush cursor follows it.
                if self.pen_mode && self.stroke_drawing.is_none() {
                    return Some(canvas::Action::request_redraw());
                }
                if self.pen_mode && self.stroke_drawing.is_some() {
                    let pos = cursor.position_in(bounds)?;
                    let (cx, cy) = clamp_pos(pos.x, pos.y);
                    let gx = cx + self.output_rect.left as f32;
                    let gy = cy + self.output_rect.top as f32;
                    if let Some(ref cb) = self.on_pencil_move {
                        return Some(canvas::Action::publish(cb(gx, gy)).and_capture());
                    }
                }
            }
            canvas::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                if !self.inside_selection(pos) {
                    return None;
                }

                // Clamp and convert to global coordinates
                let (cx, cy) = clamp_pos(pos.x, pos.y);
                let gx = cx + self.output_rect.left as f32;
                let gy = cy + self.output_rect.top as f32;

                if (self.text_mode || self.text_edit.is_some())
                    && let Some(ref cb) = self.on_text_press
                {
                    state.text_pressed = true;
                    return Some(canvas::Action::publish(cb(gx, gy)).and_capture());
                }
                if let Some(kind) = self.shape_mode {
                    state.ctrl_latched = state.ctrl_down;
                    if let Some(ref cb) = self.on_shape_start {
                        return Some(canvas::Action::publish(cb(kind, gx, gy)).and_capture());
                    }
                }
                if self.pen_mode
                    && let Some(ref cb) = self.on_pencil_start
                {
                    return Some(canvas::Action::publish(cb(gx, gy)).and_capture());
                }
            }
            canvas::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                // Clamp and convert to global coordinates
                let (cx, cy) = clamp_pos(pos.x, pos.y);
                let gx = cx + self.output_rect.left as f32;
                let gy = cy + self.output_rect.top as f32;

                if state.text_pressed
                    && let Some(ref cb) = self.on_text_release
                {
                    state.text_pressed = false;
                    return Some(canvas::Action::publish(cb(gx, gy)).and_capture());
                }
                if let Some(kind) = self.shape_mode
                    && let Some((sx, sy)) = self.shape_drawing
                {
                    let (ex, ey) = if state.ctrl_latched || state.ctrl_down {
                        Self::constrain(kind, sx, sy, gx, gy)
                    } else {
                        (gx, gy)
                    };
                    state.ctrl_latched = false;
                    if let Some(ref cb) = self.on_shape_end {
                        return Some(canvas::Action::publish(cb(kind, ex, ey)).and_capture());
                    }
                }

                if self.pen_mode && self.stroke_drawing.is_some() {
                    state.ctrl_latched = false;
                    if let Some(ref cb) = self.on_pencil_end {
                        return Some(canvas::Action::publish(cb(gx, gy)).and_capture());
                    }
                }
            }
            _ => {}
        }

        None
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: cosmic::iced::core::Rectangle,
        cursor: cosmic::iced::core::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        use canvas::{Fill, Frame, Path, Stroke};
        use cosmic::iced::core::{Point, Size};

        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let origin = Point::new(self.output_rect.left as f32, self.output_rect.top as f32);
        let cursor_global = cursor
            .position_in(bounds)
            .map(|p| Point::new(p.x + origin.x, p.y + origin.y));
        let constrain = state.ctrl_down || state.ctrl_latched;
        let ink: Color = self.shape_color.into();

        // Everything below is in global logical coordinates. The frame maps
        // them onto this output.
        frame.push_transform();
        frame.translate(Vector::new(-origin.x, -origin.y));

        // Committed operations are drawn by the selection widget, one layer per
        // run, so strokes made after a loupe render above it (`draw_operations`).

        // The picked operation's box: two tones so it reads over any capture.
        if let Some(b) = self
            .selected
            .and_then(|i| self.operations.get(i))
            .and_then(|op| op.bounds())
        {
            const DASH: [f32; 2] = [4.0, 4.0];
            let rect = Path::rectangle(
                Point::new(b.x - SELECTION_PAD, b.y - SELECTION_PAD),
                Size::new(
                    SELECTION_PAD.mul_add(2.0, b.width),
                    SELECTION_PAD.mul_add(2.0, b.height),
                ),
            );
            frame.stroke(
                &rect,
                Stroke::default()
                    .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.75))
                    .with_width(SELECTION_BORDER),
            );
            frame.stroke(
                &rect,
                Stroke {
                    line_dash: canvas::LineDash {
                        segments: &DASH,
                        offset: 0,
                    },
                    ..Stroke::default()
                        .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.95))
                        .with_width(SELECTION_BORDER)
                },
            );
        }

        // Previews use the operations that will commit them, so nothing changes on release.
        if let Some(kind) = self.shape_mode
            && let Some((sx, sy)) = self.shape_drawing
            && let Some(c) = cursor_global
        {
            let (ex, ey) = if constrain {
                Self::constrain(kind, sx, sy, c.x, c.y)
            } else {
                (c.x, c.y)
            };
            ShapeOperation::new(
                kind,
                Point::new(sx, sy),
                Point::new(ex, ey),
                ink,
                self.shape_thickness,
            )
            .draw(&mut frame, size, 1.0, false);
        }
        let outline_box = |frame: &mut Frame, start: (f32, f32), end: Point| {
            let (x0, x1) = (start.0.min(end.x), start.0.max(end.x));
            let (y0, y1) = (start.1.min(end.y), start.1.max(end.y));
            frame.stroke(
                &Path::rectangle(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0)),
                Stroke::default().with_color(Color::WHITE).with_width(1.0),
            );
        };
        if let Some(start) = self.redact_drawing
            && let Some(c) = cursor_global
        {
            RedactOperation::new(Point::new(start.0, start.1), c)
                .draw(&mut frame, size, 1.0, false);
            outline_box(&mut frame, start, c);
        }
        if let Some(start) = self.pixelate_drawing
            && let Some(c) = cursor_global
        {
            PixelateOperation::new(Point::new(start.0, start.1), c, self.pixelation_block_size)
                .draw_sampled(&mut frame, size, 1.0, false, Some(&self.source_over_all()));
            outline_box(&mut frame, start, c);
        }
        if let Some(start) = self.magnifier_drawing
            && let Some(c) = cursor_global
        {
            MagnifierOperation::new(
                Point::new(start.0, start.1),
                c,
                self.magnifier_magnification,
                ink,
            )
            .draw_sampled(&mut frame, size, 1.0, false, Some(&self.source_over_all()));
        }
        if let Some(points) = self.stroke_drawing.as_ref()
            && points.len() >= 2
        {
            let points: Vec<Point> = points.iter().map(|&(x, y)| Point::new(x, y)).collect();
            if self.highlighter_mode {
                HighlighterOperation {
                    points,
                    color: ink,
                    width: self.highlighter_thickness,
                }
                .draw(&mut frame, size, 1.0, false);
            } else {
                PenOperation {
                    points,
                    color: ink,
                    width: self.shape_thickness,
                }
                .draw(&mut frame, size, 1.0, false);
            }
        }
        // The label being typed, in the selection's coordinates.
        if let Some((preview, (ox, oy))) = self.text_edit {
            frame.push_transform();
            frame.translate(Vector::new(ox, oy));
            preview.draw(&mut frame, size, 1.0, false);
            frame.pop_transform();
        }

        frame.pop_transform();

        // The brush, drawn under the pointer at the size it will actually mark.
        //
        // Drawn here because the layer-shell backend has no custom cursors, and a
        // named shape cannot show the brush size. Drawn while dragging too.
        if let Some(pos) = cursor.position_in(bounds)
            && self.pen_mode
            && self.inside_selection(pos)
        {
            if self.highlighter_mode {
                // A nib the height and transparency of the highlight.
                let h = self.highlighter_thickness;
                frame.fill_rectangle(
                    Point::new(pos.x - HIGHLIGHT_NIB / 2.0, pos.y - h / 2.0),
                    Size::new(HIGHLIGHT_NIB, h),
                    Fill::from(Color {
                        a: ink.a * HIGHLIGHT_ALPHA,
                        ..ink
                    }),
                );
                frame.stroke(
                    &Path::rectangle(
                        Point::new(pos.x - HIGHLIGHT_NIB / 2.0, pos.y - h / 2.0),
                        Size::new(HIGHLIGHT_NIB, h),
                    ),
                    Stroke::default().with_color(outline(ink)).with_width(1.0),
                );
            } else {
                // A ring at the pen's true width. The outline keeps a thin one visible.
                let r = self.shape_thickness / 2.0;
                frame.stroke(
                    &Path::circle(pos, r),
                    Stroke::default()
                        .with_color(ink)
                        .with_width(MIN_RING_STROKE),
                );
                frame.stroke(
                    &Path::circle(pos, r + MIN_RING_STROKE),
                    Stroke::default()
                        .with_color(outline(ink))
                        .with_width(MIN_RING_STROKE),
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

/// Snap a line's end to the nearest 45° around its start, keeping its length.
fn snap_to_45_degrees(sx: f32, sy: f32, ex: f32, ey: f32) -> (f32, f32) {
    let (dx, dy) = (ex - sx, ey - sy);
    let len = dx.hypot(dy);
    if len < f32::EPSILON {
        return (ex, ey);
    }
    const STEP: f32 = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / STEP).round() * STEP;
    (len.mul_add(angle.cos(), sx), len.mul_add(angle.sin(), sy))
}
