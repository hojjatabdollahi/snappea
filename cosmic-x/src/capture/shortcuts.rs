// SPDX-License-Identifier: GPL-3.0-only

use crate::capture::Capture;
use crate::capture::msg::Msg;
use crate::geometry::Choice;
use cosmic::iced::keyboard::{Key, Modifiers, key::Named};

pub fn handle_key_event(
    capture: &Capture,
    key: Key,
    modifiers: Modifiers,
    current_output_index: usize,
) -> Option<Msg> {
    // Determine if we have a complete selection for action shortcuts
    let can_ocr = capture.ui.tesseract_available
        && matches!(&capture.selection.choice, Choice::Rectangle(r, _) if r.dimensions().is_some());
    let has_selection = match &capture.selection.choice {
        Choice::Rectangle(r, _) => r.dimensions().is_some(),
        Choice::Output(Some(_)) => true, // Only confirmed screen counts as selection
        _ => false,
    };

    // With a shape tool armed, letter keys draw instead of switching target.
    let arrow_mode = capture.annotations.shape_mode.is_some();
    let redact_mode = capture.annotations.redact_mode;

    // Check if we're in a mode that supports navigation
    let in_screen_picker = matches!(&capture.selection.choice, Choice::Output(None)); // Picker mode only

    // Check if OCR/QR have results (pressing O/Q again should copy and close)
    let has_ocr_result = capture.detection.ocr_text.is_some();
    let has_qr_result = !capture.detection.qr_codes.is_empty();

    match key {
        // Undo/redo shortcuts
        Key::Character(c) if c.as_str() == "z" && modifiers.control() && !modifiers.shift() => {
            Some(Msg::undo())
        }
        Key::Character(c)
            if (c.as_str() == "y" && modifiers.control())
                || (c.as_str() == "z" && modifiers.control() && modifiers.shift()) =>
        {
            Some(Msg::redo())
        }
        // Delete what is picked out. Ahead of Escape, because a pick is the
        // narrower thing to be undoing.
        Key::Named(Named::Delete | Named::Backspace) if capture.annotations.selected.is_some() => {
            Some(Msg::delete_selected())
        }
        // Escape releases innermost first: pick, then the tool in hand (back to
        // the cursor), then the capture, which asks before discarding work.
        Key::Named(Named::Escape) if capture.annotations.selected.is_some() => {
            Some(Msg::select_annotation(None))
        }
        Key::Named(Named::Escape)
            if capture.annotations.any_mode_active() && !capture.ui.move_mode =>
        {
            Some(Msg::toggle_move_mode())
        }
        // Save/copy shortcuts (always available: empty selection captures all screens)
        Key::Named(Named::Enter) if modifiers.control() => Some(Msg::save_to_pictures()),
        Key::Named(Named::Escape) => Some(Msg::cancel_requested()),
        // Space/Enter to confirm selection in picker mode (screen)
        Key::Character(c) if c.as_str() == " " && in_screen_picker => Some(Msg::confirm()),
        Key::Named(Named::Enter) if in_screen_picker => Some(Msg::confirm()),
        // Enter to copy when not in picker mode
        Key::Named(Named::Enter) => Some(Msg::copy_to_clipboard()),
        // Navigation keys in screen picker: h/l and arrows navigate screens
        Key::Character(c) if c.as_str() == "h" && in_screen_picker => Some(Msg::navigate_left()),
        Key::Character(c) if c.as_str() == "l" && in_screen_picker => Some(Msg::navigate_right()),
        Key::Named(Named::ArrowLeft) if in_screen_picker => Some(Msg::navigate_left()),
        Key::Named(Named::ArrowRight) if in_screen_picker => Some(Msg::navigate_right()),
        // Mode toggle shortcuts (require selection)
        // Shift+A: cycle shape tool (arrow -> circle -> rectangle -> arrow)
        Key::Character(c)
            if c.as_str().eq_ignore_ascii_case("a") && modifiers.shift() && has_selection =>
        {
            Some(Msg::cycle_shape_tool())
        }
        // A: toggle current shape tool
        Key::Character(c) if c.as_str() == "a" && has_selection => Some(Msg::shape_mode_toggle()),
        // Shift+D: cycle to next redact tool (redact/pixelate) and activate it
        Key::Character(c) if c.as_str() == "D" && modifiers.shift() && has_selection => {
            Some(Msg::cycle_redact_tool())
        }
        // D: toggle current redact tool
        Key::Character(c) if c.as_str() == "d" && has_selection => {
            Some(Msg::redact_tool_mode_toggle())
        }
        // OCR: copy the result if there is one, else start. Same gate as the button.
        Key::Character(c) if c.as_str() == "o" && has_ocr_result => Some(Msg::ocr_copy_and_close()),
        Key::Character(c) if c.as_str() == "o" && can_ocr => Some(Msg::ocr_requested()),
        // QR shortcut: if result exists, copy and close. Otherwise start scan
        Key::Character(c) if c.as_str() == "q" && has_qr_result => Some(Msg::qr_copy_and_close()),
        Key::Character(c) if c.as_str() == "q" && has_selection => Some(Msg::qr_requested()),
        // Shift+R: trigger recording (only when region is selected)
        Key::Character(c) if c.as_str() == "R" && modifiers.shift() && has_selection => {
            Some(Msg::record_region())
        }
        // Selection mode shortcuts (always available, but not when in draw mode)
        // Use current_output_index (the screen where this key was pressed)
        Key::Character(c) if c.as_str() == "r" && !arrow_mode && !redact_mode => {
            Some(Msg::region_mode())
        }
        Key::Character(c) if c.as_str() == "s" && !arrow_mode && !redact_mode => {
            Some(Msg::screen_mode(current_output_index))
        }
        _ => None,
    }
}
