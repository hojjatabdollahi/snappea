// SPDX-License-Identifier: GPL-3.0-only

//! Message handling for a capture in progress.

use super::detect::handle_ocr_msg;
use super::detect::handle_qr_msg;
use super::detect::open_url_and_close;
use super::detect::request_qr_scan;
use super::flow::cancel;
use super::flow::save;
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
use crate::config::RedactTool;
use crate::config::SaveLocationChoice;
use crate::config::ShapeTool;
use crate::config::VideoSaveLocationChoice;
use crate::dbus::PortalResponse;
use crate::fl;
use crate::geometry::Choice;
use crate::geometry::DragState;
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
            | ToolMsg::ShapePopup(ToolPopupAction::Toggle)
            | ToolMsg::FreehandPopup(ToolPopupAction::Toggle),
        ) => true,
        // Changing what is being captured moves the ground under the label.
        Msg::Select(_) => true,
        // Acting on the annotation list as a whole.
        Msg::Draw(DrawMsg::Undo | DrawMsg::Redo | DrawMsg::ToggleMoveMode) => true,
        // Everything else leaves it open.
        _ => false,
    }
}

pub fn update_msg(app: &mut App, msg: Msg) -> cosmic::Task<crate::app::Msg> {
    if app.text.is_some() && ends_text_edit(&msg) {
        crate::capture::text::commit(app);
    }

    match msg {
        Msg::Draw(draw_msg) => handle_draw_msg(app, draw_msg),

        Msg::Tool(crate::capture::msg::ToolMsg::ColorPicker(update)) => {
            handle_color_picker(app, update)
        }
        Msg::Tool(tool_msg) => handle_tool_msg(app, tool_msg),

        Msg::Select(select_msg) => handle_select_msg(app, select_msg),

        Msg::Settings(settings_msg) => handle_settings_msg(app, settings_msg),

        Msg::Detect(detect_msg) => handle_detect_msg(app, detect_msg),

        Msg::Action(capture_msg) => handle_capture_msg(app, capture_msg),
    }
}

/// Handle Draw messages (annotation drawing)
pub fn handle_draw_msg(app: &mut App, msg: DrawMsg) -> cosmic::Task<crate::app::Msg> {
    // Text needs the live editor on the app, so it is handled before delegating.
    if let DrawMsg::Text(action) = msg {
        if let Some(capture) = app.capture.as_mut() {
            capture.ui.exit_armed = false;
        }
        return crate::capture::text::handle(app, action);
    }

    if let Some(capture) = app.capture.as_mut() {
        // Further annotation work makes a pending exit confirmation stale.
        capture.ui.exit_armed = false;
        apply_draw(capture, msg);
    }
    cosmic::Task::none()
}

/// Handle Tool messages (popup and tool configuration)
pub fn handle_tool_msg(app: &mut App, msg: ToolMsg) -> cosmic::Task<crate::app::Msg> {
    // The pencil settings also apply to the recording in progress.
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
        SelectMsg::Navigate(dir) => navigate(app, dir),
        SelectMsg::Confirm => handle_confirm_selection(app),
    }
}

