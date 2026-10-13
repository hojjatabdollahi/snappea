// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod capture;
mod config;
mod dbus;
mod geometry;
mod localize;
mod recording;
mod wayland;
mod widget;

use std::env;

const USAGE: &str = "SnapPea - Screenshot and Screen Recording for Linux/Wayland

Usage: snappea [OPTION]

Options:
  --portal        Run as D-Bus portal service (for desktop integration)
  --record        Start screen recording (requires additional arguments)
  --version, -V   Show version information
  --help, -h      Show this help message

When run without arguments, opens the screenshot UI directly.
If an instance is already running, sends command to it.
Use --portal for D-Bus portal mode (system integration).";

const RECORD_USAGE: &str = "Usage: snappea --record --output FILE --output-name NAME \
--region X,Y,W,H --logical-size W,H --encoder ENC [--container FMT] [--framerate FPS] \
[--toplevel-index IDX] [--show-cursor]";

/// `N` comma-separated numbers.
fn numbers<T: std::str::FromStr, const N: usize>(list: &str) -> Option<[T; N]> {
    let parsed: Vec<T> = list
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    parsed.try_into().ok()
}

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    localize::localize();

    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return Ok(());
        }
        Some("--version" | "-V") => {
            println!(
                "{} {} ({})",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                env!("GIT_HASH")
            );
            return Ok(());
        }
        // Portal mode always starts fresh (it's the background service)
        Some("--portal") => return app::run(),
        Some("--record") => return record(&args[2..]),
        _ => {}
    }

    // Check if another instance is already running
    // If so, send a command to it instead of starting a new instance
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    let instance_running = rt.block_on(dbus::is_instance_running());

    if instance_running {
        log::info!("Another instance is running, sending screenshot command");

        // If recording, stop it. Otherwise take a screenshot
        let is_recording = rt.block_on(async {
            let Ok(conn) = zbus::Connection::session().await else {
                return false;
            };
            conn.call_method(
                Some(dbus::DBUS_NAME),
                dbus::CONTROL_PATH,
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
            dbus::ControlCommand::ToggleRecording
        } else {
            dbus::ControlCommand::TakeScreenshot
        };

        match rt.block_on(dbus::send_command(command)) {
            Ok(true) => {
                log::info!("Command sent successfully");
                return Ok(());
            }
            Ok(false) => {
                log::warn!("Command was not processed");
                return Ok(());
            }
            Err(e) => {
                log::warn!("Failed to send command: {e}, starting new instance");
                // Fall through to start new instance
            }
        }
    }

    // Default: Direct screenshot mode (no D-Bus portal)
    app::run_with_flags(app::AppFlags {
        direct_screenshot: true,
    })
}

/// The `--record` subcommand: record from the command line until SIGTERM.
fn record(args: &[String]) -> cosmic::iced::Result {
    let mut output_file = None;
    let mut output_name = None;
    let mut region = None;
    let mut logical_size = None;
    let mut encoder = None;
    let mut container = config::Container::Mp4;
    let mut framerate = 60;
    let mut toplevel_index: Option<usize> = None;
    let mut show_cursor = false;

    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--show-cursor" => show_cursor = true,
            "--output" => output_file = rest.next().map(std::path::PathBuf::from),
            "--output-name" => output_name = rest.next().cloned(),
            "--region" => {
                region = rest
                    .next()
                    .and_then(|v| numbers::<i64, 4>(v))
                    .and_then(|[x, y, w, h]| {
                        Some((
                            i32::try_from(x).ok()?,
                            i32::try_from(y).ok()?,
                            u32::try_from(w).ok()?,
                            u32::try_from(h).ok()?,
                        ))
                    });
            }
            "--logical-size" => {
                logical_size = rest
                    .next()
                    .and_then(|v| numbers::<u32, 2>(v))
                    .map(|[w, h]| (w, h));
            }
            "--encoder" => encoder = rest.next().cloned(),
            "--container" => {
                container = match rest.next().map(String::as_str) {
                    Some("Webm") => config::Container::Webm,
                    Some("Mkv") => config::Container::Mkv,
                    _ => config::Container::Mp4,
                };
            }
            "--framerate" => framerate = rest.next().and_then(|v| v.parse().ok()).unwrap_or(60),
            "--toplevel-index" => toplevel_index = rest.next().and_then(|v| v.parse().ok()),
            _ => {}
        }
    }

    let (Some(output_file), Some(output_name), Some(region), Some(logical_size), Some(encoder)) =
        (output_file, output_name, region, logical_size, encoder)
    else {
        eprintln!("Missing required arguments for --record\n{RECORD_USAGE}");
        std::process::exit(1);
    };

    if let Err(e) = recording::start_recording(
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
        log::error!("Recording failed: {e}");
        std::process::exit(1);
    }

    Ok(())
}
