//! Recording state persistence
//!
//! Manages the state file that tracks active recording sessions

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Recording state persisted to disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingState {
    /// Process ID of the recorder subprocess
    pub pid: u32,
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
    /// Get the path to the state file
    fn state_file_path() -> Result<PathBuf> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .context("XDG_RUNTIME_DIR not set")?;
        Ok(PathBuf::from(runtime_dir).join("snappea-recording.json"))
    }

    /// Save state to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::state_file_path()?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write state file: {}", path.display()))?;
        Ok(())
    }

    /// Load state from disk if it exists
    pub fn load() -> Result<Option<Self>> {
        let path = Self::state_file_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read state file: {}", path.display()))?;
        let state: RecordingState = serde_json::from_str(&json)?;
        Ok(Some(state))
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

/// Check if a recording is currently active
///
/// Automatically cleans up stale state files if the process is dead
pub fn is_recording() -> bool {
    match RecordingState::load() {
        Ok(Some(state)) => {
            if process_alive(state.pid) {
                true
            } else {
                // Process is dead, clean up stale state file
                log::warn!(
                    "Found stale recording state (PID {} is dead), cleaning up",
                    state.pid
                );
                if let Err(e) = RecordingState::delete() {
                    log::error!("Failed to clean up stale state file: {}", e);
                }
                false
            }
        }
        Ok(None) => false,
        Err(e) => {
            log::error!("Failed to load recording state: {}", e);
            false
        }
    }
}

/// Stop the currently active recording
pub fn stop_recording() -> Result<()> {
    let state = RecordingState::load()?
        .context("No active recording found")?;

    // Send SIGTERM to recorder process
    if process_alive(state.pid) {
        log::info!("Stopping recorder process {} gracefully...", state.pid);
        unsafe {
            libc::kill(state.pid as i32, libc::SIGTERM);
        }

        // Wait up to 5 seconds for graceful shutdown
        for _ in 0..50 {
            if !process_alive(state.pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // Force kill if still alive
        if process_alive(state.pid) {
            log::warn!(
                "Recorder process {} did not terminate gracefully, force killing",
                state.pid
            );
            unsafe {
                libc::kill(state.pid as i32, libc::SIGKILL);
            }
        }
    } else {
        log::warn!(
            "Recorder process {} is already dead, cleaning up state",
            state.pid
        );
    }

    RecordingState::delete()?;
    log::info!("Recording stopped, saved to: {}", state.output_file.display());
    // TODO: Show success notification to user when cosmic notification API is available
    Ok(())
}

/// Check if a process is alive
fn process_alive(pid: u32) -> bool {
    unsafe {
        // Signal 0 checks if process exists without sending a signal
        libc::kill(pid as i32, 0) == 0
    }
}
