// SPDX-License-Identifier: GPL-3.0-only

//! Message handling for a capture in progress.

use super::detect::handle_ocr_msg;
use super::detect::handle_open_url_inner;
use super::detect::handle_qr_msg;
use super::detect::handle_qr_requested_inner;
use super::flow::handle_cancel_inner;
use super::flow::handle_capture_inner;
use super::flow::recreate_screenshot_surfaces;
use super::flow::send_portal_response;
use crate::app::App;
use crate::app::Msg as AppMsg;
use crate::capture::Capture;
use crate::capture::msg::ActionMsg;
use crate::capture::msg::DetectMsg;
use crate::capture::msg::Direction;
use crate::capture::msg::DrawAction;
use crate::capture::msg::DrawMsg;
use crate::capture::msg::Msg;
use crate::capture::msg::OcrMsg;
use crate::capture::msg::QrMsg;
use crate::capture::msg::SelectMsg;
use crate::capture::msg::SettingsMsg;
use crate::capture::msg::ToolMsg;
use crate::capture::msg::ToolPopupAction;
use crate::config::Config;
use crate::config::Container;
use crate::config::RedactTool;
use crate::config::SaveLocationChoice;
use crate::config::ShapeTool;
use crate::config::VideoSaveLocationChoice;
use crate::dbus::PortalResponse;
use crate::fl;
use crate::geometry::Choice;
use crate::geometry::DragState;
use crate::geometry::ImageSaveLocation;
use crate::geometry::Rect;
use crate::recording::overlay::RecordingIndicator;
use crate::wayland::CaptureSource;
use cosmic::iced::core::Point;
use cosmic::iced::platform_specific::shell::commands::layer_surface::destroy_layer_surface;
use cosmic::iced::window;
use std::io::Write;
use std::time::Instant;
use viewer_tools::annotate::AnnotateColor;
use viewer_tools::annotate::HighlighterOperation;
use viewer_tools::annotate::MagnifierOperation;
use viewer_tools::annotate::PenOperation;
use viewer_tools::annotate::PixelateOperation;
use viewer_tools::annotate::RedactOperation;
use viewer_tools::annotate::ShapeKind;
use viewer_tools::annotate::ShapeOperation;
use wayland_client::protocol::wl_output::WlOutput;

/// Drive the color picker and adopt the color it applies. `AppliedColor`,
/// `ActionFinished` and `ToggleColorPicker` can all carry one, as in cosmic-viewer.
pub fn handle_color_picker(
    app: &mut App,
    update: cosmic::widget::color_picker::ColorPickerUpdate,
) -> cosmic::Task<crate::app::Msg> {
    use cosmic::widget::color_picker::ColorPickerUpdate;

    let was_active = app.color_picker.get_is_active();
    let adopts = matches!(
        update,
        ColorPickerUpdate::AppliedColor
            | ColorPickerUpdate::ActionFinished
            | ColorPickerUpdate::ToggleColorPicker
    );
    // Closing on a color counts only if the panel was open to begin with.
    let closing = matches!(update, ColorPickerUpdate::ToggleColorPicker);
    let applies = matches!(update, ColorPickerUpdate::AppliedColor);

    // The picker's own task carries widget-internal work. Its result is not
    // something this app acts on.
    let task: cosmic::Task<crate::app::Msg> = app
        .color_picker
        .update::<crate::app::Msg>(update)
        .map(|_| crate::app::Msg::Ignore);

    if adopts
        && (!closing || was_active)
        && let Some(color) = app.color_picker.get_applied_color()
        && let Some(capture) = app.capture.as_mut()
    {
        capture.ui.shape_color = AnnotateColor(color);
    }

    // Confirming closes the panel, like choosing a preset.
    if applies && app.color_picker.get_is_active() {
        _ = app
            .color_picker
            .update::<crate::app::Msg>(ColorPickerUpdate::ToggleColorPicker);
    }

    task
}

/// Whether this message finishes the open text label. An allow-list: housekeeping
/// traffic must leave a half-typed label alone.
pub const fn ends_text_edit(msg: &Msg) -> bool {
    use crate::capture::msg::{ToolMsg, ToolPopupAction};

    match msg {
        // Taking the capture, or giving up on it.
        Msg::Action(_) => true,
        // Picking up another tool, or putting the annotation section away.
        // Opening a tool slot's dropdown takes that tool in hand, so it counts.
        Msg::Tool(
            ToolMsg::SetShapeTool(_)
            | ToolMsg::CycleShapeTool
            | ToolMsg::ShapeModeToggle
            | ToolMsg::AnnotateModeToggle
            | ToolMsg::RedactModeToggle
            | ToolMsg::MagnifierModeToggle
            | ToolMsg::SetRedactTool(_)
            | ToolMsg::ShapePopup(ToolPopupAction::Toggle | ToolPopupAction::Open)
            | ToolMsg::FreehandPopup(ToolPopupAction::Toggle | ToolPopupAction::Open)
            | ToolMsg::RedactPopup(ToolPopupAction::Toggle | ToolPopupAction::Open)
            | ToolMsg::MagnifierPopup(ToolPopupAction::Toggle | ToolPopupAction::Open),
        ) => true,
        // Changing what is being captured moves the ground under the label.
        Msg::Select(_) => true,
        // Acting on the annotation list as a whole.
        Msg::Draw(
            DrawMsg::Undo
            | DrawMsg::Redo
            | DrawMsg::ClearShapes
            | DrawMsg::ClearRedactions
            | DrawMsg::ToggleMoveMode,
        ) => true,
        // Everything else leaves it open.
        _ => false,
    }
}

pub fn update_msg(app: &mut App, msg: Msg) -> cosmic::Task<crate::app::Msg> {
    if app.text.is_some() && ends_text_edit(&msg) {
        crate::capture::text::commit(app);
    }

    match msg {
        // Draw messages
        Msg::Draw(draw_msg) => handle_draw_msg(app, draw_msg),

        // Tool messages
        Msg::Tool(crate::capture::msg::ToolMsg::ColorPicker(update)) => {
            handle_color_picker(app, update)
        }
        Msg::Tool(tool_msg) => handle_tool_msg(app, tool_msg),

        // Selection messages
        Msg::Select(select_msg) => handle_select_msg(app, select_msg),

        // === Settings messages - UI and config ===
        Msg::Settings(settings_msg) => handle_settings_msg(app, settings_msg),

        // === Detection messages - OCR and QR ===
        Msg::Detect(detect_msg) => handle_detect_msg(app, detect_msg),

        // Capture messages
        Msg::Action(capture_msg) => handle_capture_msg(app, capture_msg),
    }
}

/// Handle Draw messages (annotation drawing)
/// Delegates to the annotations module handler
pub fn handle_draw_msg(app: &mut App, msg: DrawMsg) -> cosmic::Task<crate::app::Msg> {
    // Text needs the live editor on the app, so it is handled before delegating.
    if let DrawMsg::Text(action) = msg {
        if let Some(capture) = app.capture.as_mut() {
            capture.ui.exit_armed = false;
        }
        return crate::capture::text::handle(app, action);
    }

    let persist = false;
    if let Some(capture) = app.capture.as_mut() {
        // Further annotation work makes a pending exit confirmation stale.
        capture.ui.exit_armed = false;
        apply_draw(capture, msg);
        if persist {
            save_tool_config(capture);
        }
    }
    cosmic::Task::none()
}

/// Handle Tool messages (popup and tool configuration)
/// Delegates to the `widget::tool_handlers` module
pub fn handle_tool_msg(app: &mut App, msg: ToolMsg) -> cosmic::Task<crate::app::Msg> {
    // Handle PencilPopup actions for indicator overlay
    if let ToolMsg::PencilPopup(action) = &msg
        && let Some(indicator) = &mut app.recording_indicator
    {
        match action {
            crate::capture::msg::ToolPopupAction::Toggle => {
                indicator.pencil_popup_open = !indicator.pencil_popup_open;
                if indicator.pencil_popup_open {
                    // DISABLE annotation mode when opening popup (prevent accidental drawing)
                    indicator.annotation_mode = false;
                    if let Some(capture) = app.capture.as_mut() {
                        capture.ui.recording_annotation_mode = false;
                    }
                } else {
                    // ENABLE annotation mode when closing popup (ready to draw)
                    indicator.annotation_mode = true;
                    if let Some(capture) = app.capture.as_mut() {
                        capture.ui.recording_annotation_mode = true;
                    }
                }
            }
            crate::capture::msg::ToolPopupAction::Open => {
                indicator.pencil_popup_open = true;
                // DISABLE annotation mode when opening popup
                indicator.annotation_mode = false;
                if let Some(capture) = app.capture.as_mut() {
                    capture.ui.recording_annotation_mode = false;
                }
            }
            crate::capture::msg::ToolPopupAction::Close => {
                indicator.pencil_popup_open = false;
                // ENABLE annotation mode when closing popup (ready to draw)
                indicator.annotation_mode = true;
                if let Some(capture) = app.capture.as_mut() {
                    capture.ui.recording_annotation_mode = true;
                }
            }
        }

        // Update popup bounds: must match actual rendered position
        // Popup follows toolbar position and appears above or below based on available space
        if indicator.pencil_popup_open {
            let popup_width = 262.0_f32; // 230 content + 16*2 padding
            let popup_height = 380.0_f32; // Approximate height of popup content (increased to prevent overlap)
            let popup_gap = 16.0_f32; // Gap between popup and toolbar
            let toolbar_height = 72.0_f32; // Toolbar height including padding

            // Popup x follows toolbar x position
            let popup_x = indicator.toolbar_pos.0.max(0.0);

            // Determine if popup should appear above or below toolbar
            let space_above = indicator.toolbar_pos.1;
            let space_below = indicator.output_size.1 - indicator.toolbar_pos.1 - toolbar_height;

            let popup_y = if space_above >= popup_height + popup_gap {
                // Place above toolbar
                indicator.toolbar_pos.1 - popup_gap - popup_height
            } else if space_below >= popup_height + popup_gap {
                // Place below toolbar
                indicator.toolbar_pos.1 + toolbar_height + popup_gap
            } else {
                // Not enough space either way, place above and let it clip at top
                (indicator.toolbar_pos.1 - popup_gap - popup_height).max(0.0)
            };

            indicator.pencil_popup_bounds = Some(cosmic::iced::core::Rectangle {
                x: popup_x,
                y: popup_y,
                width: popup_width,
                height: popup_height,
            });
        } else {
            indicator.pencil_popup_bounds = None;
        }

        // Recreate surface with updated input_zone
        return cosmic::Task::done(crate::app::Msg::Recording(
            crate::recording::overlay::Msg::ToggleAnnotationMode,
        ));
    }

    // Handle ClearPencilDrawings specially since it needs access to recording_indicator
    if matches!(&msg, ToolMsg::ClearPencilDrawings) {
        if let Some(indicator) = &mut app.recording_indicator {
            indicator.annotations.clear();
            indicator.current_stroke = None;
        }
        return cosmic::Task::none();
    }

    // Handle SetPencilColor, SetPencilFadeDuration, SetPencilThickness: update recording_indicator too
    match &msg {
        ToolMsg::SetPencilColor(color) => {
            if let Some(indicator) = &mut app.recording_indicator {
                indicator.pencil_color = *color;
            }
        }
        ToolMsg::SetPencilFadeDuration(duration) => {
            if let Some(indicator) = &mut app.recording_indicator {
                indicator.pencil_fade_duration = *duration;
            }
        }
        ToolMsg::SetPencilThickness(thickness) => {
            if let Some(indicator) = &mut app.recording_indicator {
                indicator.pencil_thickness = *thickness;
            }
        }
        _ => {}
    }

    if let Some(capture) = app.capture.as_mut() {
        let needs_save = apply_tool(capture, msg);
        if needs_save {
            save_tool_config(capture);
        }
    }
    cosmic::Task::none()
}

