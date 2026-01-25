//! Handlers for selection-related messages
//!
//! Handles: Choice, OutputChanged, WindowChosen, ConfirmSelection, Navigate*,
//! SelectRegionMode, SelectWindowMode, SelectScreenMode

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

/// Handle SelectWindowMode message
pub fn handle_select_window_mode(app: &mut App, output_index: usize) -> HandlerResult {
    if let Some(args) = app.screenshot_args.as_mut() {
        // Get the output name from the index
        if let Some(output) = app.outputs.get(output_index) {
            args.session.choice = Choice::Window(output.name.clone(), None);
            args.session.focused_output_index = output_index;
            args.session.highlighted_window_index = 0;
            // Mark that we have a valid focused output (from the button click location)
            args.session.has_mouse_entered = true;
            args.clear_transient_state();
        }
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
                Choice::Window(_, None) => {
                    // In window picker mode: navigate through windows across screens
                    if args.session.highlighted_window_index > 0 {
                        // Move to previous window on same screen
                        args.session.highlighted_window_index -= 1;
                    } else {
                        // Move to previous screen and select its last window
                        let start_index = args.session.focused_output_index;
                        loop {
                            args.session.focused_output_index =
                                if args.session.focused_output_index == 0 {
                                    output_count - 1
                                } else {
                                    args.session.focused_output_index - 1
                                };

                            let window_count = app
                                .outputs
                                .get(args.session.focused_output_index)
                                .and_then(|o| args.capture.toplevel_images.get(&o.name))
                                .map(|v| v.len())
                                .unwrap_or(0);

                            if window_count > 0 {
                                // Found a screen with windows, select the last one
                                args.session.highlighted_window_index = window_count - 1;
                                if let Some(output) =
                                    app.outputs.get(args.session.focused_output_index)
                                {
                                    args.session.choice = Choice::Window(output.name.clone(), None);
                                }
                                break;
                            }

                            // If we've checked all screens and found none with windows,
                            // stay on current screen
                            if args.session.focused_output_index == start_index {
                                args.session.highlighted_window_index = 0;
                                break;
                            }
                        }
                    }
                }
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
                Choice::Window(_, None) => {
                    // In window picker mode: navigate through windows across screens
                    let current_window_count = app
                        .outputs
                        .get(args.session.focused_output_index)
                        .and_then(|o| args.capture.toplevel_images.get(&o.name))
                        .map(|v| v.len())
                        .unwrap_or(0);

                    if current_window_count > 0
                        && args.session.highlighted_window_index < current_window_count - 1
                    {
                        // Move to next window on same screen
                        args.session.highlighted_window_index += 1;
                    } else {
                        // Move to next screen and select its first window
                        let start_index = args.session.focused_output_index;
                        loop {
                            args.session.focused_output_index =
                                (args.session.focused_output_index + 1) % output_count;

                            let window_count = app
                                .outputs
                                .get(args.session.focused_output_index)
                                .and_then(|o| args.capture.toplevel_images.get(&o.name))
                                .map(|v| v.len())
                                .unwrap_or(0);

                            if window_count > 0 {
                                // Found a screen with windows, select the first one
                                args.session.highlighted_window_index = 0;
                                if let Some(output) =
                                    app.outputs.get(args.session.focused_output_index)
                                {
                                    args.session.choice = Choice::Window(output.name.clone(), None);
                                }
                                break;
                            }

                            // If we've checked all screens and found none with windows,
                            // stay on current screen
                            if args.session.focused_output_index == start_index {
                                args.session.highlighted_window_index = 0;
                                break;
                            }
                        }
                    }
                }
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
            Choice::Window(_, None) => {
                // Confirm the highlighted window on the focused output
                if let Some(output) = app.outputs.get(args.session.focused_output_index) {
                    let window_count = args
                        .capture
                        .toplevel_images
                        .get(&output.name)
                        .map(|v| v.len())
                        .unwrap_or(0);
                    if window_count > 0 && args.session.highlighted_window_index < window_count {
                        args.session.choice = Choice::Window(
                            output.name.clone(),
                            Some(args.session.highlighted_window_index),
                        );
                    }
                }
            }
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
