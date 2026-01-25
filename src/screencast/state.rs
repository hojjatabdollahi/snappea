//! Recording state management
//!
//! Manages active recording sessions with thread-based control

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Global recording handle - only one recording can be active at a time
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
    pub fn new(
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
        Ok(PathBuf::from(runtime_dir).join("snappea-recording.json"))
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
                Ok(_) => {
                    log::info!("Recording saved to: {}", output_file.display());
                    // Show desktop notification
                    show_recording_saved_notification(&output_file);
                }
                Err(e) => {
                    log::error!("Recording thread error: {}", e);
                }
            }
            // Clean up state file
            if let Err(e) = RecordingState::delete() {
                log::error!("Failed to delete recording state file: {}", e);
            }
        });

        Ok(())
    } else {
        Err(anyhow::anyhow!("No active recording"))
    }
}

/// Show a desktop notification that the recording was saved
fn show_recording_saved_notification(output_file: &std::path::Path) {
    let file_name = output_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("recording");

    let body = format!("Saved to {}", output_file.display());

    // Use notify-send (available on most Linux systems)
    let result = std::process::Command::new("notify-send")
        .arg("--app-name=SnapPea")
        .arg("--icon=video-x-generic")
        .arg("Recording Saved")
        .arg(&body)
        .spawn();

    if let Err(e) = result {
        log::warn!("Failed to show notification: {}", e);
    }
}

/// Get the current recording state (if any)
pub fn get_recording_state() -> Option<RecordingState> {
    let guard = RECORDING_HANDLE.lock().unwrap();
    guard.as_ref().map(|h| h.state.clone())
}

/// Cancel the currently active recording (stop and delete the output file)
pub fn cancel_recording() -> Result<()> {
    let handle = {
        let mut guard = RECORDING_HANDLE.lock().unwrap();
        guard.take()
    };

    if let Some(handle) = handle {
        log::info!("Cancelling recording...");
        handle.request_stop();

        let output_file = handle.state.output_file.clone();

        // Wait for the recording thread to finish
        match handle.join() {
            Ok(_) => {
                log::info!("Recording stopped");
            }
            Err(e) => {
                log::error!("Recording thread error: {}", e);
            }
        }

        // Delete the output file
        if output_file.exists() {
            if let Err(e) = std::fs::remove_file(&output_file) {
                log::error!(
                    "Failed to delete recording file {}: {}",
                    output_file.display(),
                    e
                );
            } else {
                log::info!("Deleted recording file: {}", output_file.display());
            }
        }

        // Clean up state file
        if let Err(e) = RecordingState::delete() {
            log::error!("Failed to delete recording state file: {}", e);
        }

        Ok(())
    } else {
        Err(anyhow::anyhow!("No active recording"))
    }
}