/// Handle Select messages (mode and navigation)
pub fn handle_select_msg(app: &mut App, msg: SelectMsg) -> cosmic::Task<crate::app::Msg> {
    match msg {
        SelectMsg::RegionMode => handle_select_region_mode(app),
        SelectMsg::ScreenMode(idx) => handle_select_screen_mode(app, idx),
        SelectMsg::Navigate(dir) => match dir {
            Direction::Left => handle_navigate_left(app),
            Direction::Right => handle_navigate_right(app),
            Direction::Up => handle_navigate_left(app), // Same as Left
            Direction::Down => handle_navigate_right(app), // Same as Right
        },
        SelectMsg::Confirm => handle_confirm_selection(app),
    }
}

/// Handle Settings messages (UI and config)
pub fn handle_settings_msg(app: &mut App, msg: SettingsMsg) -> cosmic::Task<crate::app::Msg> {
    // Opening and closing the settings menu.
    if matches!(msg, SettingsMsg::ToggleDrawer) {
        if let Some(capture) = app.capture.as_mut() {
            capture.ui.settings_drawer_open = !capture.ui.settings_drawer_open;

            // Opening the settings menu puts the tools down and closes
            // anything else that was up.
            if capture.ui.settings_drawer_open {
                capture.ui.shape_popup_open = false;
                capture.ui.redact_popup_open = false;
                capture.disable_all_modes();
            }
        }
        return cosmic::Task::none();
    }

    // Handle BrowseSaveLocation specially: need to hide overlay, open dialog, then restore
    if matches!(msg, SettingsMsg::BrowseSaveLocation) {
        // Destroy layer surfaces (but keep capture intact)
        let destroy_tasks: Vec<_> = app
            .outputs
            .iter()
            .map(|o| destroy_layer_surface(o.id))
            .collect();

        // Open file dialog and send result
        let dialog_task = cosmic::Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title(fl!("browse-screenshots-title"))
                    .pick_folder()
                    .await
                    .map(|handle| handle.path().to_string_lossy().to_string())
            },
            |result| {
                crate::app::Msg::Screenshot(crate::capture::msg::Msg::browse_save_location_result(
                    result,
                ))
            },
        );

        return cosmic::Task::batch(destroy_tasks).chain(dialog_task);
    }

    // Handle BrowseSaveLocationResult: restore overlay and optionally set path
    if let SettingsMsg::BrowseSaveLocationResult(path) = msg {
        // Choosing a folder also selects it. A cancelled dialog changes nothing.
        if let Some(path) = path
            && let Some(capture) = app.capture.as_mut()
        {
            capture.ui.custom_save_path = path.clone();
            capture.ui.save_location_setting = SaveLocationChoice::Custom;
            Config::store("custom_save_path", path);
            Config::store("save_location", SaveLocationChoice::Custom);
        }

        // Recreate layer surfaces
        return recreate_screenshot_surfaces(app);
    }

    // Handle BrowseVideoSaveLocation specially: same pattern
    if matches!(msg, SettingsMsg::BrowseVideoSaveLocation) {
        let destroy_tasks: Vec<_> = app
            .outputs
            .iter()
            .map(|o| destroy_layer_surface(o.id))
            .collect();

        let dialog_task = cosmic::Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title(fl!("browse-videos-title"))
                    .pick_folder()
                    .await
                    .map(|handle| handle.path().to_string_lossy().to_string())
            },
            |result| {
                crate::app::Msg::Screenshot(
                    crate::capture::msg::Msg::browse_video_save_location_result(result),
                )
            },
        );

        return cosmic::Task::batch(destroy_tasks).chain(dialog_task);
    }

    // Handle BrowseVideoSaveLocationResult: restore overlay and optionally set path
    if let SettingsMsg::BrowseVideoSaveLocationResult(path) = msg {
        if let Some(path) = path
            && let Some(capture) = app.capture.as_mut()
        {
            capture.ui.video_custom_save_path = path.clone();
            capture.ui.video_save_location_setting = VideoSaveLocationChoice::Custom;
            Config::store("video_custom_save_path", path);
            Config::store("video_save_location", VideoSaveLocationChoice::Custom);
        }

        return recreate_screenshot_surfaces(app);
    }

    let Some(capture) = app.capture.as_mut() else {
        log::error!("No capture in progress");
        return cosmic::Task::none();
    };
    match msg {
        SettingsMsg::ToolbarDragStart => handle_toolbar_drag_start(capture),
        SettingsMsg::ToolbarDragMove(output, cursor) => {
            handle_toolbar_drag_move(capture, output, cursor)
        }
        SettingsMsg::ToolbarDragEnd => handle_toolbar_drag_end(capture),
        SettingsMsg::ToggleDrawer => {
            // Already handled above
            cosmic::Task::none()
        }
        SettingsMsg::ToggleMagnifier => handle_toggle_magnifier(capture),
        SettingsMsg::SetSaveLocation(loc) => match loc {
            SaveLocationChoice::Clipboard => handle_set_save_location_clipboard(capture),
            SaveLocationChoice::Pictures => handle_set_save_location_pictures(capture),
            SaveLocationChoice::Documents => handle_set_save_location_documents(capture),
            SaveLocationChoice::Custom => handle_set_save_location_custom(capture),
        },
        SettingsMsg::SetCustomSavePath(path) => handle_set_custom_save_path(capture, path),
        SettingsMsg::BrowseSaveLocation => {
            // Already handled above (special handling for overlay hide/restore)
            cosmic::Task::none()
        }
        SettingsMsg::BrowseSaveLocationResult(_) => {
            // Already handled above
            cosmic::Task::none()
        }
        SettingsMsg::SetVideoSaveLocation(loc) => handle_set_video_save_location(capture, loc),
        SettingsMsg::SetVideoCustomSavePath(path) => {
            handle_set_video_custom_save_path(capture, path)
        }
        SettingsMsg::BrowseVideoSaveLocation => {
            // Already handled above (special handling for overlay hide/restore)
            cosmic::Task::none()
        }
        SettingsMsg::BrowseVideoSaveLocationResult(_) => {
            // Already handled above
            cosmic::Task::none()
        }
        SettingsMsg::SetSaveLocationClipboard => handle_set_save_location_clipboard(capture),
        SettingsMsg::SetCaptureDelay(secs) => {
            capture.ui.capture_delay_secs = secs;
            capture.ui.delay_popup_open = false;
            crate::config::Config::store("capture_delay_secs", secs);
            cosmic::Task::none()
        }
        SettingsMsg::ToolbarBounds(bounds) => {
            capture.ui.toolbar_bounds = Some(bounds);
            if let Some(indicator) = &mut app.recording_indicator {
                indicator.toolbar_bounds = Some(bounds);
            }
            cosmic::Task::none()
        }
        SettingsMsg::SetVideoEncoder(encoder) => handle_set_video_encoder(capture, encoder),
        SettingsMsg::SetVideoContainer(container) => handle_set_video_container(capture, container),
        SettingsMsg::SetVideoFramerate(framerate) => handle_set_video_framerate(capture, framerate),
        SettingsMsg::ToggleShowCursor => {
            capture.ui.video_show_cursor = !capture.ui.video_show_cursor;
            crate::config::Config::store("video_show_cursor", capture.ui.video_show_cursor);
            cosmic::Task::none()
        }
        SettingsMsg::ToggleScreenshotCursor => {
            capture.ui.show_cursor = !capture.ui.show_cursor;
            crate::config::Config::store("show_cursor", capture.ui.show_cursor);
            cosmic::Task::none()
        }
        SettingsMsg::EncodersDetected(encoders) => {
            log::debug!("Encoders detected: {} available", encoders.len());

            // The container must suit the encoder that will actually run here.
            let effective = capture
                .ui
                .selected_encoder
                .clone()
                .or_else(|| encoders.first().map(|e| e.gst_element.clone()));
            if let Some(element) = effective {
                let codec = crate::recording::encoder::Codec::from_element_name(&element);
                let container =
                    crate::recording::encoder::container_for(codec, capture.ui.video_container);
                if container != capture.ui.video_container {
                    log::info!(
                        "container {:?} cannot hold {element}; using {container:?}",
                        capture.ui.video_container
                    );
                    capture.ui.video_container = container;
                    crate::config::Config::store("video_container", container);
                }
            }

            capture.ui.available_encoders = encoders;
            cosmic::Task::none()
        }
        SettingsMsg::TimelineTick(_, instant) => {
            capture.ui.now = instant;
            // No selection, no annotation tools.
            if capture.ui.annotate_mode && !capture.selection.choice.has_selection() {
                capture.leave_annotate_mode();
            }
            // `sync` only starts sections whose target changed, so this is cheap per tick.
            let sections = crate::capture::ToolbarSections::of(crate::capture::ToolbarMode {
                annotating: capture.ui.annotate_mode,
                video: capture.ui.is_video_mode,
                has_selection: capture.selection.choice.has_selection(),
                annotation_selected: capture.annotations.selected.is_some(),
                // Only a dragged region, and only where tesseract is
                // installed to read it.
                can_ocr: capture.ui.tesseract_available
                    && matches!(
                        &capture.selection.choice,
                        crate::geometry::Choice::Rectangle(r, _)
                            if r.dimensions().is_some()
                    ),
            });
            capture.ui.toolbar_anim.sync(sections, instant);
            cosmic::Task::none()
        }
        SettingsMsg::SetMoveOffset(offset) => {
            capture.ui.move_offset = offset;
            cosmic::Task::none()
        }
        SettingsMsg::ToggleRecognizeQrCodes => {
            capture.ui.recognize_qr_codes = !capture.ui.recognize_qr_codes;
            crate::config::Config::store("recognize_qr_codes", capture.ui.recognize_qr_codes);
            cosmic::Task::none()
        }
        SettingsMsg::SetAsDefaultPortal => handle_set_as_default_portal(capture),
    }
}

