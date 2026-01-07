//! Tool message handlers for shape and redact tool configuration
//!
//! Handles ToolMsg for popup state, tool selection, colors, and config persistence.

use crate::config::{BlazingshotConfig, RedactTool, ShapeTool};
use crate::screenshot::Args;
use crate::session::messages::{ToolMsg, ToolPopupAction};

/// Handle a ToolMsg, modifying Args state
///
/// Returns true if config was modified and needs saving.
pub fn handle_tool_msg(args: &mut Args, msg: ToolMsg) -> bool {
    match msg {
        ToolMsg::ShapeModeToggle => {
            handle_shape_mode_toggle(args);
            false
        }
        ToolMsg::SetShapeTool(tool) => {
            set_primary_shape_tool(args, tool);
            true // needs config save
        }
        ToolMsg::CycleShapeTool => {
            cycle_shape_tool(args);
            true // needs config save
        }
        ToolMsg::ShapePopup(action) => {
            handle_shape_popup(args, action);
            false
        }
        ToolMsg::SetShapeColor(color) => {
            args.ui.shape_color = color;
            true // needs config save
        }
        ToolMsg::ToggleShapeShadow => {
            args.ui.shape_shadow = !args.ui.shape_shadow;
            true // needs config save
        }
        ToolMsg::SetRedactTool(tool) => {
            set_primary_redact_tool(args, tool);
            true // needs config save
        }
        ToolMsg::RedactModeToggle => {
            handle_redact_mode_toggle(args);
            false
        }
        ToolMsg::CycleRedactTool => {
            cycle_redact_tool(args);
            true // needs config save
        }
        ToolMsg::RedactPopup(action) => {
            handle_redact_popup(args, action);
            false
        }
        ToolMsg::SetPixelationBlockSize(size) => {
            args.ui.pixelation_block_size = size;
            false // saved on release, not during drag
        }
        ToolMsg::SavePixelationBlockSize => {
            true // needs config save
        }
    }
}

/// Save current tool config to persistent storage
pub fn save_tool_config(args: &Args) {
    let mut config = BlazingshotConfig::load();
    config.primary_shape_tool = args.ui.primary_shape_tool;
    config.shape_color = args.ui.shape_color;
    config.shape_shadow = args.ui.shape_shadow;
    config.primary_redact_tool = args.ui.primary_redact_tool;
    config.pixelation_block_size = args.ui.pixelation_block_size;
    config.save();
}

// ============================================================================
// Shape tool handlers
// ============================================================================

fn handle_shape_mode_toggle(args: &mut Args) {
    match args.ui.primary_shape_tool {
        ShapeTool::Arrow => {
            args.annotations.arrow_mode = !args.annotations.arrow_mode;
            if args.annotations.arrow_mode {
                disable_other_modes_except(args, Mode::Arrow);
            } else {
                args.annotations.arrow_drawing = None;
            }
        }
        ShapeTool::Circle => {
            args.annotations.circle_mode = !args.annotations.circle_mode;
            if args.annotations.circle_mode {
                disable_other_modes_except(args, Mode::Circle);
            } else {
                args.annotations.circle_drawing = None;
            }
        }
        ShapeTool::Rectangle => {
            args.annotations.rect_outline_mode = !args.annotations.rect_outline_mode;
            if args.annotations.rect_outline_mode {
                disable_other_modes_except(args, Mode::Rectangle);
            } else {
                args.annotations.rect_outline_drawing = None;
            }
        }
    }
    // Close popups
    args.close_all_popups();
}

fn set_primary_shape_tool(args: &mut Args, tool: ShapeTool) {
    args.ui.primary_shape_tool = tool;

    // Activate the new tool
    match tool {
        ShapeTool::Arrow => {
            args.annotations.arrow_mode = true;
            disable_other_modes_except(args, Mode::Arrow);
        }
        ShapeTool::Circle => {
            args.annotations.circle_mode = true;
            disable_other_modes_except(args, Mode::Circle);
        }
        ShapeTool::Rectangle => {
            args.annotations.rect_outline_mode = true;
            disable_other_modes_except(args, Mode::Rectangle);
        }
    }
    args.close_all_popups();
}

fn cycle_shape_tool(args: &mut Args) {
    args.ui.primary_shape_tool = args.ui.primary_shape_tool.next();
    set_primary_shape_tool(args, args.ui.primary_shape_tool);
}

fn handle_shape_popup(args: &mut Args, action: ToolPopupAction) {
    match action {
        ToolPopupAction::Toggle => {
            args.ui.shape_popup_open = !args.ui.shape_popup_open;
            if args.ui.shape_popup_open {
                args.ui.redact_popup_open = false;
                args.ui.settings_drawer_open = false;
                args.disable_all_modes();
            }
        }
        ToolPopupAction::Open => {
            args.ui.shape_popup_open = true;
            args.ui.redact_popup_open = false;
            args.ui.settings_drawer_open = false;
            args.disable_all_modes();
        }
        ToolPopupAction::Close => {
            args.ui.shape_popup_open = false;
        }
    }
}

// ============================================================================
// Redact tool handlers
// ============================================================================

fn set_primary_redact_tool(args: &mut Args, tool: RedactTool) {
    args.ui.primary_redact_tool = tool;

    match tool {
        RedactTool::Redact => {
            args.annotations.redact_mode = true;
            disable_other_modes_except(args, Mode::Redact);
        }
        RedactTool::Pixelate => {
            args.annotations.pixelate_mode = true;
            disable_other_modes_except(args, Mode::Pixelate);
        }
    }
    args.close_all_popups();
}

fn cycle_redact_tool(args: &mut Args) {
    args.ui.primary_redact_tool = args.ui.primary_redact_tool.next();
    set_primary_redact_tool(args, args.ui.primary_redact_tool);
}

fn handle_redact_popup(args: &mut Args, action: ToolPopupAction) {
    match action {
        ToolPopupAction::Toggle => {
            args.ui.redact_popup_open = !args.ui.redact_popup_open;
            if args.ui.redact_popup_open {
                args.ui.shape_popup_open = false;
                args.ui.settings_drawer_open = false;
                args.disable_all_modes();
            }
        }
        ToolPopupAction::Open => {
            args.ui.redact_popup_open = true;
            args.ui.shape_popup_open = false;
            args.ui.settings_drawer_open = false;
            args.disable_all_modes();
        }
        ToolPopupAction::Close => {
            args.ui.redact_popup_open = false;
        }
    }
}

fn handle_redact_mode_toggle(args: &mut Args) {
    match args.ui.primary_redact_tool {
        RedactTool::Redact => {
            args.annotations.redact_mode = !args.annotations.redact_mode;
            if args.annotations.redact_mode {
                disable_other_modes_except(args, Mode::Redact);
            } else {
                args.annotations.redact_drawing = None;
            }
        }
        RedactTool::Pixelate => {
            args.annotations.pixelate_mode = !args.annotations.pixelate_mode;
            if args.annotations.pixelate_mode {
                disable_other_modes_except(args, Mode::Pixelate);
            } else {
                args.annotations.pixelate_drawing = None;
            }
        }
    }
    // Close popups
    args.close_all_popups();
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

fn disable_other_modes_except(args: &mut Args, keep: Mode) {
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
    // Clear OCR/QR when switching modes
    args.detection.clear();
}