/// Handle Settings messages (UI and config)
pub fn handle_settings_msg(app: &mut App, msg: SettingsMsg) -> cosmic::Task<crate::app::Msg> {
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
            capture.ui.settings_drawer_open = !capture.ui.settings_drawer_open;
            // Opening the settings menu puts the tools down and closes
            // anything else that was up.
            if capture.ui.settings_drawer_open {
                capture.ui.shape_popup_open = false;
                capture.disable_all_modes();
            }
            cosmic::Task::none()
        }
        SettingsMsg::ToggleMagnifier => {
            capture.ui.magnifier_enabled = !capture.ui.magnifier_enabled;
            Config::store("magnifier_enabled", capture.ui.magnifier_enabled);
            cosmic::Task::none()
        }
        SettingsMsg::SetSaveLocation(location) => {
            capture.ui.save_location_setting = location;
            Config::store("save_location", location);
            cosmic::Task::none()
        }
        SettingsMsg::SetVideoSaveLocation(location) => {
            capture.ui.video_save_location_setting = location;
            Config::store("video_save_location", location);
            cosmic::Task::none()
        }
        // The folder pickers take the overlay down while the dialog is up.
        SettingsMsg::BrowseSaveLocation => browse_folder(
            &app.outputs,
            fl!("browse-screenshots-title"),
            Msg::browse_save_location_result,
        ),
        SettingsMsg::BrowseVideoSaveLocation => browse_folder(
            &app.outputs,
            fl!("browse-videos-title"),
            Msg::browse_video_save_location_result,
        ),
        // Choosing a folder also selects it. A cancelled dialog changes nothing.
        SettingsMsg::BrowseSaveLocationResult(path) => {
            if let Some(path) = path {
                capture.ui.custom_save_path = path.clone();
                capture.ui.save_location_setting = SaveLocationChoice::Custom;
                Config::store("custom_save_path", path);
                Config::store("save_location", SaveLocationChoice::Custom);
            }
            app.open_overlay()
        }
        SettingsMsg::BrowseVideoSaveLocationResult(path) => {
            if let Some(path) = path {
                capture.ui.video_custom_save_path = path.clone();
                capture.ui.video_save_location_setting = VideoSaveLocationChoice::Custom;
                Config::store("video_custom_save_path", path);
                Config::store("video_save_location", VideoSaveLocationChoice::Custom);
            }
            app.open_overlay()
        }
        SettingsMsg::SetCaptureDelay(secs) => {
            capture.ui.capture_delay_secs = secs;
            capture.ui.delay_popup_open = false;
            Config::store("capture_delay_secs", secs);
            cosmic::Task::none()
        }
        SettingsMsg::ToolbarBounds(bounds) => {
            capture.ui.toolbar_bounds = Some(bounds);
            cosmic::Task::none()
        }
        SettingsMsg::SetVideoEncoder(encoder) => handle_set_video_encoder(capture, encoder),
        SettingsMsg::SetVideoContainer(container) => {
            capture.ui.video_container = container;
            Config::store("video_container", container);
            cosmic::Task::none()
        }
        SettingsMsg::SetVideoFramerate(framerate) => {
            capture.ui.video_framerate = framerate;
            Config::store("video_framerate", framerate);
            cosmic::Task::none()
        }
        SettingsMsg::ToggleShowCursor => {
            capture.ui.video_show_cursor = !capture.ui.video_show_cursor;
            crate::config::Config::store("video_show_cursor", capture.ui.video_show_cursor);
            cosmic::Task::none()
        }
        SettingsMsg::ToggleRegionBorder => {
            capture.ui.video_region_border = !capture.ui.video_region_border;
            crate::config::Config::store("video_region_border", capture.ui.video_region_border);
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

/// Take the overlay down and open a folder picker. `done` carries the pick back.
fn browse_folder(
    outputs: &[crate::app::OutputState],
    title: String,
    done: fn(Option<String>) -> Msg,
) -> cosmic::Task<crate::app::Msg> {
    let destroy = cosmic::Task::batch(outputs.iter().map(|o| destroy_layer_surface(o.id)));
    let dialog = cosmic::Task::perform(
        async move {
            rfd::AsyncFileDialog::new()
                .set_title(title)
                .pick_folder()
                .await
                .map(|handle| handle.path().to_string_lossy().to_string())
        },
        move |result| crate::app::Msg::Screenshot(done(result)),
    );
    destroy.chain(dialog)
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
        ActionMsg::Capture => save(app),
        ActionMsg::Cancel => cancel(app),
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
                cancel(app)
            }
        }
        ActionMsg::SaveToPictures => {
            if let Some(capture) = app.capture.as_mut() {
                capture.selection.location = capture.ui.save_location_setting;
                // A folder saves a file and copies to the clipboard. Only the clipboard row skips the file.
                capture.selection.also_copy_to_clipboard =
                    capture.selection.location != SaveLocationChoice::Clipboard;
            }
            save(app)
        }
        ActionMsg::DelayedCapture => {
            let Some(capture) = app.capture.as_ref() else {
                return cosmic::Task::none();
            };
            let delay = capture.ui.capture_delay_secs.max(1);

            // The overlay stays up because it is capture-excluded. It stops drawing the selection
            // UI and shows the countdown toolbar on every output, where the capture toolbar was.
            let toolbar_pos = capture.ui.toolbar_pos.clone();
            // The capture toolbar's size, until the countdown toolbar reports its own.
            let size = capture
                .ui
                .toolbar_bounds
                .map_or((0.0, 0.0), |b| (b.width, b.height));
            let windows: Vec<_> = app
                .outputs
                .iter()
                .map(|output| crate::capture::countdown::CountdownWindow {
                    // The output's own overlay surface, taking a new job.
                    id: output.id,
                    output_name: output.name.clone(),
                    output_size: (output.logical_size.0 as f32, output.logical_size.1 as f32),
                    pos: toolbar_pos.get(&output.name).map(|p| (p.x, p.y)),
                    size,
                })
                .collect();

            let countdown = crate::capture::countdown::Countdown::new(
                delay,
                windows,
                capture.ui.toolbar_anim.progress(capture.ui.now),
                capture.selection.choice.clone(),
            );
            let tasks = [
                crate::app::App::enter_countdown(&countdown),
                crate::app::countdown_fire(&countdown),
                crate::app::countdown_refresh(&countdown),
            ];
            app.countdown = Some(countdown);

            cosmic::Task::batch(tasks)
        }
        ActionMsg::RecordRegion => start_recording(app),
        ActionMsg::Choice(c) => set_choice(app, c),
        ActionMsg::OutputChanged(wl_output) => focus_output(app, wl_output),
        ActionMsg::OpenUrl(url) => open_url_and_close(app, url),
        ActionMsg::ToggleCaptureMode(is_video) => {
            log::info!("Capture mode toggled: is_video_mode = {is_video}");
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.is_video_mode = is_video;
                capture.close_all_popups();
                capture.disable_all_modes();
                capture.clear_annotations();
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
        ActionMsg::StopRecording => {
            log::info!("Stop recording requested");
            if let Err(e) = crate::recording::stop_recording() {
                log::error!("Failed to stop recording: {e}");
            }

            let indicator_task = if let Some(mut indicator) = app.recording_indicator.take() {
                indicator.destroy_surfaces(&app.outputs)
            } else {
                cosmic::Task::none()
            };

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
            if let Some(capture) = app.capture.as_mut() {
                capture.ui.recording_annotation_mode = !capture.ui.recording_annotation_mode;
            }
            match app.recording_indicator.as_mut() {
                Some(indicator) => {
                    indicator.annotation_mode = !indicator.annotation_mode;
                    log::info!("Toggle annotation mode: {}", indicator.annotation_mode);
                    // The annotation surface takes input only while the pencil is active.
                    indicator.apply_state()
                }
                None => cosmic::Task::none(),
            }
        }
    }
}