/// Handle Detect messages (OCR and QR)
pub fn handle_detect_msg(app: &mut App, msg: DetectMsg) -> cosmic::Task<crate::app::Msg> {
    if let Some(capture) = app.capture.as_mut()
        && matches!(
            msg,
            DetectMsg::Qr(QrMsg::Requested | QrMsg::CopyAndClose)
                | DetectMsg::Ocr(OcrMsg::Requested | OcrMsg::CopyAndClose)
        )
    {
        capture.disable_all_modes();
        capture.close_all_popups();
    }
    match msg {
        DetectMsg::Qr(qr_msg) => handle_qr_msg(app, qr_msg),
        DetectMsg::Ocr(ocr_msg) => handle_ocr_msg(app, ocr_msg),
    }
}

/// Handle Capture messages (capture workflow)
pub fn handle_capture_msg(app: &mut App, msg: ActionMsg) -> cosmic::Task<crate::app::Msg> {
    match msg {
        ActionMsg::Capture => handle_capture_inner(app),
        ActionMsg::Cancel => handle_cancel_inner(app),
        ActionMsg::CancelRequested => {
            // Escape with work in progress arms an exit confirmation instead of
            // discarding it outright. A second Escape goes through.
            let needs_confirm = app.capture.as_ref().is_some_and(|capture| {
                !capture.ui.exit_armed && capture.annotations.has_unsaved_work()
            });
            if needs_confirm {
                if let Some(capture) = app.capture.as_mut() {
                    capture.ui.exit_armed = true;
                }
                cosmic::Task::none()
            } else {
                handle_cancel_inner(app)
            }
        }
        ActionMsg::CopyToClipboard => {
            if let Some(capture) = app.capture.as_mut() {
                capture.selection.location = ImageSaveLocation::Clipboard;
            }
            handle_capture_inner(app)
        }
        ActionMsg::SaveToPictures => {
            if let Some(capture) = app.capture.as_mut() {
                capture.selection.location = match capture.ui.save_location_setting {
                    SaveLocationChoice::Clipboard => ImageSaveLocation::Clipboard,
                    SaveLocationChoice::Pictures => ImageSaveLocation::Pictures,
                    SaveLocationChoice::Documents => ImageSaveLocation::Documents,
                    SaveLocationChoice::Custom => {
                        // For custom save location, we use Pictures as the base
                        // but the actual path is handled in save_screenshot
                        ImageSaveLocation::Pictures
                    }
                };
                // A folder saves a file and copies to the clipboard. Only the clipboard row skips the file.
                capture.selection.also_copy_to_clipboard =
                    capture.selection.location != ImageSaveLocation::Clipboard;
            }
            handle_capture_inner(app)
        }
        ActionMsg::CycleCaptureDelay => {
            if let Some(capture) = app.capture.as_mut() {
                let next = match capture.ui.capture_delay_secs {
                    0..=3 => 5,
                    4..=5 => 10,
                    _ => 3,
                };
                capture.ui.capture_delay_secs = next;
                crate::config::Config::store("capture_delay_secs", next);
            }
            cosmic::Task::none()
        }
        ActionMsg::DelayedCapture => {
            let Some(capture) = app.capture.as_ref() else {
                return cosmic::Task::none();
            };
            let delay = capture.ui.capture_delay_secs.max(1);

            // The overlay stays up because it is capture-excluded. It stops drawing the selection
            // UI and shows the countdown pill on every output.
            let toolbar_pos = capture.ui.toolbar_pos.clone();
            let outgoing_tools = capture.ui.toolbar_anim.progress(capture.ui.now).tools;
            let transition_size =
                crate::widget::toolbar::countdown_toolbar_transition_size(outgoing_tools);
            let windows: Vec<_> = app
                .outputs
                .iter()
                .map(|output| {
                    let output_size = (output.logical_size.0 as f32, output.logical_size.1 as f32);
                    let transition_from_pos = toolbar_pos.get(&output.name).map_or_else(
                        || {
                            (
                                ((output_size.0 - transition_size.0) / 2.0).max(0.0),
                                (output_size.1 - transition_size.1 - 32.0).max(0.0),
                            )
                        },
                        |p| {
                            (
                                p.x.clamp(0.0, (output_size.0 - transition_size.0).max(0.0)),
                                p.y.clamp(0.0, (output_size.1 - transition_size.1).max(0.0)),
                            )
                        },
                    );
                    // The pill takes the toolbar's place, or the bottom center by default.
                    let pos = toolbar_pos.get(&output.name).map_or_else(
                        || crate::capture::countdown::Countdown::default_pos(output_size),
                        |p| crate::capture::countdown::clamp_pos((p.x, p.y), output_size),
                    );
                    crate::capture::countdown::CountdownWindow {
                        // The output's own overlay surface, taking a new job.
                        id: output.id,
                        output_name: output.name.clone(),
                        output_size,
                        pos,
                        transition_from_pos,
                        transition_bounds: crate::capture::countdown::input_zone_for_size(
                            transition_from_pos,
                            transition_size,
                        ),
                    }
                })
                .collect();

            let countdown =
                crate::capture::countdown::Countdown::new(delay, windows, outgoing_tools);
            let tasks = [
                crate::app::App::enter_countdown(&countdown),
                crate::app::countdown_fire(&countdown),
                crate::app::countdown_refresh(&countdown),
            ];
            app.countdown = Some(countdown);

            cosmic::Task::batch(tasks)
        }
        ActionMsg::RecordRegion => {
            // Get region from selection state
            let Some(capture) = app.capture.as_ref() else {
                log::warn!("Record clicked but no screenshot capture available");
                return cosmic::Task::none();
            };

            let region = match &capture.selection.choice {
                Choice::Rectangle(rect, _) if rect.width() > 0 && rect.height() > 0 => (
                    rect.left,
                    rect.top,
                    rect.width() as u32,
                    rect.height() as u32,
                ),
                Choice::Output(Some(output_name)) => {
                    // Find output dimensions
                    if let Some(output) = app.outputs.iter().find(|o| &o.name == output_name) {
                        (
                            output.logical_pos.0,
                            output.logical_pos.1,
                            output.logical_size.0,
                            output.logical_size.1,
                        )
                    } else {
                        log::warn!("Record clicked but output not found: {output_name}");
                        return cosmic::Task::none();
                    }
                }
                _ => {
                    log::warn!("Record clicked but no valid region selected");
                    return cosmic::Task::none();
                }
            };

            super::flow::remember_capture(
                crate::config::CaptureMode::Video,
                &capture.selection.choice,
            );

            // Find output with most overlap with the region
            let region_rect = crate::geometry::Rect {
                left: region.0,
                top: region.1,
                right: region.0 + region.2 as i32,
                bottom: region.1 + region.3 as i32,
            };

            let selected_output = app
                .outputs
                .iter()
                .filter_map(|output| {
                    let output_rect = crate::geometry::Rect {
                        left: output.logical_pos.0,
                        top: output.logical_pos.1,
                        right: output.logical_pos.0 + output.logical_size.0 as i32,
                        bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                    };

                    // Calculate overlap area
                    region_rect.intersect(output_rect).map(|intersection| {
                        let overlap_area = (intersection.right - intersection.left) as u64
                            * (intersection.bottom - intersection.top) as u64;
                        (output, overlap_area)
                    })
                })
                .max_by_key(|(_, area)| *area)
                .map(|(output, area)| {
                    log::info!(
                        "Selected output '{}' with {} pixels of overlap",
                        output.name,
                        area
                    );
                    output
                });

            let (output_name, local_region, output_logical_size) = if let Some(output) =
                selected_output
            {
                // Translate global region to output-local LOGICAL coordinates
                // The recorder will scale to physical using actual screencopy dimensions
                let local_x = (region.0 - output.logical_pos.0).max(0);
                let local_y = (region.1 - output.logical_pos.1).max(0);

                // Clamp to output logical bounds
                let clamped_w = region
                    .2
                    .min((output.logical_size.0 as i32 - local_x).max(0) as u32);
                let clamped_h = region
                    .3
                    .min((output.logical_size.1 as i32 - local_y).max(0) as u32);

                log::info!(
                    "Translated region: global ({}, {}, {}x{}) -> local logical ({}, {}, {}x{}) on output '{}' (logical_size={}x{})",
                    region.0,
                    region.1,
                    region.2,
                    region.3,
                    local_x,
                    local_y,
                    clamped_w,
                    clamped_h,
                    output.name,
                    output.logical_size.0,
                    output.logical_size.1
                );

                (
                    output.name.clone(),
                    (local_x, local_y, clamped_w, clamped_h),
                    (output.logical_size.0, output.logical_size.1),
                )
            } else {
                // Fallback to first output with original region
                log::warn!("No output overlap found, using first output as fallback");
                let (output_name, logical_size) = app.outputs.first().map_or_else(
                    || ("Unknown".to_string(), (1920, 1080)),
                    |o| (o.name.clone(), (o.logical_size.0, o.logical_size.1)),
                );
                (output_name, region, logical_size)
            };

            // Generate timestamped output filename
            let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
            let config = crate::config::Config::load();
            // WebM is no longer a recording format (realtime software VP9 is slow and
            // blocky, so it is only an editor export). Coerce any stale saved value.
            let mut container = match config.video_container {
                crate::config::Container::Webm => crate::config::Container::Mp4,
                other => other,
            };
            // Fall back to ~/Videos, never the bare home directory.
            let videos_default =
                || dirs::video_dir().or_else(|| dirs::home_dir().map(|h| h.join("Videos")));
            let output_dir = match config.video_save_location {
                crate::config::VideoSaveLocationChoice::Custom
                    if !config.video_custom_save_path.is_empty() =>
                {
                    Some(std::path::PathBuf::from(&config.video_custom_save_path))
                }
                _ => videos_default(),
            }
            .or_else(videos_default)
            .unwrap_or_else(|| std::path::PathBuf::from("."));

            // Ensure output directory exists
            if !output_dir.exists()
                && let Err(e) = std::fs::create_dir_all(&output_dir)
            {
                log::error!(
                    "Failed to create output directory '{}': {}",
                    output_dir.display(),
                    e
                );
                crate::dbus::notify_error(
                    "Recording failed",
                    format!(
                        "Could not create output directory '{}': {e}",
                        output_dir.display()
                    ),
                );
                return cosmic::Task::none();
            }

            // Determine encoder (from config or best_encoder)
            let encoder = config
                .video_encoder
                .or_else(|| crate::recording::best_encoder().ok().map(|e| e.gst_element))
                .unwrap_or_else(|| "x264enc".to_string());

            // Coerce the container to one the codec can mux into (VP9 in MP4 writes nothing).
            if let Some(codec) =
                crate::recording::encoder::detect_encoders()
                    .ok()
                    .and_then(|list| {
                        list.iter()
                            .find(|e| e.gst_element == encoder)
                            .map(|e| e.codec)
                    })
                && !codec.supports_container(container)
            {
                let fallback = codec.default_container();
                log::warn!(
                    "Codec {} cannot be muxed into {:?}; using {:?} instead",
                    codec.name(),
                    container,
                    fallback
                );
                container = fallback;
            }

            let output_file =
                output_dir.join(format!("recording-{}.{}", timestamp, container.extension()));

            let framerate = config.video_framerate;
            let show_cursor = capture.ui.video_show_cursor;
            let previous_toolbar_pos = capture
                .ui
                .toolbar_pos
                .get(&output_name)
                .map(|position| (position.x, position.y));
            let previous_toolbar_bounds = capture.ui.toolbar_bounds;
            let outgoing_choice = capture.selection.choice.clone();
            log::info!("Cursor visibility setting: {show_cursor}");

            // Look up Wayland objects in app.wayland_helper's own connection. Cosmic-iced's
            // objects belong to a different connection.
            let wl_output = app.wayland_helper.outputs().into_iter().find(|o| {
                app.wayland_helper
                    .output_info(o)
                    .and_then(|info| info.name)
                    .as_deref()
                    == Some(&output_name)
            });

            let capture_source = if let Some(output) = wl_output {
                log::info!("Recording output '{output_name}' region: {local_region:?}");
                CaptureSource::Output(output)
            } else {
                log::error!("Output '{output_name}' not found in wayland_helper");
                crate::dbus::notify_error(
                    "Recording failed",
                    format!("Display output '{output_name}' could not be found."),
                );
                return cosmic::Task::none();
            };

            // Clone wayland_helper for the recording thread
            let wayland_helper = app.wayland_helper.clone();

            // Create stop flag for graceful shutdown
            let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stop_flag_clone = stop_flag.clone();

            // Create recording state
            let recording_state = crate::recording::RecordingState {
                output_file: output_file.clone(),
                region: local_region,
                output_name,
                started_at: chrono::Utc::now().to_rfc3339(),
            };

            // Save recording state to disk for external tools
            if let Err(e) = recording_state.save() {
                log::warn!("Failed to save recording state: {e}");
            }

            // Spawn recording thread
            let output_file_for_thread = output_file.clone();
            let encoder_clone = encoder;
            let thread_handle = std::thread::spawn(move || {
                let result = crate::recording::record(
                    wayland_helper,
                    capture_source,
                    output_file_for_thread,
                    local_region,
                    output_logical_size,
                    encoder_clone,
                    container,
                    framerate,
                    show_cursor,
                    &stop_flag_clone,
                );
                // The UI already shows "recording", so surface a failure as a notification.
                if let Err(ref e) = result {
                    log::error!("Recording failed: {e:#}");
                    crate::dbus::notify_error("Recording failed", format!("{e:#}"));
                }
                result
            });

            // Register the recording handle
            let recording_handle =
                crate::recording::RecordingHandle::new(stop_flag, thread_handle, recording_state);
            crate::recording::set_recording(recording_handle);

            log::info!(
                "Recording started: output: {}, local_region: {:?}",
                output_file.display(),
                local_region
            );

            // Set recording mode in UI state
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.is_recording = true;
                capture.ui.recording_annotation_mode = false;
                // Close any open popups/drawers
                capture.close_all_popups();
            }

            // Calculate toolbar position and bounds for the indicator
            let output_size = selected_output.map_or((1920.0, 1080.0), |o| {
                (o.logical_size.0 as f32, o.logical_size.1 as f32)
            });

            // Sized from what the toolbar actually draws, so the input zone
            // matches it. The pen starts unarmed, so this is its narrow form.
            let (toolbar_width, toolbar_height) =
                crate::widget::toolbar::recording_toolbar_size(false);
            let toolbar_margin = 32.0_f32;

            // Keep the recording toolbar where the capture toolbar was, clamped to its new size.
            let (toolbar_x, toolbar_y) = previous_toolbar_pos.map_or_else(
                || {
                    (
                        ((output_size.0 - toolbar_width) / 2.0).max(0.0),
                        (output_size.1 - toolbar_height - toolbar_margin).max(0.0),
                    )
                },
                |(x, y)| {
                    (
                        x.clamp(0.0, (output_size.0 - toolbar_width).max(0.0)),
                        y.clamp(0.0, (output_size.1 - toolbar_height).max(0.0)),
                    )
                },
            );

            let toolbar_input_bounds = cosmic::iced::core::Rectangle {
                x: toolbar_x,
                y: toolbar_y,
                width: toolbar_width,
                height: toolbar_height,
            };
            let toolbar_transition_bounds = previous_toolbar_bounds.unwrap_or(toolbar_input_bounds);
            let toolbar_transition_from_pos =
                (toolbar_transition_bounds.x, toolbar_transition_bounds.y);

            // Reuse this output's session overlay for the recording chrome. Only
            // live annotations need a new surface, on the captured layer below.
            let indicator_output = selected_output.or_else(|| app.outputs.first());

            if let Some(output) = indicator_output {
                app.recording_indicator = Some(RecordingIndicator {
                    chrome_window_id: output.id,
                    chrome_surface_alive: true,
                    annotation_window_id: window::Id::unique(),
                    annotation_surface_alive: false,
                    output: output.output.clone(),
                    output_size,
                    region: local_region,
                    region_border: config.video_region_border,
                    blink_visible: config.video_region_border,
                    annotations: Vec::new(),
                    current_stroke: None,
                    pointer: None,
                    annotation_mode: false,
                    pencil_color: config.pencil_color,
                    pencil_fade_duration: config.pencil_fade_duration,
                    pencil_thickness: config.pencil_thickness,
                    toolbar_bounds: Some(toolbar_input_bounds),
                    toolbar_pos: (toolbar_x, toolbar_y),
                    toolbar_transition_from_pos,
                    toolbar_transition_bounds,
                    toolbar_transition_started: Instant::now(),
                    recording_started: Instant::now(),
                    toolbar_transition_finished: false,
                    outgoing_choice,
                    toolbar_dragging: false,
                    drag_offset: (0.0, 0.0),
                    fade_popup_open: false,
                    thickness_popup_open: false,
                    pencil_popup_open: false,
                    pencil_popup_bounds: None,
                });

                log::info!(
                    "Reusing capture overlay for recording chrome: region={local_region:?}, toolbar={toolbar_input_bounds:?}"
                );

                let annotation_task = app
                    .recording_indicator
                    .as_mut()
                    .unwrap()
                    .create_annotation_surface();
                cosmic::Task::batch([app.enter_recording(), annotation_task])
            } else {
                cosmic::Task::none()
            }
        }
        ActionMsg::Choice(c) => handle_choice_inner(app, c),
        ActionMsg::Location(loc) => handle_location_inner(app, loc),
        ActionMsg::OutputChanged(wl_output) => handle_output_changed_inner(app, wl_output),
        ActionMsg::OpenUrl(url) => handle_open_url_inner(app, url),
        ActionMsg::ToggleCaptureMode(is_video) => {
            log::info!("Capture mode toggled: is_video_mode = {is_video}");
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.is_video_mode = is_video;
                capture.close_all_popups();
                capture.disable_all_modes();
                capture.clear_annotations();
                // The target carries over. Only a half-finished drag or a target
                // the new mode cannot use is dropped.
                capture.selection.choice =
                    match std::mem::replace(&mut capture.selection.choice, Choice::AllScreens) {
                        Choice::Rectangle(r, _) => Choice::Rectangle(r, DragState::default()),
                        Choice::AllScreens if is_video => {
                            Choice::Rectangle(Rect::default(), DragState::default())
                        }
                        other => other,
                    };
                Config::store(
                    "last_mode",
                    if is_video {
                        crate::config::CaptureMode::Video
                    } else {
                        crate::config::CaptureMode::Screenshot
                    },
                );
            }
            cosmic::Task::none()
        }
        ActionMsg::PencilRightClick => {
            // If popup is open, close it (and enable pencil). Otherwise open popup.
            let action = if app
                .recording_indicator
                .as_ref()
                .is_some_and(|i| i.pencil_popup_open)
            {
                crate::capture::msg::ToolPopupAction::Close
            } else {
                crate::capture::msg::ToolPopupAction::Open
            };
            cosmic::Task::done(crate::app::Msg::Screenshot(crate::capture::msg::Msg::Tool(
                crate::capture::msg::ToolMsg::PencilPopup(action),
            )))
        }
        ActionMsg::StopRecording => {
            log::info!("Stop recording requested");
            // Stop the recording
            if let Err(e) = crate::recording::stop_recording() {
                log::error!("Failed to stop recording: {e}");
            }

            // Close the captured annotation surface and all of the session
            // overlays, including the one borrowed by the recording toolbar.
            let indicator_task = if let Some(mut indicator) = app.recording_indicator.take() {
                indicator.destroy_surfaces(&app.outputs)
            } else {
                cosmic::Task::none()
            };

            // Clear the screenshot session completely. The output records stay
            // valid. Only their layer surfaces were closed above.
            if let Some(capture) = app.capture.take() {
                send_portal_response(
                    capture.portal.tx,
                    capture.portal.expects_response,
                    PortalResponse::Cancelled,
                );
            }

            // Don't clear app.outputs: they are Wayland outputs that remain valid.
            // Their window IDs are stale and will be replaced next session.

            indicator_task
        }
        ActionMsg::ToggleRecordingAnnotation => {
            // If popup is open, close it (and enable pencil)
            if let Some(indicator) = app.recording_indicator.as_ref()
                && indicator.pencil_popup_open
            {
                log::info!("Pencil clicked while popup open - closing popup");
                return cosmic::Task::done(crate::app::Msg::Screenshot(
                    crate::capture::msg::Msg::Tool(crate::capture::msg::ToolMsg::PencilPopup(
                        crate::capture::msg::ToolPopupAction::Close,
                    )),
                ));
            }

            // Normal toggle behavior
            if let Some(indicator) = app.recording_indicator.as_mut() {
                indicator.annotation_mode = !indicator.annotation_mode;
                log::info!("Toggle annotation mode: {}", indicator.annotation_mode);
            }
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.recording_annotation_mode = !capture.ui.recording_annotation_mode;
            }
            // Return a task that sends ToggleAnnotationMode to properly
            // recreate the layer surface with correct input zone
            cosmic::Task::done(crate::app::Msg::Recording(
                crate::recording::overlay::Msg::ToggleAnnotationMode,
            ))
        }
    }
}

