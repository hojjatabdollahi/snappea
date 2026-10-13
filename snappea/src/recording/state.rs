// SPDX-License-Identifier: GPL-3.0-only

//! The active recording and its control handle.

use crate::fl;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Global recording handle: only one recording can be active at a time
static RECORDING_HANDLE: Mutex<Option<RecordingHandle>> = Mutex::new(None);

/// Handle to control an active recording
pub struct RecordingHandle {
    /// Flag to signal the recording thread to stop
    pub stop_flag: Arc<AtomicBool>,
    /// Thread handle for joining
    thread_handle: Option<JoinHandle<Result<()>>>,
    /// Where the recording is being written
    pub output_file: PathBuf,
}

impl RecordingHandle {
    /// Create a new recording handle
    pub const fn new(
        stop_flag: Arc<AtomicBool>,
        thread_handle: JoinHandle<Result<()>>,
        output_file: PathBuf,
    ) -> Self {
        Self {
            stop_flag,
            thread_handle: Some(thread_handle),
            output_file,
        }
    }

    /// Signal the recording to stop
    pub fn request_stop(&self) {
        log::info!("Requesting recording stop...");
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    /// Wait for the recording thread to finish
    pub fn join(mut self) -> Result<()> {
        if let Some(handle) = self.thread_handle.take() {
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!("Recording thread panicked")),
            }
        } else {
            Ok(())
        }
    }
}

/// Register an active recording
pub fn set_recording(handle: RecordingHandle) {
    let mut guard = RECORDING_HANDLE.lock().unwrap();
    *guard = Some(handle);
}

/// Check if a recording is currently active
pub fn is_recording() -> bool {
    let guard = RECORDING_HANDLE.lock().unwrap();
    guard.is_some()
}

/// Stop the currently active recording
pub fn stop_recording() -> Result<()> {
    let handle = {
        let mut guard = RECORDING_HANDLE.lock().unwrap();
        guard.take()
    };

    if let Some(handle) = handle {
        log::info!("Stopping recording...");
        handle.request_stop();

        // Spawn a thread to wait for cleanup (don't block the caller)
        let output_file = handle.output_file.clone();
        std::thread::spawn(move || match handle.join() {
            Ok(()) => {
                log::info!("Recording saved to: {}", output_file.display());
                // Launch snappea-edit with --discard so user can trim/convert
                launch_editor(&output_file);
            }
            Err(e) => {
                log::error!("Recording thread error: {e}");
            }
        });

        Ok(())
    } else {
        Err(anyhow::anyhow!("No active recording"))
    }
}

/// Launch snappea-edit to let the user trim/convert the recording.
fn launch_editor(output_file: &Path) {
    let editor_bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("snappea-edit")))
        .unwrap_or_else(|| PathBuf::from("snappea-edit"));

    match std::process::Command::new(&editor_bin)
        .arg("--discard")
        .arg(output_file)
        .spawn()
    {
        Ok(_) => log::info!("Launched editor for: {}", output_file.display()),
        Err(e) => {
            log::warn!("Failed to launch snappea-edit: {e}");
            // Fallback: show a notification instead
            let body = fl!("saved-to", path = output_file.display().to_string());
            let _ = std::process::Command::new("notify-send")
                .arg("--app-name=SnapPea")
                .arg("--icon=video-x-generic")
                .arg(fl!("recording-saved"))
                .arg(&body)
                .spawn();
        }
    }
}