// Recording.

/// Start recording the selection: resolve it to one output's region, pick the
/// file, spawn the recorder thread and hand the overlay to the recording chrome.
fn start_recording(app: &mut App) -> cosmic::Task<AppMsg> {
    let Some(capture) = app.capture.as_ref() else {
        log::warn!("Record clicked but no screenshot capture available");
        return cosmic::Task::none();
    };
    let region = match &capture.selection.choice {
        Choice::Rectangle(rect, _) if rect.dimensions().is_some() => *rect,
        Choice::Output(Some(name)) => match app.outputs.iter().find(|o| &o.name == name) {
            Some(output) => output.rect(),
            None => {
                log::warn!("Record clicked but output not found: {name}");
                return cosmic::Task::none();
            }
        },
        _ => {
            log::warn!("Record clicked but no valid region selected");
            return cosmic::Task::none();
        }
    };
    super::flow::remember_capture(crate::config::CaptureMode::Video, &capture.selection.choice);

    // The output most of the region lies on. A recording spans one screen, so
    // the region is clamped to it, in output-local logical coordinates. The
    // recorder scales to physical pixels itself.
    let Some((output, visible)) = app
        .outputs
        .iter()
        .filter_map(|output| Some((output, region.intersect(output.rect())?)))
        .max_by_key(|(_, visible)| i64::from(visible.width()) * i64::from(visible.height()))
    else {
        log::warn!("Record clicked but the selection overlaps no output");
        return cosmic::Task::none();
    };
    let output_rect = output.rect();
    let local_region = (
        visible.left - output_rect.left,
        visible.top - output_rect.top,
        visible.width() as u32,
        visible.height() as u32,
    );
    let output_name = output.name.clone();
    log::info!("Recording {local_region:?} on output '{output_name}'");

    let config = Config::load();
    // WebM is no longer a recording format (realtime software VP9 is slow and
    // blocky, so it is only an editor export). Coerce any stale saved value.
    let mut container = match config.video_container {
        crate::config::Container::Webm => crate::config::Container::Mp4,
        other => other,
    };
    let Some(output_dir) = recording_dir(&config) else {
        return cosmic::Task::none();
    };
    let encoder = config
        .video_encoder
        .clone()
        .or_else(|| crate::recording::best_encoder().ok().map(|e| e.gst_element))
        .unwrap_or_else(|| "x264enc".to_string());
    // Coerce the container to one the codec can mux into (VP9 in MP4 writes nothing).
    if let Some(codec) = crate::recording::encoder::detect_encoders()
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
            "Codec {} cannot be muxed into {container:?}; using {fallback:?} instead",
            codec.name()
        );
        container = fallback;
    }
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let output_file = output_dir.join(format!("recording-{timestamp}.{}", container.extension()));

    let Some(wl_output) = app.wayland_helper.outputs().into_iter().find(|o| {
        app.wayland_helper
            .output_info(o)
            .and_then(|info| info.name)
            .as_deref()
            == Some(&output_name)
    }) else {
        log::error!("Output '{output_name}' not found in wayland_helper");
        crate::dbus::notify_error(
            "Recording failed",
            format!("Display output '{output_name}' could not be found."),
        );
        return cosmic::Task::none();
    };

    let wayland_helper = app.wayland_helper.clone();
    let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_handle = {
        let stop_flag = stop_flag.clone();
        let output_file = output_file.clone();
        let show_cursor = capture.ui.video_show_cursor;
        let output_logical_size = output.logical_size;
        std::thread::spawn(move || {
            let result = crate::recording::record(
                wayland_helper,
                CaptureSource::Output(wl_output),
                output_file,
                local_region,
                output_logical_size,
                encoder,
                container,
                config.video_framerate,
                show_cursor,
                &stop_flag,
            );
            // The UI already shows "recording", so surface a failure as a notification.
            if let Err(ref e) = result {
                log::error!("Recording failed: {e:#}");
                crate::dbus::notify_error("Recording failed", format!("{e:#}"));
            }
            result
        })
    };
    crate::recording::set_recording(crate::recording::RecordingHandle::new(
        stop_flag,
        thread_handle,
        output_file.clone(),
    ));
    log::info!("Recording started: {}", output_file.display());

    // Reuse this output's session overlay for the recording chrome. Only live
    // annotations need a new surface, on the captured layer below.
    let indicator = RecordingIndicator {
        chrome_window_id: output.id,
        chrome_surface_alive: true,
        video_live_annotation_window_id: window::Id::unique(),
        annotation_surface_alive: false,
        output: output.output.clone(),
        output_size: (output.logical_size.0 as f32, output.logical_size.1 as f32),
        region: local_region,
        region_border: capture.ui.video_region_border,
        blink_visible: capture.ui.video_region_border,
        annotations: Vec::new(),
        current_stroke: None,
        pointer: None,
        annotation_mode: false,
        pencil_color: config.pencil_color,
        pencil_fade_duration: config.pencil_fade_duration,
        pencil_thickness: config.pencil_thickness,
        // Where the capture toolbar was, at its size until the recording toolbar reports its own.
        toolbar_pos: capture
            .ui
            .toolbar_pos
            .get(&output_name)
            .map(|position| (position.x, position.y)),
        toolbar_size: capture
            .ui
            .toolbar_bounds
            .map_or((0.0, 0.0), |b| (b.width, b.height)),
        outgoing: capture.ui.toolbar_anim.progress(capture.ui.now),
        toolbar_transition_started: Instant::now(),
        recording_started: Instant::now(),
        toolbar_transition_finished: false,
        outgoing_choice: capture.selection.choice.clone(),
        toolbar_dragging: false,
        drag_offset: None,
        fade_popup_open: false,
        thickness_popup_open: false,
    };

    if let Some(capture) = app.capture.as_mut() {
        capture.ui.is_recording = true;
        capture.ui.recording_annotation_mode = false;
        capture.close_all_popups();
    }
    let indicator = app.recording_indicator.insert(indicator);
    let annotation_task = indicator.create_annotation_surface();
    cosmic::Task::batch([app.enter_recording(), annotation_task])
}