pub fn handle_choice_inner(app: &mut App, c: Choice) -> cosmic::Task<crate::app::Msg> {
    if let Some(capture) = app.capture.as_mut() {
        // Clear OCR/QR/arrows when rectangle changes (new selection started)
        if let Choice::Rectangle(new_r, new_s) = &c {
            if let Choice::Rectangle(old_r, _) = &capture.selection.choice {
                // If the rectangle position/size changed significantly, clear everything
                if new_r.left != old_r.left
                    || new_r.top != old_r.top
                    || new_r.right != old_r.right
                    || new_r.bottom != old_r.bottom
                {
                    capture.clear_transient_state();
                }
            }
            // Also clear if we're starting a new drag from None state
            if *new_s != DragState::None {
                capture.clear_transient_state();
            }
        }
        // Clear annotations when switching modes (Region or Output picker)
        if matches!(
            &c,
            Choice::Rectangle(_, DragState::None) | Choice::Output(None) // Only clear in picker mode, not when confirmed
        ) {
            capture.clear_annotations();
            capture.close_all_popups();
        }
        capture.selection.choice = c;
    } else {
        log::error!("No capture in progress");
        return cosmic::Task::none();
    }

    // Scan once the drag has settled. The scan handler aborts any previous one.
    let settled = app.capture.as_ref().is_some_and(|capture| {
        capture.ui.recognize_qr_codes
            && match &capture.selection.choice {
                Choice::Rectangle(r, DragState::None) => r.dimensions().is_some(),
                Choice::Output(Some(_)) | Choice::AllScreens => true,
                _ => false,
            }
    });

    if settled {
        return handle_qr_requested_inner(app, false);
    }

    // Selection cleared or still moving, so there is nothing to scan.
    if let Some(handle) = app.qr_scan.take() {
        handle.abort();
    }
    cosmic::Task::none()
}

