mod annotations;
mod buffer;
mod capture;
mod config;
mod core;
mod domain;
mod localize;
mod render;
mod screencast;
mod screenshot;
mod session;
mod tray;
mod wayland;
mod widget;

use std::env;

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    localize::localize();

    let args: Vec<String> = env::args().collect();

    // Check for --help flag
    if args.len() > 1 && (args[1] == "--help" || args[1] == "-h") {
        println!("SnapPea - Screenshot and Screen Recording for Linux/Wayland");
        println!();
        println!("Usage: snappea [OPTION]");
        println!();
        println!("Options:");
        println!("  --portal        Run as D-Bus portal service (for desktop integration)");
        println!("  --record        Start screen recording (requires additional arguments)");
        println!("  --help, -h      Show this help message");
        println!();
        println!("When run without arguments, opens the screenshot UI directly.");
        println!("If an instance is already running, sends command to it.");
        println!("Use --portal for D-Bus portal mode (system integration).");
        return Ok(());
    }

    // Check for --portal flag (D-Bus portal mode)
    // Portal mode always starts fresh (it's the background service)
    if args.len() > 1 && args[1] == "--portal" {
        return core::app::run();
    }

    // Check for --record subcommand
    if args.len() > 1 && args[1] == "--record" {
        // Parse arguments
        let mut output_file = None;
        let mut output_name = None;
        let mut region = None;
        let mut logical_size = None;
        let mut encoder = None;
        let mut container = config::Container::Mp4;
        let mut framerate = 60;
        let mut toplevel_index: Option<usize> = None;
        let mut show_cursor = false;

        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--output" => {
                    if i + 1 < args.len() {
                        output_file = Some(std::path::PathBuf::from(&args[i + 1]));
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--output-name" => {
                    if i + 1 < args.len() {
                        output_name = Some(args[i + 1].clone());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--region" => {
                    if i + 1 < args.len() {
                        let parts: Vec<&str> = args[i + 1].split(',').collect();
                        if parts.len() == 4 {
                            if let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                                parts[0].parse::<i32>(),
                                parts[1].parse::<i32>(),
                                parts[2].parse::<u32>(),
                                parts[3].parse::<u32>(),
                            ) {
                                region = Some((x, y, w, h));
                            }
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--logical-size" => {
                    if i + 1 < args.len() {
                        let parts: Vec<&str> = args[i + 1].split(',').collect();
                        if parts.len() == 2 {
                            if let (Ok(w), Ok(h)) =
                                (parts[0].parse::<u32>(), parts[1].parse::<u32>())
                            {
                                logical_size = Some((w, h));
                            }
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--encoder" => {
                    if i + 1 < args.len() {
                        encoder = Some(args[i + 1].clone());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--container" => {
                    if i + 1 < args.len() {
                        container = match args[i + 1].as_str() {
                            "Mp4" => config::Container::Mp4,
                            "Webm" => config::Container::Webm,
                            "Mkv" => config::Container::Mkv,
                            _ => config::Container::Mp4,
                        };
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--framerate" => {
                    if i + 1 < args.len() {
                        framerate = args[i + 1].parse().unwrap_or(60);
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--toplevel-index" => {
                    if i + 1 < args.len() {
                        toplevel_index = args[i + 1].parse().ok();
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--show-cursor" => {
                    show_cursor = true;
                    i += 1;
                }
                _ => i += 1,
            }
        }

        // Run recording
        if let (
            Some(output_file),
            Some(output_name),
            Some(region),
            Some(logical_size),
            Some(encoder),
        ) = (output_file, output_name, region, logical_size, encoder)
        {
            // Save state before starting
            let state = screencast::RecordingState {
                output_file: output_file.clone(),
                region,
                output_name: output_name.clone(),
                started_at: chrono::Utc::now().to_rfc3339(),
            };
            if let Err(e) = state.save() {
                log::error!("Failed to save recording state: {}", e);
            }

            if let Err(e) = screencast::start_recording(
                output_file,
                output_name,
                region,
                logical_size,
                encoder,
                container,
                framerate,
                toplevel_index,
                show_cursor,
            ) {
                log::error!("Recording failed: {}", e);
                let _ = screencast::RecordingState::delete();
                std::process::exit(1);
            }

            // Clean up state file after successful recording
            if let Err(e) = screencast::RecordingState::delete() {
                log::warn!("Failed to delete state file after recording: {}", e);
            }
        } else {
            log::error!("{}", fl!("cli-missing-args"));
            log::error!("{}", fl!("cli-usage"));
            std::process::exit(1);
        }

        return Ok(());
    }

    // Check if another instance is already running
    // If so, send a command to it instead of starting a new instance
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    let instance_running = rt.block_on(core::control::is_instance_running());

    if instance_running {
        log::info!("Another instance is running, sending screenshot command");

        // If recording, stop it; otherwise take a screenshot
        let is_recording = rt.block_on(async {
            let Ok(conn) = zbus::Connection::session().await else {
                return false;
            };
            conn.call_method(
                Some(core::portal::DBUS_NAME),
                core::control::CONTROL_PATH,
                Some("io.github.hojjatabdollahi.snappea.Control"),
                "IsRecording",
                &(),
            )
            .await
            .ok()
            .and_then(|r| r.body().deserialize::<bool>().ok())
            .unwrap_or(false)
        });

        let command = if is_recording {
            "toggle-recording"
        } else {
            "screenshot"
        };

        match rt.block_on(core::control::send_command(command)) {
            Ok(true) => {
                log::info!("Command sent successfully");
                return Ok(());
            }
            Ok(false) => {
                log::warn!("Command was not processed");
                return Ok(());
            }
            Err(e) => {
                log::warn!("Failed to send command: {}, starting new instance", e);
                // Fall through to start new instance
            }
        }
    }

    // Default: Direct screenshot mode (no D-Bus portal)
    core::app::run_with_flags(core::app::AppFlags {
        direct_screenshot: true,
    })
}
