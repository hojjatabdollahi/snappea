// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod colors;
mod export;
mod localize;
mod widgets;

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    localize::localize();

    let args: Vec<String> = std::env::args().collect();
    let can_discard = args.iter().any(|a| a == "--discard");
    let Some(arg) = args.iter().skip(1).find(|a| !a.starts_with("--")) else {
        eprintln!("Usage: cosmic-x-trim [--discard] <path-to-video>");
        std::process::exit(1);
    };

    // Canonicalize so a relative command-line path still becomes a valid file
    // URI for GStreamer, and so the title bar shows where the file is.
    let path = PathBuf::from(arg)
        .canonicalize()
        .map_err(|e| format!("{arg}: {e}"))?;

    if !export::ffmpeg_available() {
        eprintln!("cosmic-x-trim needs ffmpeg on PATH");
        std::process::exit(1);
    }

    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(700.0, 500.0))
        .debug(false);

    cosmic::app::run::<app::Editor>(settings, app::Flags { path, can_discard })?;
    Ok(())
}