pub fn handle_output_changed_inner(
    app: &mut App,
    wl_output: WlOutput,
) -> cosmic::Task<crate::app::Msg> {
    // In screen picker mode, cursor hover just updates focused_output_index
    // In confirmed mode, this is ignored (screen stays locked)
    if let Some(capture) = app.capture.as_mut() {
        // Find the output index
        if let Some(output_index) = app.outputs.iter().position(|o| o.output == wl_output) {
            // Mark that mouse has entered an output (for initial highlight)
            capture.selection.has_mouse_entered = true;
            // Update focused_output_index to the output where mouse entered
            capture.selection.focused_output_index = output_index;
        }
    }
    app.active_output = Some(wl_output);
    cosmic::Task::none()
}

pub fn handle_location_inner(app: &mut App, loc: usize) -> cosmic::Task<crate::app::Msg> {
    if let Some(capture) = app.capture.as_mut() {
        let loc = match loc {
            loc if loc == ImageSaveLocation::Clipboard as usize => ImageSaveLocation::Clipboard,
            loc if loc == ImageSaveLocation::Pictures as usize => ImageSaveLocation::Pictures,
            loc if loc == ImageSaveLocation::Documents as usize => ImageSaveLocation::Documents,
            _ => capture.selection.location,
        };
        capture.selection.location = loc;
    } else {
        log::error!("No capture in progress");
    }
    cosmic::Task::none()
}

// Selection and navigation messages.

/// Handle `SelectRegionMode` message
pub fn handle_select_region_mode(app: &mut App) -> cosmic::Task<AppMsg> {
    if let Some(capture) = app.capture.as_mut() {
        // Switch to rectangle selection with a fresh/default rect
        capture.selection.choice = Choice::Rectangle(Rect::default(), DragState::default());
        capture.clear_transient_state();
    }
    cosmic::Task::none()
}

/// Handle `SelectScreenMode` message
pub fn handle_select_screen_mode(app: &mut App, output_index: usize) -> cosmic::Task<AppMsg> {
    if let Some(capture) = app.capture.as_mut() {
        // Go to picker mode (None), not directly selecting a screen
        capture.selection.choice = Choice::Output(None);
        capture.selection.focused_output_index = output_index;
        // Mark that we have a valid focused output (from the button click location)
        capture.selection.has_mouse_entered = true;
        capture.clear_transient_state();
    }
    cosmic::Task::none()
}

/// Handle `NavigateLeft` message
pub fn handle_navigate_left(app: &mut App) -> cosmic::Task<AppMsg> {
    if let Some(capture) = app.capture.as_mut() {
        let output_count = app.outputs.len();
        if output_count > 0 && matches!(&capture.selection.choice, Choice::Output(None)) {
            // In screen picker mode: move to previous screen (just update index)
            capture.selection.focused_output_index = if capture.selection.focused_output_index == 0
            {
                output_count - 1
            } else {
                capture.selection.focused_output_index - 1
            };
            // Choice stays as None (picker mode)
        }
    }
    cosmic::Task::none()
}

/// Handle `NavigateRight` message
pub fn handle_navigate_right(app: &mut App) -> cosmic::Task<AppMsg> {
    if let Some(capture) = app.capture.as_mut() {
        let output_count = app.outputs.len();
        if output_count > 0 && matches!(&capture.selection.choice, Choice::Output(None)) {
            // In screen picker mode: move to next screen (just update index)
            capture.selection.focused_output_index =
                (capture.selection.focused_output_index + 1) % output_count;
            // Choice stays as None (picker mode)
        }
    }
    cosmic::Task::none()
}

/// Handle `ConfirmSelection` message
pub fn handle_confirm_selection(app: &mut App) -> cosmic::Task<AppMsg> {
    if let Some(capture) = app.capture.as_mut()
        && matches!(&capture.selection.choice, Choice::Output(None))
    {
        // Confirm the highlighted screen (enter confirmed mode)
        if let Some(output) = app.outputs.get(capture.selection.focused_output_index) {
            capture.selection.choice = Choice::Output(Some(output.name.clone()));
        }
    }
    cosmic::Task::none()
}

// Settings messages.

/// Handle `ToolbarDragStart` message
pub fn handle_toolbar_drag_start(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    capture.ui.toolbar_dragging = true;
    // Resolved on the first motion, see `handle_toolbar_drag_move`.
    capture.ui.toolbar_drag_offset = None;
    cosmic::Task::none()
}

/// Move `output`'s toolbar. Each screen has its own.
pub fn handle_toolbar_drag_move(
    capture: &mut Capture,
    output: String,
    cursor: Point,
) -> cosmic::Task<AppMsg> {
    if !capture.ui.toolbar_dragging {
        return cosmic::Task::none();
    }

    // `mouse_area` reports no press position, so the grab offset is taken on the first motion.
    let offset = if let Some(offset) = capture.ui.toolbar_drag_offset {
        offset
    } else {
        let Some(bounds) = capture.ui.toolbar_bounds else {
            return cosmic::Task::none();
        };
        let offset = cursor - Point::new(bounds.x, bounds.y);
        capture.ui.toolbar_drag_offset = Some(offset);
        offset
    };

    capture.ui.toolbar_pos.insert(output, cursor - offset);
    cosmic::Task::none()
}

/// Handle `ToolbarDragEnd` message
pub fn handle_toolbar_drag_end(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    capture.ui.toolbar_dragging = false;
    capture.ui.toolbar_drag_offset = None;
    cosmic::Task::none()
}

/// Handle `ToggleMagnifier` message
pub fn handle_toggle_magnifier(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    capture.ui.magnifier_enabled = !capture.ui.magnifier_enabled;
    Config::store("magnifier_enabled", capture.ui.magnifier_enabled);
    cosmic::Task::none()
}

/// Handle `SetSaveLocationPictures` message
pub fn handle_set_save_location_pictures(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    capture.ui.save_location_setting = SaveLocationChoice::Pictures;
    Config::store("save_location", capture.ui.save_location_setting);
    cosmic::Task::none()
}

/// Handle `SetSaveLocationDocuments` message
pub fn handle_set_save_location_documents(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    capture.ui.save_location_setting = SaveLocationChoice::Documents;
    Config::store("save_location", capture.ui.save_location_setting);
    cosmic::Task::none()
}

/// Handle `SetSaveLocationCustom` message
pub fn handle_set_save_location_custom(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    capture.ui.save_location_setting = SaveLocationChoice::Custom;
    Config::store("save_location", capture.ui.save_location_setting);
    cosmic::Task::none()
}

/// Handle `SetCustomSavePath` message
pub fn handle_set_custom_save_path(capture: &mut Capture, path: String) -> cosmic::Task<AppMsg> {
    capture.ui.custom_save_path = path.clone();
    Config::store("custom_save_path", path);
    cosmic::Task::none()
}

// Note: BrowseSaveLocation is handled specially in screenshot/mod.rs
// to support hiding/restoring the overlay when the file dialog opens.

/// Handle `SetVideoSaveLocation` message
pub fn handle_set_video_save_location(
    capture: &mut Capture,
    loc: VideoSaveLocationChoice,
) -> cosmic::Task<AppMsg> {
    capture.ui.video_save_location_setting = loc;
    Config::store("video_save_location", loc);
    cosmic::Task::none()
}

/// Handle `SetVideoCustomSavePath` message
pub fn handle_set_video_custom_save_path(
    capture: &mut Capture,
    path: String,
) -> cosmic::Task<AppMsg> {
    capture.ui.video_custom_save_path = path.clone();
    Config::store("video_custom_save_path", path);
    cosmic::Task::none()
}

// Note: BrowseVideoSaveLocation is handled specially in screenshot/mod.rs
// to support hiding/restoring the overlay when the file dialog opens.

/// Handle `SetSaveLocationClipboard` message
pub fn handle_set_save_location_clipboard(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    capture.ui.save_location_setting = SaveLocationChoice::Clipboard;
    Config::store("save_location", capture.ui.save_location_setting);
    cosmic::Task::none()
}

