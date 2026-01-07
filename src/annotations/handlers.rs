//! Annotation message handlers
//!
//! Handles DrawMsg for all annotation drawing operations.

use crate::domain::{
    Annotation, ArrowAnnotation, CircleOutlineAnnotation, PixelateAnnotation,
    RectOutlineAnnotation, RedactAnnotation,
};
use crate::screenshot::Args;
use crate::session::messages::{DrawAction, DrawMsg};

/// Handle a DrawMsg, modifying Args state
///
/// Returns true if the message was handled, false otherwise.
/// The caller is responsible for returning Task::none().
pub fn handle_draw_msg(args: &mut Args, msg: DrawMsg) {
    match msg {
        DrawMsg::Arrow(action) => handle_arrow(args, action),
        DrawMsg::Circle(action) => handle_circle(args, action),
        DrawMsg::Rectangle(action) => handle_rectangle(args, action),
        DrawMsg::Redact(action) => handle_redact(args, action),
        DrawMsg::Pixelate(action) => handle_pixelate(args, action),
        DrawMsg::ClearShapes => args.annotations.clear_shapes(),
        DrawMsg::ClearRedactions => args.annotations.clear_redactions(),
        DrawMsg::Undo => args.annotations.undo(),
        DrawMsg::Redo => args.annotations.redo(),
    }
}

// ============================================================================
// Arrow handlers
// ============================================================================

fn handle_arrow(args: &mut Args, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            args.annotations.arrow_mode = !args.annotations.arrow_mode;
            if !args.annotations.arrow_mode {
                args.annotations.arrow_drawing = None;
            } else {
                disable_other_modes(args, Mode::Arrow);
                args.detection.clear();
            }
        }
        DrawAction::Start(x, y) => {
            if args.annotations.arrow_mode {
                args.annotations.arrow_drawing = Some((x, y));
            }
        }
        DrawAction::End(x, y) => {
            if let Some((start_x, start_y)) = args.annotations.arrow_drawing.take() {
                let arrow = ArrowAnnotation {
                    start_x,
                    start_y,
                    end_x: x,
                    end_y: y,
                    color: args.ui.shape_color,
                    shadow: args.ui.shape_shadow,
                };
                args.annotations.arrows.push(arrow.clone());
                args.annotations.add(Annotation::Arrow(arrow));
            }
        }
    }
}

// ============================================================================
// Circle handlers
// ============================================================================

fn handle_circle(args: &mut Args, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            args.annotations.circle_mode = !args.annotations.circle_mode;
            if !args.annotations.circle_mode {
                args.annotations.circle_drawing = None;
            } else {
                disable_other_modes(args, Mode::Circle);
                args.detection.clear();
            }
        }
        DrawAction::Start(x, y) => {
            if args.annotations.circle_mode {
                args.annotations.circle_drawing = Some((x, y));
            }
        }
        DrawAction::End(x, y) => {
            if let Some((start_x, start_y)) = args.annotations.circle_drawing.take() {
                let circle = CircleOutlineAnnotation {
                    start_x,
                    start_y,
                    end_x: x,
                    end_y: y,
                    color: args.ui.shape_color,
                    shadow: args.ui.shape_shadow,
                };
                args.annotations.circles.push(circle.clone());
                args.annotations.add(Annotation::Circle(circle));
            }
        }
    }
}

// ============================================================================
// Rectangle handlers
// ============================================================================

fn handle_rectangle(args: &mut Args, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            args.annotations.rect_outline_mode = !args.annotations.rect_outline_mode;
            if !args.annotations.rect_outline_mode {
                args.annotations.rect_outline_drawing = None;
            } else {
                disable_other_modes(args, Mode::Rectangle);
                args.detection.clear();
            }
        }
        DrawAction::Start(x, y) => {
            if args.annotations.rect_outline_mode {
                args.annotations.rect_outline_drawing = Some((x, y));
            }
        }
        DrawAction::End(x, y) => {
            if let Some((start_x, start_y)) = args.annotations.rect_outline_drawing.take() {
                let rect = RectOutlineAnnotation {
                    start_x,
                    start_y,
                    end_x: x,
                    end_y: y,
                    color: args.ui.shape_color,
                    shadow: args.ui.shape_shadow,
                };
                args.annotations.rect_outlines.push(rect.clone());
                args.annotations.add(Annotation::Rectangle(rect));
            }
        }
    }
}

// ============================================================================
// Redact handlers
// ============================================================================

fn handle_redact(args: &mut Args, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            args.annotations.redact_mode = !args.annotations.redact_mode;
            if !args.annotations.redact_mode {
                args.annotations.redact_drawing = None;
            } else {
                disable_other_modes(args, Mode::Redact);
                args.detection.clear();
            }
        }
        DrawAction::Start(x, y) => {
            if args.annotations.redact_mode {
                args.annotations.redact_drawing = Some((x, y));
            }
        }
        DrawAction::End(x, y) => {
            if let Some((start_x, start_y)) = args.annotations.redact_drawing.take() {
                let redact = RedactAnnotation {
                    x: start_x,
                    y: start_y,
                    x2: x,
                    y2: y,
                };
                args.annotations.redactions.push(redact.clone());
                args.annotations.add(Annotation::Redact(redact));
            }
        }
    }
}

// ============================================================================
// Pixelate handlers
// ============================================================================

fn handle_pixelate(args: &mut Args, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            args.annotations.pixelate_mode = !args.annotations.pixelate_mode;
            if !args.annotations.pixelate_mode {
                args.annotations.pixelate_drawing = None;
            } else {
                disable_other_modes(args, Mode::Pixelate);
                args.detection.clear();
            }
        }
        DrawAction::Start(x, y) => {
            if args.annotations.pixelate_mode {
                args.annotations.pixelate_drawing = Some((x, y));
            }
        }
        DrawAction::End(x, y) => {
            if let Some((start_x, start_y)) = args.annotations.pixelate_drawing.take() {
                let pixelate = PixelateAnnotation {
                    x: start_x,
                    y: start_y,
                    x2: x,
                    y2: y,
                    block_size: args.ui.pixelation_block_size,
                };
                args.annotations.pixelations.push(pixelate.clone());
                args.annotations.add(Annotation::Pixelate(pixelate));
            }
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Arrow,
    Circle,
    Rectangle,
    Redact,
    Pixelate,
}

fn disable_other_modes(args: &mut Args, keep: Mode) {
    if keep != Mode::Arrow {
        args.annotations.arrow_mode = false;
        args.annotations.arrow_drawing = None;
    }
    if keep != Mode::Circle {
        args.annotations.circle_mode = false;
        args.annotations.circle_drawing = None;
    }
    if keep != Mode::Rectangle {
        args.annotations.rect_outline_mode = false;
        args.annotations.rect_outline_drawing = None;
    }
    if keep != Mode::Redact {
        args.annotations.redact_mode = false;
        args.annotations.redact_drawing = None;
    }
    if keep != Mode::Pixelate {
        args.annotations.pixelate_mode = false;
        args.annotations.pixelate_drawing = None;
    }
}
