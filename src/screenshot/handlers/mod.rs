//! Message handlers for screenshot functionality
//!
//! Each submodule handles a category of messages.
//! The main update_msg() function dispatches to these handlers.
//!
//! Handler locations:
//! - DrawMsg handlers: crate::annotations::handlers
//! - ToolMsg handlers: crate::widget::tool_handlers  
//! - SettingsMsg handlers: crate::widget::settings_handlers
//! - SelectMsg handlers: selection.rs (needs App.outputs access)
//! - CaptureMsg handlers: inline in screenshot/mod.rs (needs portal access)

pub mod selection;

use crate::core::app::Msg as AppMsg;
use cosmic::Task;

/// Result type for message handlers
/// This matches the return type of update_msg()
pub type HandlerResult = Task<AppMsg>;

// Re-export selection handler functions
pub use selection::{
    handle_confirm_selection, handle_navigate_left, handle_navigate_right,
    handle_select_region_mode, handle_select_screen_mode, handle_select_window_mode,
};

// Note: Settings handlers moved to widget::settings_handlers
// Note: Tool handlers moved to widget::tool_handlers

/// Helper to get mutable args or return Task::none()
#[macro_export]
macro_rules! with_args {
    ($app:expr, $body:expr) => {
        if let Some(args) = $app.screenshot_args.as_mut() {
            $body(args)
        } else {
            log::error!("Failed to find screenshot Args");
            cosmic::Task::none()
        }
    };
}

/// Helper to get immutable args or return Task::none()
#[macro_export]
macro_rules! with_args_ref {
    ($app:expr, $body:expr) => {
        if let Some(args) = $app.screenshot_args.as_ref() {
            $body(args)
        } else {
            log::error!("Failed to find screenshot Args");
            cosmic::Task::none()
        }
    };
}
