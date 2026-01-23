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
mod wayland;
mod widget;

use std::env;

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    localize::localize();

    // Check for --record subcommand
    let args: Vec<String> = env::args().collect();
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
                            if let (Ok(w), Ok(h)) = (
                                parts[0].parse::<u32>(),
                                parts[1].parse::<u32>(),
                            ) {
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
        if let (Some(output_file), Some(output_name), Some(region), Some(logical_size), Some(encoder)) =
            (output_file, output_name, region, logical_size, encoder)
        {
            // Save state before starting
            let state = screencast::RecordingState {
                pid: std::process::id(),
                output_file: output_file.clone(),
                region,
                output_name: output_name.clone(),
                started_at: chrono::Local::now().to_rfc3339(),
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
            log::error!("Missing required arguments for --record");
            log::error!(
                "Usage: snappea --record --output FILE --output-name NAME --region X,Y,W,H --logical-size W,H --encoder ENC [--container FMT] [--framerate FPS] [--toplevel-index IDX]"
            );
            std::process::exit(1);
        }

        return Ok(());
    }

    // Normal UI mode
    core::app::run()
}
