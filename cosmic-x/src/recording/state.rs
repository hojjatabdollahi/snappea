// SPDX-License-Identifier: GPL-3.0-only

//! The active recording and its control handle.

use crate::fl;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
    /// Recording metadata
    pub state: RecordingState,
}

impl RecordingHandle {
    /// Create a new recording handle
    pub const fn new(
        stop_flag: Arc<AtomicBool>,
        thread_handle: JoinHandle<Result<()>>,
        state: RecordingState,
    ) -> Self {
        Self {
            stop_flag,
            thread_handle: Some(thread_handle),
            state,
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

/// Recording state metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingState {
    /// Output file path
    pub output_file: PathBuf,
    /// Recording region (x, y, width, height)
    pub region: (i32, i32, u32, u32),
    /// Output name being recorded
    pub output_name: String,
    /// When recording started (ISO 8601)
    pub started_at: String,
}

impl RecordingState {
    /// Get the path to the state file (for compatibility with external tools)
    fn state_file_path() -> Result<PathBuf> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR not set")?;
        Ok(PathBuf::from(runtime_dir).join("cosmic-x-recording.json"))
    }

    /// Save state to disk (for external tools to detect active recording)
    pub fn save(&self) -> Result<()> {
        let path = Self::state_file_path()?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write state file: {}", path.display()))?;
        Ok(())
    }

    /// Delete state file
    pub fn delete() -> Result<()> {
        let path = Self::state_file_path()?;
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to delete state file: {}", path.display()))?;
        }
        Ok(())
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
        let output_file = handle.state.output_file.clone();
        std::thread::spawn(move || {
            match handle.join() {
                Ok(()) => {
                    log::info!("Recording saved to: {}", output_file.display());
                    // Launch cosmic-x-trim with --discard so user can trim/convert
                    launch_editor(&output_file);
                }
                Err(e) => {
                    log::error!("Recording thread error: {e}");
                }
            }
            // Clean up state file
            if let Err(e) = RecordingState::delete() {
                log::error!("Failed to delete recording state file: {e}");
            }
        });

        Ok(())
    } else {
        Err(anyhow::anyhow!("No active recording"))
    }
}

/// Launch cosmic-x-trim to let the user trim/convert the recording.
fn launch_editor(output_file: &std::path::Path) {
    let editor_bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("cosmic-x-trim")))
        .unwrap_or_else(|| std::path::PathBuf::from("cosmic-x-trim"));

    match std::process::Command::new(&editor_bin)
        .arg("--discard")
        .arg(output_file)
        .spawn()
    {
        Ok(_) => log::info!("Launched editor for: {}", output_file.display()),
        Err(e) => {
            log::warn!("Failed to launch cosmic-x-trim: {e}");
            // Fallback: show a notification instead
            let body = fl!("saved-to", path = output_file.display().to_string());
            let _ = std::process::Command::new("notify-send")
                .arg("--app-name=COSMIC X")
                .arg("--icon=video-x-generic")
                .arg(fl!("recording-saved"))
                .arg(&body)
                .spawn();
        }
    }
}
