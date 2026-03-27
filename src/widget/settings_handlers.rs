//! Handlers for settings-related messages
//!
//! Handles: ToolbarPositionChange, ToggleSettingsDrawer, ToggleMagnifier,
//!          SetSaveLocation, ToggleCopyOnSave, SetVideoEncoder, SetVideoContainer,
//!          SetVideoFramerate

use std::io::Write;

use crate::config::{Container, SaveLocationChoice, SnapPeaConfig, ToolbarPosition, VideoSaveLocationChoice};
use crate::screenshot::Args;
use crate::screenshot::handlers::HandlerResult;

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
    args.ui.save_location_setting = SaveLocationChoice::Pictures;
    let mut config = SnapPeaConfig::load();
    config.save_location = args.ui.save_location_setting;
    config.save();
    cosmic::Task::none()
}

/// Handle SetSaveLocationDocuments message
pub fn handle_set_save_location_documents(args: &mut Args) -> HandlerResult {
    args.ui.save_location_setting = SaveLocationChoice::Documents;
    let mut config = SnapPeaConfig::load();
    config.save_location = args.ui.save_location_setting;
    config.save();
    cosmic::Task::none()
}

/// Handle SetSaveLocationCustom message
pub fn handle_set_save_location_custom(args: &mut Args) -> HandlerResult {
    args.ui.save_location_setting = SaveLocationChoice::Custom;
    let mut config = SnapPeaConfig::load();
    config.save_location = args.ui.save_location_setting;
    config.save();
    cosmic::Task::none()
}

/// Handle SetCustomSavePath message
pub fn handle_set_custom_save_path(args: &mut Args, path: String) -> HandlerResult {
    args.ui.custom_save_path = path.clone();
    let mut config = SnapPeaConfig::load();
    config.custom_save_path = path;
    config.save();
    cosmic::Task::none()
}

// Note: BrowseSaveLocation is handled specially in screenshot/mod.rs
// to support hiding/restoring the overlay when the file dialog opens.

/// Handle SetVideoSaveLocation message
pub fn handle_set_video_save_location(args: &mut Args, loc: VideoSaveLocationChoice) -> HandlerResult {
    args.ui.video_save_location_setting = loc;
    let mut config = SnapPeaConfig::load();
    config.video_save_location = loc;
    config.save();
    cosmic::Task::none()
}

/// Handle SetVideoCustomSavePath message
pub fn handle_set_video_custom_save_path(args: &mut Args, path: String) -> HandlerResult {
    args.ui.video_custom_save_path = path.clone();
    let mut config = SnapPeaConfig::load();
    config.video_custom_save_path = path;
    config.save();
    cosmic::Task::none()
}

// Note: BrowseVideoSaveLocation is handled specially in screenshot/mod.rs
// to support hiding/restoring the overlay when the file dialog opens.

/// Handle ToggleCopyOnSave message
pub fn handle_toggle_copy_on_save(args: &mut Args) -> HandlerResult {
    args.ui.copy_to_clipboard_on_save = !args.ui.copy_to_clipboard_on_save;
    let mut config = SnapPeaConfig::load();
    config.copy_to_clipboard_on_save = args.ui.copy_to_clipboard_on_save;
    config.save();
    cosmic::Task::none()
}

// Note: SettingsTab activation is handled directly in screenshot/mod.rs
// because it needs access to app.settings_tab_model

/// Handle SetToolbarOpacity message
/// Updates UI immediately and schedules a debounced save to config
pub fn handle_set_toolbar_opacity(args: &mut Args, opacity: f32) -> HandlerResult {
    // Update UI immediately for responsive feedback
    args.ui.toolbar_unhovered_opacity = opacity.clamp(0.1, 1.0);

    // Increment save ID to invalidate any pending saves
    args.ui.toolbar_opacity_save_id = args.ui.toolbar_opacity_save_id.wrapping_add(1);
    let save_id = args.ui.toolbar_opacity_save_id;

    // Schedule a debounced save (500ms delay)
    cosmic::Task::perform(
        async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            (opacity, save_id)
        },
        |(opacity, save_id)| {
            crate::core::app::Msg::Screenshot(crate::session::messages::Msg::Settings(
                crate::session::messages::SettingsMsg::SaveToolbarOpacityDebounced(
                    opacity, save_id,
                ),
            ))
        },
    )
}

