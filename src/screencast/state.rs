//! Recording state persistence
//!
//! Manages the state file that tracks active recording sessions

use anyhow::{Context, Result};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
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

/// Stop the currently active recording (non-blocking)
///
/// Sends SIGTERM to the recorder process and spawns a background thread
/// to handle cleanup. Returns immediately so the UI can appear fast.
pub fn stop_recording() -> Result<()> {
    let state = RecordingState::load()?
        .context("No active recording found")?;

    let pid = Pid::from_raw(state.pid as i32);

    // Send SIGTERM to recorder process
    if process_alive(state.pid) {
        log::info!("Sending SIGTERM to recorder process {}...", state.pid);
        if let Err(e) = signal::kill(pid, Signal::SIGTERM) {
            log::error!("Failed to send SIGTERM to process {}: {}", state.pid, e);
        }

        // Spawn background thread to wait and cleanup
        // This allows the UI to appear immediately
        let output_file = state.output_file.clone();
        std::thread::spawn(move || {
            cleanup_recording(state.pid, output_file);
        });
    } else {
        log::warn!(
            "Recorder process {} is already dead, cleaning up state",
            state.pid
        );
        RecordingState::delete().ok();
    }

    Ok(())
}

/// Background cleanup after stopping recording
fn cleanup_recording(pid: u32, output_file: PathBuf) {
    let nix_pid = Pid::from_raw(pid as i32);

    // Wait up to 5 seconds for graceful shutdown
    for _ in 0..50 {
        if !process_alive(pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Force kill if still alive
    if process_alive(pid) {
        log::warn!(
            "Recorder process {} did not terminate gracefully, force killing",
            pid
        );
        if let Err(e) = signal::kill(nix_pid, Signal::SIGKILL) {
            log::error!("Failed to send SIGKILL to process {}: {}", pid, e);
        }

        // Wait a bit more for the kill to take effect
        for _ in 0..10 {
            if !process_alive(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    // Clean up state file
    if let Err(e) = RecordingState::delete() {
        log::error!("Failed to delete recording state: {}", e);
    }

    log::info!("Recording stopped, saved to: {}", output_file.display());
    // TODO: Show success notification to user when cosmic notification API is available
}

/// Check if a process is alive
fn process_alive(pid: u32) -> bool {
    // Signal 0 checks if process exists without sending a signal
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}