/// The folder recordings go in, created if missing: the custom folder when
/// set, else ~/Videos.
fn recording_dir(config: &Config) -> Option<std::path::PathBuf> {
    let dir = match config.video_save_location {
        VideoSaveLocationChoice::Custom if !config.video_custom_save_path.is_empty() => {
            std::path::PathBuf::from(&config.video_custom_save_path)
        }
        _ => dirs::video_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join("Videos")))
            .unwrap_or_else(|| std::path::PathBuf::from(".")),
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::error!("Failed to create output directory '{}': {e}", dir.display());
        crate::dbus::notify_error(
            "Recording failed",
            format!("Could not create output directory '{}': {e}", dir.display()),
        );
        return None;
    }
    Some(dir)
}

pub fn set_choice(app: &mut App, c: Choice) -> cosmic::Task<crate::app::Msg> {
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
        return request_qr_scan(app, false);
    }

    // Selection cleared or still moving, so there is nothing to scan.
    if let Some(handle) = app.qr_scan.take() {
        handle.abort();
    }
    cosmic::Task::none()
}

pub fn focus_output(app: &mut App, wl_output: WlOutput) -> cosmic::Task<crate::app::Msg> {
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
    cosmic::Task::none()
}

// Selection and navigation messages.

/// Switch to a fresh region selection.
pub fn handle_select_region_mode(app: &mut App) -> cosmic::Task<AppMsg> {
    if let Some(capture) = app.capture.as_mut() {
        // Switch to rectangle selection with a fresh/default rect
        capture.selection.choice = Choice::Rectangle(Rect::default(), DragState::default());
        capture.clear_transient_state();
    }
    cosmic::Task::none()
}