/// Handle SaveToolbarOpacityDebounced message
/// Only saves to config if the save ID matches (no newer changes)
pub fn handle_save_toolbar_opacity_debounced(
    args: &mut Args,
    opacity: f32,
    save_id: u64,
) -> HandlerResult {
    // Only save if this is still the latest change (ID matches)
    if args.ui.toolbar_opacity_save_id == save_id {
        let mut config = SnapPeaConfig::load();
        config.toolbar_unhovered_opacity = opacity.clamp(0.1, 1.0);
        config.save();
    }
    // If IDs don't match, a newer change came in, so skip this save
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

/// Handle SetAsDefaultPortal toggle message
///
/// - When enabling:
///   1. Writes `~/.local/share/xdg-desktop-portal/portals/snappea.portal`
///   2. Writes `~/.local/share/dbus-1/services/io.github.hojjatabdollahi.snappea.service`
///      with Exec pointing to the currently running binary.
///   3. Writes `~/.config/xdg-desktop-portal/cosmic-portals.conf`
/// - When disabling: removes all three files (cleaning up portals.conf carefully).
/// Restarts the user's xdg-desktop-portal service to apply the change.
pub fn handle_set_as_default_portal(args: &mut Args) -> HandlerResult {
    let enabling = !args.ui.is_default_portal;

    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let local_data_dir = dirs::data_local_dir().ok_or("could not determine local data dir")?;

        let portal_dir = local_data_dir.join("xdg-desktop-portal/portals");
        let portal_file = portal_dir.join("snappea.portal");

        let service_dir = local_data_dir.join("dbus-1/services");
        let service_file =
            service_dir.join("io.github.hojjatabdollahi.snappea.service");

        let config_dir = dirs::config_dir()
            .ok_or("could not determine config directory")?
            .join("xdg-desktop-portal");
        std::fs::create_dir_all(&config_dir)?;
        let conf_path = config_dir.join("cosmic-portals.conf");

        if enabling {
            // 1. Write user-local .portal file
            std::fs::create_dir_all(&portal_dir)?;
            std::fs::write(
                &portal_file,
                "[portal]\nDBusName=io.github.hojjatabdollahi.snappea\nInterfaces=org.freedesktop.impl.portal.Screenshot\nUseIn=COSMIC\n",
            )?;
            log::info!("snappea: portal file written to {}", portal_file.display());

            // 2. Write user-local D-Bus session service file
            std::fs::create_dir_all(&service_dir)?;
            let exe = std::env::current_exe()
                .unwrap_or_else(|_| std::path::PathBuf::from("snappea"));
            let service_content = format!(
                "[D-BUS Service]\nName=io.github.hojjatabdollahi.snappea\nExec={} --portal\n",
                exe.display()
            );
            std::fs::write(&service_file, service_content)?;
            log::info!("snappea: D-Bus service file written to {}", service_file.display());

            // 3. Write portals.conf
            let mut file = std::fs::File::create(&conf_path)?;
            file.write_all(
                b"[preferred]\ndefault=cosmic;gtk;\norg.freedesktop.impl.portal.Screenshot=snappea\n",
            )?;
            log::info!("snappea: portal config written to {}", conf_path.display());
        } else {
            // Remove .portal file
            if portal_file.exists() {
                let _ = std::fs::remove_file(&portal_file);
                log::info!("snappea: removed portal file {}", portal_file.display());
            }

            // Remove D-Bus service file
            if service_file.exists() {
                let _ = std::fs::remove_file(&service_file);
                log::info!("snappea: removed D-Bus service file {}", service_file.display());
            }

            // Remove only the Screenshot line from portals.conf; delete file if nothing meaningful remains
            if conf_path.exists() {
                let contents = std::fs::read_to_string(&conf_path)?;
                let filtered: String = contents
                    .lines()
                    .filter(|l| {
                        l.trim() != "org.freedesktop.impl.portal.Screenshot=snappea"
                    })
                    .map(|l| format!("{}\n", l))
                    .collect();
                let meaningful = filtered
                    .lines()
                    .any(|l| !l.trim().is_empty() && !l.trim().starts_with('['));
                if meaningful {
                    std::fs::write(&conf_path, filtered)?;
                } else {
                    std::fs::remove_file(&conf_path)?;
                }
            }
            log::info!("snappea: removed default portal entry");
        }

        // Restart xdg-desktop-portal so the change takes effect immediately
        let status = std::process::Command::new("systemctl")
            .args(["--user", "restart", "xdg-desktop-portal"])
            .status();
        match status {
            Ok(s) if s.success() => log::info!("snappea: xdg-desktop-portal restarted"),
            Ok(s) => log::warn!("snappea: xdg-desktop-portal restart exited with {}", s),
            Err(e) => log::warn!("snappea: could not restart xdg-desktop-portal: {}", e),
        }

        Ok(())
    })();

    match result {
        Ok(()) => args.ui.is_default_portal = enabling,
        Err(e) => log::error!("snappea: failed to toggle default portal: {}", e),
    }

    cosmic::Task::none()
}