/// Pick an encoder. The container follows: kept if the codec can mux into it,
/// otherwise the best one that can.
pub fn handle_set_video_encoder(
    capture: &mut Capture,
    encoder: Option<String>,
) -> cosmic::Task<AppMsg> {
    use crate::recording::encoder::{Codec, container_for};

    // Automatic means the head of the detected list.
    let element = encoder.clone().or_else(|| {
        capture
            .ui
            .available_encoders
            .first()
            .map(|e| e.gst_element.clone())
    });
    let codec = element.as_deref().and_then(Codec::from_element_name);
    let container = container_for(codec, capture.ui.video_container);

    capture.ui.selected_encoder = encoder.clone();
    capture.ui.video_container = container;

    Config::store("video_encoder", encoder);
    Config::store("video_container", container);
    cosmic::Task::none()
}

/// Handle `SetVideoContainer` message
pub fn handle_set_video_container(
    capture: &mut Capture,
    container: Container,
) -> cosmic::Task<AppMsg> {
    capture.ui.video_container = container;
    Config::store("video_container", container);
    cosmic::Task::none()
}

/// Handle `SetVideoFramerate` message
pub fn handle_set_video_framerate(capture: &mut Capture, framerate: u32) -> cosmic::Task<AppMsg> {
    capture.ui.video_framerate = framerate;
    Config::store("video_framerate", framerate);
    cosmic::Task::none()
}

/// Toggle cosmic-x as the preferred Screenshot portal in `cosmic-portals.conf`,
/// then restart xdg-desktop-portal.
pub fn handle_set_as_default_portal(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    let enabling = !capture.ui.is_default_portal;

    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let config_dir = dirs::config_dir()
            .ok_or("could not determine config directory")?
            .join("xdg-desktop-portal");
        std::fs::create_dir_all(&config_dir)?;
        let conf_path = config_dir.join("cosmic-portals.conf");

        if enabling {
            let mut file = std::fs::File::create(&conf_path)?;
            file.write_all(
                b"[preferred]\ndefault=cosmic;gtk;\norg.freedesktop.impl.portal.Screenshot=cosmic-x\n",
            )?;
            log::info!("portal config written to {}", conf_path.display());
        } else {
            // Remove only the Screenshot line. If the file becomes empty / header-only, delete it
            if conf_path.exists() {
                let contents = std::fs::read_to_string(&conf_path)?;
                let filtered: String = contents
                    .lines()
                    .filter(|l| l.trim() != "org.freedesktop.impl.portal.Screenshot=cosmic-x")
                    .map(|l| format!("{l}\n"))
                    .collect();
                // If nothing meaningful remains beyond a bare [preferred] header, delete the file
                let meaningful = filtered
                    .lines()
                    .any(|l| !l.trim().is_empty() && !l.trim().starts_with('['));
                if meaningful {
                    std::fs::write(&conf_path, filtered)?;
                } else {
                    std::fs::remove_file(&conf_path)?;
                }
            }
            log::info!("removed default portal entry");
        }

        // Restart xdg-desktop-portal so the change takes effect immediately
        let status = std::process::Command::new("systemctl")
            .args(["--user", "restart", "xdg-desktop-portal"])
            .status();
        match status {
            Ok(s) if s.success() => log::info!("xdg-desktop-portal restarted"),
            Ok(s) => log::warn!("xdg-desktop-portal restart exited with {s}"),
            Err(e) => log::warn!("could not restart xdg-desktop-portal: {e}"),
        }

        Ok(())
    })();

    match result {
        Ok(()) => capture.ui.is_default_portal = enabling,
        Err(e) => log::error!("failed to toggle default portal: {e}"),
    }

    cosmic::Task::none()
}

// Annotation message handlers.

pub const MAGNIFIER_MIN_ZOOM: f32 = 1.5;
pub const MAGNIFIER_MAX_ZOOM: f32 = 10.0;

/// Minimum distance between two sampled freehand points, in logical units.
const PENCIL_MIN_STEP: f32 = 1.5;

pub fn apply_draw(capture: &mut Capture, msg: DrawMsg) {
    match msg {
        DrawMsg::Shape(kind, action) => handle_shape(capture, kind, action),
        // Text is driven at the app level, where the live editor lives.
        DrawMsg::Text(_) => {}
        DrawMsg::Pen(action) => handle_pen(capture, action),
        DrawMsg::Magnifier(action) => handle_magnifier(capture, action),
        DrawMsg::MagnifierSelect(index) => {
            capture.annotations.selected_magnifier = index;
        }
        DrawMsg::MagnifierMove(index, x, y) => {
            capture.annotations.selected_magnifier = Some(index);
            capture.annotations.edit_selected_magnifier(|m| {
                let radius = m.radius();
                m.set_geometry(Point::new(x, y), radius);
            });
        }
        DrawMsg::MagnifierResize(index, radius) => {
            capture.annotations.selected_magnifier = Some(index);
            capture.annotations.edit_selected_magnifier(|m| {
                let center = m.center();
                m.set_geometry(center, radius);
            });
        }
        DrawMsg::MagnifierSetZoom(index, zoom) => {
            capture.annotations.selected_magnifier = Some(index);
            let zoom = zoom.clamp(MAGNIFIER_MIN_ZOOM, MAGNIFIER_MAX_ZOOM);
            capture
                .annotations
                .edit_selected_magnifier(|m| m.magnification = zoom);
        }
        DrawMsg::Redact(action) => handle_redact(capture, action),
        DrawMsg::Pixelate(action) => handle_pixelate(capture, action),
        DrawMsg::ClearShapes => capture.annotations.clear_shapes(),
        DrawMsg::ClearRedactions => capture.annotations.clear_redactions(),
        DrawMsg::ToggleMoveMode => {
            capture.ui.move_mode = !capture.ui.move_mode;
            capture.annotations.clear_selection();
            if capture.ui.move_mode {
                capture.annotations.disable_all_modes();
                capture.close_all_popups();
            }
        }
        DrawMsg::SelectAnnotation(index) => {
            capture.annotations.selected = index;
        }
        DrawMsg::DeleteSelected => {
            if let Some(index) = capture.annotations.selected.take() {
                capture.annotations.stack.remove(index);
                capture.annotations.selected_magnifier = None;
            }
        }
        DrawMsg::MoveAnnotation(index, dx, dy) => {
            if let Some(op) = capture.annotations.stack.get_mut(index) {
                op.translate(dx, dy);
            }
        }
        DrawMsg::Undo => {
            capture.annotations.clear_selection();
            capture.annotations.undo();
        }
        DrawMsg::Redo => {
            capture.annotations.clear_selection();
            capture.annotations.redo();
        }
    }
}

fn handle_shape(capture: &mut Capture, kind: ShapeKind, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            if capture.annotations.shape_mode == Some(kind) {
                capture.annotations.shape_mode = None;
                capture.annotations.shape_drawing = None;
            } else {
                capture.annotations.shape_mode = Some(kind);
                disable_other_draw_modes(capture, DrawMode::Shape);
                capture.detection.clear();
            }
        }
        DrawAction::Start(x, y) => {
            if capture.annotations.shape_mode.is_some() {
                capture.annotations.shape_drawing = Some((x, y));
            }
        }
        DrawAction::Move(..) => {}
        DrawAction::End(x, y) => {
            if let Some((sx, sy)) = capture.annotations.shape_drawing.take() {
                capture.annotations.commit(Box::new(ShapeOperation::new(
                    kind,
                    Point::new(sx, sy),
                    Point::new(x, y),
                    capture.ui.shape_color.0,
                    capture.ui.shape_thickness,
                )));
            }
        }
    }
}

fn handle_pen(capture: &mut Capture, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            capture.annotations.pen_mode = !capture.annotations.pen_mode;
            if capture.annotations.pen_mode {
                disable_other_draw_modes(capture, DrawMode::Pen);
                capture.detection.clear();
            } else {
                capture.annotations.stroke_drawing = None;
            }
        }
        DrawAction::Start(x, y) => {
            if capture.annotations.pen_mode || capture.annotations.highlighter_mode {
                capture.annotations.stroke_drawing = Some(vec![(x, y)]);
            }
        }
        DrawAction::Move(x, y) => {
            if let Some(points) = capture.annotations.stroke_drawing.as_mut() {
                let far_enough = points
                    .last()
                    .is_none_or(|(lx, ly)| (x - lx).hypot(y - ly) >= PENCIL_MIN_STEP);
                if far_enough {
                    points.push((x, y));
                }
            }
        }
        DrawAction::End(x, y) => {
            let Some(mut points) = capture.annotations.stroke_drawing.take() else {
                return;
            };
            if points.last().is_none_or(|(lx, ly)| *lx != x || *ly != y) {
                points.push((x, y));
            }
            // A single click has one point and makes no stroke.
            if points.len() < 2 {
                return;
            }
            let points: Vec<Point> = points.into_iter().map(|(x, y)| Point::new(x, y)).collect();
            let color = capture.ui.shape_color.0;
            if capture.annotations.highlighter_mode {
                capture.annotations.commit(Box::new(HighlighterOperation {
                    points,
                    color,
                    width: capture.ui.highlighter_thickness,
                }));
            } else {
                capture.annotations.commit(Box::new(PenOperation {
                    points,
                    color,
                    width: capture.ui.shape_thickness,
                }));
            }
        }
    }
}

fn handle_magnifier(capture: &mut Capture, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            capture.annotations.magnifier_mode = !capture.annotations.magnifier_mode;
            if capture.annotations.magnifier_mode {
                disable_other_draw_modes(capture, DrawMode::Magnifier);
                capture.detection.clear();
            } else {
                capture.annotations.magnifier_drawing = None;
                capture.annotations.selected_magnifier = None;
            }
        }
        DrawAction::Start(x, y) => {
            if capture.annotations.magnifier_mode {
                capture.annotations.magnifier_drawing = Some((x, y));
            }
        }
        DrawAction::Move(..) => {}
        DrawAction::End(x, y) => {
            if let Some((sx, sy)) = capture.annotations.magnifier_drawing.take() {
                let loupe = MagnifierOperation::new(
                    Point::new(sx, sy),
                    Point::new(x, y),
                    capture.ui.magnifier_magnification,
                    capture.ui.shape_color.0,
                );
                // Ignore a click without a drag.
                if loupe.radius() < MIN_MAGNIFIER_RADIUS {
                    return;
                }
                capture.annotations.commit(Box::new(loupe));
                // Selected straight away, so its zoom can be adjusted.
                capture.annotations.selected_magnifier = Some(capture.annotations.stack.len() - 1);
            }
        }
    }
}

