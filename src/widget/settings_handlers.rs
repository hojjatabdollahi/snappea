//! Handlers for settings-related messages
//!
//! Handles: ToolbarPositionChange, ToggleSettingsDrawer, ToggleMagnifier,
//!          SetSaveLocation, ToggleCopyOnSave, SetVideoEncoder, SetVideoContainer,
//!          SetVideoFramerate

use crate::config::{Container, SaveLocation, SnapPeaConfig, ToolbarPosition};
use crate::screenshot::handlers::HandlerResult;
use crate::screenshot::Args;
use crate::session::state::SettingsTab;

/// Handle ToolbarPositionChange message
pub fn handle_toolbar_position_change(args: &mut Args, position: ToolbarPosition) -> HandlerResult {
    args.ui.toolbar_position = position;
    // Save to config
    let mut config = SnapPeaConfig::load();
    config.toolbar_position = position;
    config.save();
    cosmic::Task::none()
}

/// Handle ToggleSettingsDrawer message
pub fn handle_toggle_settings_drawer(args: &mut Args) -> HandlerResult {
    args.ui.settings_drawer_open = !args.ui.settings_drawer_open;
    // Close other popups when opening settings
    if args.ui.settings_drawer_open {
        args.ui.shape_popup_open = false;
        args.ui.redact_popup_open = false;
        args.disable_all_modes();
    }
    cosmic::Task::none()
}

/// Handle ToggleMagnifier message
pub fn handle_toggle_magnifier(args: &mut Args) -> HandlerResult {
    args.ui.magnifier_enabled = !args.ui.magnifier_enabled;
    // Save to config
    let mut config = SnapPeaConfig::load();
    config.magnifier_enabled = args.ui.magnifier_enabled;
    config.save();
    cosmic::Task::none()
}

/// Handle SetSaveLocationPictures message
pub fn handle_set_save_location_pictures(args: &mut Args) -> HandlerResult {
    args.ui.save_location_setting = SaveLocation::Pictures;
    let mut config = SnapPeaConfig::load();
    config.save_location = args.ui.save_location_setting;
    config.save();
    cosmic::Task::none()
}

/// Handle SetSaveLocationDocuments message
pub fn handle_set_save_location_documents(args: &mut Args) -> HandlerResult {
    args.ui.save_location_setting = SaveLocation::Documents;
    let mut config = SnapPeaConfig::load();
    config.save_location = args.ui.save_location_setting;
    config.save();
    cosmic::Task::none()
}

/// Handle ToggleCopyOnSave message
pub fn handle_toggle_copy_on_save(args: &mut Args) -> HandlerResult {
    args.ui.copy_to_clipboard_on_save = !args.ui.copy_to_clipboard_on_save;
    let mut config = SnapPeaConfig::load();
    config.copy_to_clipboard_on_save = args.ui.copy_to_clipboard_on_save;
    config.save();
    cosmic::Task::none()
}

/// Handle SetSettingsTab message
pub fn handle_set_settings_tab(args: &mut Args, tab: SettingsTab) -> HandlerResult {
    args.ui.settings_tab = tab;
    args.ui.settings_tab_model.activate_tab(tab);
    cosmic::Task::none()
}

/// Handle SetToolbarOpacity message
pub fn handle_set_toolbar_opacity(args: &mut Args, opacity: f32) -> HandlerResult {
    args.ui.toolbar_unhovered_opacity = opacity.clamp(0.1, 1.0);
    let mut config = SnapPeaConfig::load();
    config.toolbar_unhovered_opacity = args.ui.toolbar_unhovered_opacity;
    config.save();
    cosmic::Task::none()
}

/// Handle SetVideoEncoder message
pub fn handle_set_video_encoder(args: &mut Args, encoder: String) -> HandlerResult {
    args.ui.selected_encoder = Some(encoder.clone());
    let mut config = SnapPeaConfig::load();
    config.video_encoder = Some(encoder);
    config.save();
    cosmic::Task::none()
}

/// Handle SetVideoContainer message
pub fn handle_set_video_container(args: &mut Args, container: Container) -> HandlerResult {
    args.ui.video_container = container;
    let mut config = SnapPeaConfig::load();
    config.video_container = container;
    config.save();
    cosmic::Task::none()
}

/// Handle SetVideoFramerate message
pub fn handle_set_video_framerate(args: &mut Args, framerate: u32) -> HandlerResult {
    args.ui.video_framerate = framerate;
    let mut config = SnapPeaConfig::load();
    config.video_framerate = framerate;
    config.save();
    cosmic::Task::none()
}