/// Open the screen picker, focused on the clicked output.
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

/// Move the screen picker's focus one output over, wrapping around.
fn navigate(app: &mut App, dir: Direction) -> cosmic::Task<AppMsg> {
    let count = app.outputs.len();
    if let Some(capture) = app.capture.as_mut()
        && count > 0
        && matches!(&capture.selection.choice, Choice::Output(None))
    {
        let index = &mut capture.selection.focused_output_index;
        *index = match dir {
            Direction::Left => (*index + count - 1) % count,
            Direction::Right => (*index + 1) % count,
        };
    }
    cosmic::Task::none()
}

/// Confirm the highlighted screen in the picker.
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

/// Start dragging the toolbar.
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

pub fn handle_toolbar_drag_end(capture: &mut Capture) -> cosmic::Task<AppMsg> {
    capture.ui.toolbar_dragging = false;
    capture.ui.toolbar_drag_offset = None;
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
                b"[preferred]\ndefault=cosmic;gtk;\norg.freedesktop.impl.portal.Screenshot=snappea\n",
            )?;
            log::info!("portal config written to {}", conf_path.display());
        } else {
            // Remove only the Screenshot line. If the file becomes empty / header-only, delete it
            if conf_path.exists() {
                let contents = std::fs::read_to_string(&conf_path)?;
                let filtered: String = contents
                    .lines()
                    .filter(|l| l.trim() != "org.freedesktop.impl.portal.Screenshot=snappea")
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
                // factor. The selected tool decides what the number means.
                capture.ui.magnifier_magnification = v;
            } else if capture.annotations.highlighter_mode {
                capture.ui.highlighter_thickness = v;
            } else {
                capture.ui.shape_thickness = v;
            }
            // A dropdown preset, so each pick is final.
            true
        }
        ToolMsg::RedactModeToggle => {
            handle_redact_mode_toggle(capture);
            false
        }
        ToolMsg::CycleRedactTool => {
            cycle_redact_tool(capture);
            true // needs config save
        }
        // The pencil settings come from dropdown presets, so each pick is final.
        ToolMsg::SetPencilColor(color) => {
            capture.ui.pencil_color = color;
            true
        }
        ToolMsg::SetPencilFadeDuration(duration) => {
            capture.ui.pencil_fade_duration = duration;
            true
        }
        ToolMsg::SetPencilThickness(thickness) => {
            capture.ui.pencil_thickness = thickness;
            true
        }
        // Handled by `handle_color_picker`, which can reach the picker model.
        ToolMsg::ColorPicker(_) => false,
    }
}

/// Save the tool settings as one transaction.
pub fn save_tool_config(capture: &Capture) {
    let ui = capture.ui.clone();
    Config::store_many(move |tx| {
        use cosmic_config::ConfigSet;
        tx.set("primary_shape_tool", ui.primary_shape_tool)?;
        tx.set("shape_choice", ui.shape_choice)?;
        tx.set("freehand_choice", ui.freehand_choice)?;
        tx.set("shape_color", ui.shape_color)?;
        tx.set("shape_thickness", ui.shape_thickness)?;
        tx.set("highlighter_thickness", ui.highlighter_thickness)?;
        tx.set("primary_redact_tool", ui.primary_redact_tool)?;
        tx.set("pixelation_block_size", ui.pixelation_block_size)?;
        tx.set("magnifier_magnification", ui.magnifier_magnification)?;
        tx.set("pencil_color", ui.pencil_color)?;
        tx.set("pencil_fade_duration", ui.pencil_fade_duration)?;
        tx.set("pencil_thickness", ui.pencil_thickness)?;
        tx.set("text_font_size", ui.text_font_size)
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
    capture.close_all_popups();
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
