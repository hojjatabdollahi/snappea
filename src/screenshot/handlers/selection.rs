//! Handlers for selection-related messages
//!
//! Handles: Choice, OutputChanged, ConfirmSelection, Navigate*,
//! SelectRegionMode, SelectScreenMode

use crate::core::app::App;
use crate::domain::{Choice, DragState, Rect};

use super::HandlerResult;

/// Handle SelectRegionMode message
pub fn handle_select_region_mode(app: &mut App) -> HandlerResult {
    if let Some(args) = app.screenshot_args.as_mut() {
        // Switch to rectangle selection with a fresh/default rect
        args.session.choice = Choice::Rectangle(Rect::default(), DragState::default());
        args.clear_transient_state();
    }
    cosmic::Task::none()
}

/// Handle SelectScreenMode message
pub fn handle_select_screen_mode(app: &mut App, output_index: usize) -> HandlerResult {
    if let Some(args) = app.screenshot_args.as_mut() {
        // Go to picker mode (None), not directly selecting a screen
        args.session.choice = Choice::Output(None);
        args.session.focused_output_index = output_index;
        // Mark that we have a valid focused output (from the button click location)
        args.session.has_mouse_entered = true;
        args.clear_transient_state();
    }
    cosmic::Task::none()
}

/// Handle NavigateLeft message
pub fn handle_navigate_left(app: &mut App) -> HandlerResult {
    if let Some(args) = app.screenshot_args.as_mut() {
        let output_count = app.outputs.len();
        if output_count > 0 {
            match &args.session.choice {
                Choice::Output(None) => {
                    // In screen picker mode: move to previous screen (just update index)
                    args.session.focused_output_index = if args.session.focused_output_index == 0 {
                        output_count - 1
                    } else {
                        args.session.focused_output_index - 1
                    };
                    // Choice stays as None (picker mode)
                }
                _ => {}
            }
        }
    }
    cosmic::Task::none()
}

/// Handle NavigateRight message
pub fn handle_navigate_right(app: &mut App) -> HandlerResult {
    if let Some(args) = app.screenshot_args.as_mut() {
        let output_count = app.outputs.len();
        if output_count > 0 {
            match &args.session.choice {
                Choice::Output(None) => {
                    // In screen picker mode: move to next screen (just update index)
                    args.session.focused_output_index =
                        (args.session.focused_output_index + 1) % output_count;
                    // Choice stays as None (picker mode)
                }
                _ => {}
            }
        }
    }
    cosmic::Task::none()
}

/// Handle ConfirmSelection message
pub fn handle_confirm_selection(app: &mut App) -> HandlerResult {
    if let Some(args) = app.screenshot_args.as_mut() {
        match &args.session.choice {
            Choice::Output(None) => {
                // Confirm the highlighted screen (enter confirmed mode)
                if let Some(output) = app.outputs.get(args.session.focused_output_index) {
                    args.session.choice = Choice::Output(Some(output.name.clone()));
                }
            }
            _ => {}
        }
    }
    cosmic::Task::none()
}
