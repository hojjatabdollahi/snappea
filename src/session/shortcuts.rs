use crate::config::ToolbarPosition;
use crate::domain::Choice;
use crate::screenshot::Args;
use crate::session::messages::Msg;
use cosmic::iced::keyboard::{key::Named, Key, Modifiers};

pub fn handle_key_event(
    args: &Args,
    key: Key,
    modifiers: Modifiers,
    current_output_index: usize,
) -> Option<Msg> {
    // Determine if we have a complete selection for action shortcuts
    let has_selection = match &args.session.choice {
        Choice::Rectangle(r, _) => r.dimensions().is_some(),
        Choice::Output(Some(_)) => true, // Only confirmed screen counts as selection
        _ => false,
    };

    let arrow_mode = args.annotations.arrow_mode;
    let redact_mode = args.annotations.redact_mode;

    // Check if we're in a mode that supports navigation
    let in_screen_picker = matches!(&args.session.choice, Choice::Output(None)); // Picker mode only

    // Check if OCR/QR have results (pressing O/Q again should copy and close)
    let has_ocr_result = args.detection.ocr_text.is_some();
    let has_qr_result = !args.detection.qr_codes.is_empty();

    match key {
        // Ctrl+hjkl or Ctrl+arrows: move toolbar position
        Key::Character(c) if c.as_str() == "h" && modifiers.control() => {
            Some(Msg::toolbar_position(ToolbarPosition::Left))
        }
        Key::Character(c) if c.as_str() == "j" && modifiers.control() => {
            Some(Msg::toolbar_position(ToolbarPosition::Bottom))
        }
        Key::Character(c) if c.as_str() == "k" && modifiers.control() => {
            Some(Msg::toolbar_position(ToolbarPosition::Top))
        }
        Key::Character(c) if c.as_str() == "l" && modifiers.control() => {
            Some(Msg::toolbar_position(ToolbarPosition::Right))
        }
        Key::Named(Named::ArrowLeft) if modifiers.control() => {
            Some(Msg::toolbar_position(ToolbarPosition::Left))
        }
        Key::Named(Named::ArrowDown) if modifiers.control() => {
            Some(Msg::toolbar_position(ToolbarPosition::Bottom))
        }
        Key::Named(Named::ArrowUp) if modifiers.control() => {
            Some(Msg::toolbar_position(ToolbarPosition::Top))
        }
        Key::Named(Named::ArrowRight) if modifiers.control() => {
            Some(Msg::toolbar_position(ToolbarPosition::Right))
        }
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
        // Save/copy shortcuts (always available - empty selection captures all screens)
        Key::Named(Named::Enter) if modifiers.control() => Some(Msg::save_to_pictures()),
        Key::Named(Named::Escape) => Some(Msg::cancel()),
        // Space/Enter to confirm selection in picker mode (screen)
        Key::Named(Named::Space) if in_screen_picker => Some(Msg::confirm()),
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
        // OCR shortcut: if result exists, copy and close; otherwise start OCR
        Key::Character(c) if c.as_str() == "o" && has_ocr_result => Some(Msg::ocr_copy_and_close()),
        Key::Character(c) if c.as_str() == "o" && has_selection => Some(Msg::ocr_requested()),
        // QR shortcut: if result exists, copy and close; otherwise start scan
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