/// Smallest loupe a drag creates, in logical pixels. Anything less was a click.
const MIN_MAGNIFIER_RADIUS: f32 = 8.0;

fn handle_redact(capture: &mut Capture, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            capture.annotations.redact_mode = !capture.annotations.redact_mode;
            if capture.annotations.redact_mode {
                disable_other_draw_modes(capture, DrawMode::Redact);
                capture.detection.clear();
            } else {
                capture.annotations.redact_drawing = None;
            }
        }
        DrawAction::Start(x, y) => {
            if capture.annotations.redact_mode {
                capture.annotations.redact_drawing = Some((x, y));
            }
        }
        DrawAction::Move(..) => {}
        DrawAction::End(x, y) => {
            if let Some((sx, sy)) = capture.annotations.redact_drawing.take() {
                capture.annotations.commit(Box::new(RedactOperation::new(
                    Point::new(sx, sy),
                    Point::new(x, y),
                )));
            }
        }
    }
}

fn handle_pixelate(capture: &mut Capture, action: DrawAction) {
    match action {
        DrawAction::ModeToggle => {
            capture.annotations.pixelate_mode = !capture.annotations.pixelate_mode;
            if capture.annotations.pixelate_mode {
                disable_other_draw_modes(capture, DrawMode::Pixelate);
                capture.detection.clear();
            } else {
                capture.annotations.pixelate_drawing = None;
            }
        }
        DrawAction::Start(x, y) => {
            if capture.annotations.pixelate_mode {
                capture.annotations.pixelate_drawing = Some((x, y));
            }
        }
        DrawAction::Move(..) => {}
        DrawAction::End(x, y) => {
            if let Some((sx, sy)) = capture.annotations.pixelate_drawing.take() {
                capture.annotations.commit(Box::new(PixelateOperation::new(
                    Point::new(sx, sy),
                    Point::new(x, y),
                    capture.ui.pixelation_block_size as f32,
                )));
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DrawMode {
    Shape,
    Pen,
    Magnifier,
    Redact,
    Pixelate,
}

fn disable_other_draw_modes(capture: &mut Capture, keep: DrawMode) {
    let a = &mut capture.annotations;
    if keep != DrawMode::Shape {
        a.shape_mode = None;
        a.shape_drawing = None;
    }
    if keep != DrawMode::Pen {
        a.pen_mode = false;
        a.stroke_drawing = None;
    }
    if keep != DrawMode::Magnifier {
        a.magnifier_mode = false;
        a.magnifier_drawing = None;
        a.selected_magnifier = None;
    }
    if keep != DrawMode::Redact {
        a.redact_mode = false;
        a.redact_drawing = None;
    }
    if keep != DrawMode::Pixelate {
        a.pixelate_mode = false;
        a.pixelate_drawing = None;
    }
}

// Tool messages: popups, tool selection, colors and persistence.

/// Handle a `ToolMsg`. Returns whether config needs saving.
pub fn apply_tool(capture: &mut Capture, msg: ToolMsg) -> bool {
    match msg {
        ToolMsg::ShapeModeToggle => {
            handle_shape_mode_toggle(capture);
            false
        }
        ToolMsg::SetShapeTool(tool) => {
            set_primary_shape_tool(capture, tool);
            true // needs config save
        }
        ToolMsg::CycleShapeTool => {
            cycle_shape_tool(capture);
            true // needs config save
        }
        ToolMsg::ShapePopup(action) => {
            handle_shape_popup(capture, action);
            false
        }
        ToolMsg::AnnotateModeToggle => {
            capture.ui.annotate_mode = !capture.ui.annotate_mode;
            if capture.ui.annotate_mode {
                // Arm the tool the row shows as selected, so the highlight is not
                // claiming something is active before the first click.
                set_primary_shape_tool(capture, capture.ui.primary_shape_tool);
            } else {
                capture.leave_annotate_mode();
            }
            false
        }
        ToolMsg::DelayPopup(action) => {
            capture.ui.delay_popup_open = resolve_popup(capture.ui.delay_popup_open, action);
            if capture.ui.delay_popup_open {
                capture.ui.settings_drawer_open = false;
            }
            false
        }
        ToolMsg::FreehandPopup(action) => {
            // The slot's tool comes into hand with its dropdown, as for shapes.
            if resolve_popup(capture.ui.freehand_popup_open, action) {
                set_primary_shape_tool(capture, capture.ui.freehand_choice);
                capture.ui.freehand_popup_open = true;
            } else {
                capture.ui.freehand_popup_open = false;
            }
            false
        }
        ToolMsg::StrokePopup(action) => {
            capture.ui.stroke_popup_open = resolve_popup(capture.ui.stroke_popup_open, action);
            if capture.ui.stroke_popup_open {
                capture.ui.font_popup_open = false;
                capture.ui.shape_popup_open = false;
                capture.ui.freehand_popup_open = false;
            }
            false
        }
        ToolMsg::FontPopup(action) => {
            capture.ui.font_popup_open = resolve_popup(capture.ui.font_popup_open, action);
            if capture.ui.font_popup_open {
                capture.ui.stroke_popup_open = false;
                capture.ui.shape_popup_open = false;
            }
            false
        }
        ToolMsg::SetAnnotateColor(color) => {
            if capture.ui.move_mode && capture.annotations.selected.is_some() {
                capture.annotations.recolor_selected(color);
                return false;
            }
            capture.ui.shape_color = color;
            true // needs config save
        }
        ToolMsg::SetShapeThickness(v) => {
            // A picked annotation takes the new size itself. The defaults stay.
            if capture.ui.move_mode && capture.annotations.resize_selected(v) {
                return false;
            }
            // For pixelation the number is a block size, with its own presets and value.
            if capture.annotations.pixelate_mode {
                capture.ui.pixelation_block_size = v as u32;
            } else if capture.annotations.magnifier_mode {
                // For the magnifier the same dropdown and message carry a zoom
                // factor. The armed tool decides what the number means.
                capture.ui.magnifier_magnification = v;
            } else if capture.annotations.highlighter_mode {
                capture.ui.highlighter_thickness = v;
            } else {
                capture.ui.shape_thickness = v;
            }
            false // saved on release, not during drag
        }
        ToolMsg::SaveShapeThickness => {
            true // needs config save
        }
        ToolMsg::SetRedactTool(tool) => {
            set_primary_redact_tool(capture, tool);
            true // needs config save
        }
        ToolMsg::RedactModeToggle => {
            handle_redact_mode_toggle(capture);
            false
        }
        ToolMsg::CycleRedactTool => {
            cycle_redact_tool(capture);
            true // needs config save
        }
        ToolMsg::RedactPopup(action) => {
            handle_redact_popup(capture, action);
            false
        }
        ToolMsg::SetPixelationBlockSize(size) => {
            if capture.ui.move_mode && capture.annotations.resize_selected(size as f32) {
                return false;
            }
            capture.ui.pixelation_block_size = size;
            false // saved on release, not during drag
        }
        ToolMsg::SavePixelationBlockSize => {
            true // needs config save
        }
        ToolMsg::MagnifierModeToggle => {
            handle_magnifier_mode_toggle(capture);
            false
        }
        ToolMsg::MagnifierPopup(action) => {
            handle_magnifier_popup(capture, action);
            false
        }
        ToolMsg::SetMagnification(value) => {
            if capture.ui.move_mode && capture.annotations.resize_selected(value) {
                return false;
            }
            capture.ui.magnifier_magnification = value;
            // If a magnifier is selected, apply the zoom to it live too
            if capture.annotations.selected_magnifier.is_some() {
                capture
                    .annotations
                    .edit_selected_magnifier(|m| m.magnification = value);
            }
            false // saved on release, not during drag
        }
        ToolMsg::SaveMagnification => {
            true // needs config save
        }
        ToolMsg::PencilPopup(action) => {
            handle_pen_popup(capture, action);
            false
        }
        ToolMsg::SetPencilColor(color) => {
            capture.ui.pencil_color = color;
            true // needs config save
        }
        ToolMsg::SetPencilFadeDuration(duration) => {
            capture.ui.pencil_fade_duration = duration;
            false // saved on release, not during drag
        }
        ToolMsg::SavePencilFadeDuration => {
            true // needs config save
        }
        ToolMsg::SetPencilThickness(thickness) => {
            capture.ui.pencil_thickness = thickness;
            false // saved on release, not during drag
        }
        ToolMsg::SavePencilThickness => {
            true // needs config save
        }
        ToolMsg::ColorPicker(_) => {
            // Handled in screenshot/mod.rs, which can reach app.color_picker
            false
        }
        ToolMsg::ClearPencilDrawings => {
            // This is handled in screenshot/mod.rs because it needs access to app.recording_indicator
            false
        }
    }
}

/// Save the tool settings as one transaction.
pub fn save_tool_config(capture: &Capture) {
    let ui = &capture.ui;
    let (
        primary_shape_tool,
        shape_choice,
        freehand_choice,
        shape_color,
        shape_thickness,
        highlighter_thickness,
        primary_redact_tool,
        pixelation_block_size,
        magnifier_magnification,
        pencil_color,
        pencil_fade_duration,
        pencil_thickness,
        text_font_size,
    ) = (
        ui.primary_shape_tool,
        ui.shape_choice,
        ui.freehand_choice,
        ui.shape_color,
        ui.shape_thickness,
        ui.highlighter_thickness,
        ui.primary_redact_tool,
        ui.pixelation_block_size,
        ui.magnifier_magnification,
        ui.pencil_color,
        ui.pencil_fade_duration,
        ui.pencil_thickness,
        ui.text_font_size,
    );

    Config::store_many(move |tx| {
        use cosmic_config::ConfigSet;
        tx.set("primary_shape_tool", primary_shape_tool)?;
        tx.set("shape_choice", shape_choice)?;
        tx.set("freehand_choice", freehand_choice)?;
        tx.set("shape_color", shape_color)?;
        tx.set("shape_thickness", shape_thickness)?;
        tx.set("highlighter_thickness", highlighter_thickness)?;
        tx.set("primary_redact_tool", primary_redact_tool)?;
        tx.set("pixelation_block_size", pixelation_block_size)?;
        tx.set("magnifier_magnification", magnifier_magnification)?;
        tx.set("pencil_color", pencil_color)?;
        tx.set("pencil_fade_duration", pencil_fade_duration)?;
        tx.set("pencil_thickness", pencil_thickness)?;
        tx.set("text_font_size", text_font_size)
    });
}

fn handle_shape_mode_toggle(capture: &mut Capture) {
    // Every shape goes through one path: which one it is, is the kind.
    if let Some(kind) = capture.ui.primary_shape_tool.shape_kind() {
        if capture.annotations.shape_mode == Some(kind) {
            capture.annotations.shape_mode = None;
            capture.annotations.shape_drawing = None;
        } else {
            capture.annotations.shape_mode = Some(kind);
            disable_other_modes_except(capture, Mode::Shape);
        }
        capture.close_all_popups();
        return;
    }
    match capture.ui.primary_shape_tool {
        ShapeTool::Text => {
            capture.annotations.text_mode = !capture.annotations.text_mode;
            if capture.annotations.text_mode {
                disable_other_modes_except(capture, Mode::Text);
            }
        }
        ShapeTool::Pen => {
            capture.annotations.pen_mode = !capture.annotations.pen_mode;
            if capture.annotations.pen_mode {
                disable_other_modes_except(capture, Mode::Pen);
            } else {
                capture.annotations.stroke_drawing = None;
            }
        }
        ShapeTool::Highlighter => {
            capture.annotations.highlighter_mode = !capture.annotations.highlighter_mode;
            if capture.annotations.highlighter_mode {
                disable_other_modes_except(capture, Mode::Highlighter);
            } else {
                capture.annotations.stroke_drawing = None;
            }
        }
        ShapeTool::Pixelate => {
            capture.annotations.pixelate_mode = !capture.annotations.pixelate_mode;
            if capture.annotations.pixelate_mode {
                disable_other_modes_except(capture, Mode::Pixelate);
            } else {
                capture.annotations.pixelate_drawing = None;
            }
        }
        ShapeTool::Magnifier => {
            capture.annotations.magnifier_mode = !capture.annotations.magnifier_mode;
            if capture.annotations.magnifier_mode {
                disable_other_modes_except(capture, Mode::Magnifier);
            } else {
                capture.annotations.magnifier_drawing = None;
            }
        }
        // Handled above, where the shape kinds are.
        _ => {}
    }
    // Close popups
    capture.close_all_popups();
}

fn set_primary_shape_tool(capture: &mut Capture, tool: ShapeTool) {
    capture.ui.primary_shape_tool = tool;
    // Remember it for its slot, in this session and the next.
    if ShapeTool::SHAPES.contains(&tool) {
        capture.ui.shape_choice = tool;
    }
    if ShapeTool::FREEHAND.contains(&tool) {
        capture.ui.freehand_choice = tool;
    }
    // Arming a tool leaves move mode.
    capture.ui.move_mode = false;

    // Activate the new tool
    if let Some(kind) = tool.shape_kind() {
        capture.annotations.shape_mode = Some(kind);
        disable_other_modes_except(capture, Mode::Shape);
        capture.close_all_popups();
        return;
    }
    match tool {
        ShapeTool::Text => {
            capture.annotations.text_mode = true;
            disable_other_modes_except(capture, Mode::Text);
        }
        ShapeTool::Pen => {
            capture.annotations.pen_mode = true;
            disable_other_modes_except(capture, Mode::Pen);
        }
        ShapeTool::Highlighter => {
            capture.annotations.highlighter_mode = true;
            disable_other_modes_except(capture, Mode::Highlighter);
        }
        ShapeTool::Pixelate => {
            capture.annotations.pixelate_mode = true;
            disable_other_modes_except(capture, Mode::Pixelate);
        }
        ShapeTool::Magnifier => {
            capture.annotations.magnifier_mode = true;
            disable_other_modes_except(capture, Mode::Magnifier);
        }
        // Handled above, where the shape kinds are.
        _ => {}
    }
    capture.close_all_popups();
}

fn cycle_shape_tool(capture: &mut Capture) {
    capture.ui.primary_shape_tool = capture.ui.primary_shape_tool.next();
    set_primary_shape_tool(capture, capture.ui.primary_shape_tool);
}

/// Apply a popup action to a dropdown's open flag.
const fn resolve_popup(is_open: bool, action: ToolPopupAction) -> bool {
    match action {
        ToolPopupAction::Toggle => !is_open,
        ToolPopupAction::Open => true,
        ToolPopupAction::Close => false,
    }
}

/// Opening a tool slot's dropdown also takes that slot's tool in hand, so
/// dismissing the dropdown leaves the user ready to draw with it.
fn handle_shape_popup(capture: &mut Capture, action: ToolPopupAction) {
    if resolve_popup(capture.ui.shape_popup_open, action) {
        set_primary_shape_tool(capture, capture.ui.shape_choice);
        capture.ui.shape_popup_open = true;
    } else {
        capture.ui.shape_popup_open = false;
    }
}

fn set_primary_redact_tool(capture: &mut Capture, tool: RedactTool) {
    capture.ui.primary_redact_tool = tool;

    match tool {
        RedactTool::Redact => {
            capture.annotations.redact_mode = true;
            disable_other_modes_except(capture, Mode::Redact);
        }
        RedactTool::Pixelate => {
            capture.annotations.pixelate_mode = true;
            disable_other_modes_except(capture, Mode::Pixelate);
        }
    }
    capture.close_all_popups();
}

fn cycle_redact_tool(capture: &mut Capture) {
    capture.ui.primary_redact_tool = capture.ui.primary_redact_tool.next();
    set_primary_redact_tool(capture, capture.ui.primary_redact_tool);
}

fn handle_redact_popup(capture: &mut Capture, action: ToolPopupAction) {
    if resolve_popup(capture.ui.redact_popup_open, action) {
        set_primary_redact_tool(capture, capture.ui.primary_redact_tool);
        capture.ui.redact_popup_open = true;
    } else {
        capture.ui.redact_popup_open = false;
    }
}

fn handle_redact_mode_toggle(capture: &mut Capture) {
    match capture.ui.primary_redact_tool {
        RedactTool::Redact => {
            capture.annotations.redact_mode = !capture.annotations.redact_mode;
            if capture.annotations.redact_mode {
                disable_other_modes_except(capture, Mode::Redact);
            } else {
                capture.annotations.redact_drawing = None;
            }
        }
        RedactTool::Pixelate => {
            capture.annotations.pixelate_mode = !capture.annotations.pixelate_mode;
            if capture.annotations.pixelate_mode {
                disable_other_modes_except(capture, Mode::Pixelate);
            } else {
                capture.annotations.pixelate_drawing = None;
            }
        }
    }
    // Close popups
    capture.close_all_popups();
}

fn handle_magnifier_mode_toggle(capture: &mut Capture) {
    capture.annotations.magnifier_mode = !capture.annotations.magnifier_mode;
    if capture.annotations.magnifier_mode {
        disable_other_modes_except(capture, Mode::Magnifier);
    } else {
        capture.annotations.magnifier_drawing = None;
        capture.annotations.selected_magnifier = None;
    }
    // Close popups
    capture.close_all_popups();
}

fn handle_magnifier_popup(capture: &mut Capture, action: ToolPopupAction) {
    match action {
        ToolPopupAction::Toggle => {
            capture.ui.magnifier_popup_open = !capture.ui.magnifier_popup_open;
            if capture.ui.magnifier_popup_open {
                capture.ui.shape_popup_open = false;
                capture.ui.redact_popup_open = false;
                capture.ui.settings_drawer_open = false;
                // Show the selected magnifier's zoom in the slider, if any
                if let Some(zoom) = capture.annotations.selected_magnifier_zoom() {
                    capture.ui.magnifier_magnification = zoom;
                }
                capture.annotations.magnifier_mode = true;
                disable_other_modes_except(capture, Mode::Magnifier);
            } else {
                // Re-enable the magnifier tool when closing
                capture.annotations.magnifier_mode = true;
                disable_other_modes_except(capture, Mode::Magnifier);
            }
        }
        ToolPopupAction::Open => {
            capture.ui.magnifier_popup_open = true;
            capture.ui.shape_popup_open = false;
            capture.ui.redact_popup_open = false;
            capture.ui.settings_drawer_open = false;
            if let Some(zoom) = capture.annotations.selected_magnifier_zoom() {
                capture.ui.magnifier_magnification = zoom;
            }
            capture.annotations.magnifier_mode = true;
            disable_other_modes_except(capture, Mode::Magnifier);
        }
        ToolPopupAction::Close => {
            capture.ui.magnifier_popup_open = false;
            // Re-enable the magnifier tool when closing
            capture.annotations.magnifier_mode = true;
            disable_other_modes_except(capture, Mode::Magnifier);
        }
    }
}

const fn handle_pen_popup(capture: &mut Capture, action: ToolPopupAction) {
    match action {
        ToolPopupAction::Toggle => {
            capture.ui.pencil_popup_open = !capture.ui.pencil_popup_open;
            if capture.ui.pencil_popup_open {
                capture.ui.shape_popup_open = false;
                capture.ui.redact_popup_open = false;
                capture.ui.settings_drawer_open = false;
            }
        }
        ToolPopupAction::Open => {
            capture.ui.pencil_popup_open = true;
            capture.ui.shape_popup_open = false;
            capture.ui.redact_popup_open = false;
            capture.ui.settings_drawer_open = false;
        }
        ToolPopupAction::Close => {
            capture.ui.pencil_popup_open = false;
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Shape,
    Text,
    Pen,
    Highlighter,
    Magnifier,
    Redact,
    Pixelate,
}

fn disable_other_modes_except(capture: &mut Capture, keep: Mode) {
    if keep != Mode::Shape {
        capture.annotations.shape_mode = None;
        capture.annotations.shape_drawing = None;
    }
    if keep != Mode::Text {
        capture.annotations.text_mode = false;
    }
    // Pen and highlighter share the stroke buffer. Switching must not clear it.
    if keep == Mode::Pen || keep == Mode::Highlighter {
        capture.annotations.pen_mode = keep == Mode::Pen;
        capture.annotations.highlighter_mode = keep == Mode::Highlighter;
    } else {
        capture.annotations.pen_mode = false;
        capture.annotations.highlighter_mode = false;
        capture.annotations.stroke_drawing = None;
    }
    if keep != Mode::Magnifier {
        capture.annotations.magnifier_mode = false;
        capture.annotations.magnifier_drawing = None;
        capture.annotations.selected_magnifier = None;
    }
    if keep != Mode::Redact {
        capture.annotations.redact_mode = false;
        capture.annotations.redact_drawing = None;
    }
    if keep != Mode::Pixelate {
        capture.annotations.pixelate_mode = false;
        capture.annotations.pixelate_drawing = None;
    }
    // Clear OCR/QR when switching modes
    capture.detection.clear();
}
