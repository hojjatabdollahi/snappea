//! Main recording loop

use anyhow::{Context, Result};
use futures::executor::block_on;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wayland_client::Connection;

use crate::wayland::{CaptureSource, Rect, WaylandHelper};
use super::Pipeline;
use super::encoder::detect_encoders;

/// Global flag for graceful shutdown on SIGTERM
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Start recording
///
/// # Arguments
/// * `output_file` - Path to save video
/// * `output_name` - Wayland output name
/// * `region` - (x, y, width, height) in logical coordinates
/// * `encoder` - Encoder element name
/// * `container` - Container format
/// * `framerate` - Frames per second
pub fn start_recording(
    output_file: PathBuf,
    output_name: String,
    region: (i32, i32, u32, u32),
    encoder: String,
    container: crate::config::Container,
    framerate: u32,
) -> Result<()> {
    log::info!(
        "Starting recording: output={}, region={:?}, encoder={}, fps={}",
        output_file.display(),
        region,
        encoder,
        framerate
    );

    // Set up SIGTERM handler for graceful shutdown
    setup_signal_handler()?;

    // Connect to Wayland
    log::info!("Connecting to Wayland compositor...");
    let conn = Connection::connect_to_env()
        .context("Failed to connect to Wayland compositor. Is a Wayland session running?")?;
    let wayland_helper = WaylandHelper::new(conn);

    // Find the output by name
    log::info!("Looking for output: {}", output_name);
    let all_outputs = wayland_helper.outputs();
    let available_outputs: Vec<_> = all_outputs
        .iter()
        .filter_map(|o| {
            wayland_helper
                .output_info(o)
                .and_then(|info| info.name.as_ref().map(|name| (name.clone(), o.clone())))
        })
        .collect();

    log::info!(
        "Available outputs: {:?}",
        available_outputs.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );

    let output = available_outputs
        .iter()
        .find(|(name, _)| name == &output_name)
        .map(|(_, output)| output.clone())
        .with_context(|| {
            format!(
                "Output '{}' not found. Available outputs: {:?}",
                output_name,
                available_outputs.iter().map(|(name, _)| name).collect::<Vec<_>>()
            )
        })?;

    log::info!("Found output: {}", output_name);

    // Create screencopy session for the output
    let capture_source = CaptureSource::Output(output);
    let overlay_cursor = false; // Don't capture cursor in recordings
    let session = wayland_helper.capture_source_session(capture_source, overlay_cursor);

    // Wait for formats to be negotiated
    log::info!("Waiting for screencopy formats...");
    let formats = block_on(session.wait_for_formats(|formats| formats.clone()))
        .context("Failed to get screencopy formats")?;

    let (buffer_width, buffer_height) = formats.buffer_size;
    log::info!(
        "Screencopy formats: {}x{}, {:?} SHM formats, {:?} DMA-buf formats",
        buffer_width,
        buffer_height,
        formats.shm_formats.len(),
        formats.dmabuf_formats.len()
    );

    // Find encoder by name
    let encoders = detect_encoders()
        .context("Failed to detect available video encoders. Is GStreamer installed?")?;

    log::info!(
        "Available encoders: {:?}",
        encoders.iter().map(|e| &e.gst_element).collect::<Vec<_>>()
    );

    let encoder_info = encoders
        .into_iter()
        .find(|e| e.gst_element == encoder)
        .with_context(|| {
            format!(
                "Encoder '{}' not available. Install GStreamer plugins for this encoder.",
                encoder
            )
        })?;

    log::info!("Using encoder: {} ({:?})", encoder_info.gst_element, encoder_info.codec);

    // Create GStreamer pipeline
    log::info!("Creating GStreamer pipeline...");
    let (_, _, record_width, record_height) = region;
    let pipeline = Pipeline::new(
        &encoder_info,
        container,
        &output_file,
        record_width,
        record_height,
        framerate,
    )
    .context("Failed to create GStreamer pipeline. Check logs for details.")?;

    pipeline.start()
        .context("Failed to start GStreamer pipeline")?;
    log::info!("Recording started successfully!");

    // Main recording loop
    let frame_duration = Duration::from_secs_f64(1.0 / framerate as f64);
    let mut frame_count = 0u64;
    let mut consecutive_errors = 0u32;
    const MAX_CONSECUTIVE_ERRORS: u32 = 10;
    let start_time = Instant::now();

    while !STOP_REQUESTED.load(Ordering::Relaxed) {
        let frame_start = Instant::now();

        // Capture frame using SHM
        match capture_frame_shm(&wayland_helper, &session, &formats) {
            Ok(frame_data) => {
                // Calculate timestamp (in nanoseconds)
                let timestamp = frame_count * 1_000_000_000 / framerate as u64;

                // Push frame to GStreamer pipeline
                if let Err(e) = pipeline.push_frame(&frame_data, timestamp) {
                    log::error!("Failed to push frame to pipeline: {}", e);
                    consecutive_errors += 1;
                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        log::error!(
                            "Too many consecutive errors ({}), stopping recording",
                            consecutive_errors
                        );
                        break;
                    }
                } else {
                    consecutive_errors = 0; // Reset on success
                    frame_count += 1;

                    // Log progress every 60 frames
                    if frame_count % 60 == 0 {
                        let elapsed = start_time.elapsed();
                        let fps = frame_count as f64 / elapsed.as_secs_f64();
                        log::info!(
                            "Recording: {} frames captured ({:.1} fps)",
                            frame_count,
                            fps
                        );
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to capture frame: {}", e);
                consecutive_errors += 1;
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    log::error!(
                        "Too many consecutive frame capture failures ({}), stopping recording",
                        consecutive_errors
                    );
                    break;
                }
            }
        }

        // Frame rate limiting
        let frame_elapsed = frame_start.elapsed();
        if frame_elapsed < frame_duration {
            std::thread::sleep(frame_duration - frame_elapsed);
        }
    }

    // Graceful shutdown
    log::info!("Stopping recording... ({} frames captured)", frame_count);
    pipeline.finish()?;
    log::info!("Recording finished: {}", output_file.display());

    Ok(())
}

/// Capture a single frame using shared memory
fn capture_frame_shm(
    wayland_helper: &WaylandHelper,
    session: &crate::wayland::Session,
    formats: &cosmic_client_toolkit::screencopy::Formats,
) -> Result<Vec<u8>> {
    let (width, height) = formats.buffer_size;

    // Create shared memory buffer
    let fd = crate::buffer::create_memfd(width, height);

    // Create Wayland SHM buffer
    let wl_buffer = wayland_helper.create_shm_buffer(
        &fd,
        width,
        height,
        width * 4,
        wayland_client::protocol::wl_shm::Format::Abgr8888,
    );

    // Full frame damage
    let damage = &[Rect {
        x: 0,
        y: 0,
        width: width as i32,
        height: height as i32,
    }];

    // Capture to buffer
    let _frame = block_on(session.capture_wl_buffer(&wl_buffer, damage))
        .map_err(|e| anyhow::anyhow!("Screencopy failed: {:?}", e))?;

    // Read data from shared memory
    let mmap = unsafe { memmap2::Mmap::map(&fd)? };
    let frame_data = mmap.to_vec();

    // Cleanup
    wl_buffer.destroy();

    // Convert ABGR to RGBA for GStreamer
    let mut rgba_data = Vec::with_capacity(frame_data.len());
    for pixel in frame_data.chunks_exact(4) {
        // ABGR -> RGBA
        rgba_data.push(pixel[3]); // R
        rgba_data.push(pixel[2]); // G
        rgba_data.push(pixel[1]); // B
        rgba_data.push(pixel[0]); // A
    }

    Ok(rgba_data)
}

/// Set up signal handler for SIGTERM
fn setup_signal_handler() -> Result<()> {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        // Set up SIGTERM handler
        unsafe {
            let handler = sigterm_handler as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::signal(libc::SIGTERM, handler);
            libc::signal(libc::SIGINT, handler);
        }
    });

    Ok(())
}

/// SIGTERM signal handler
extern "C" fn sigterm_handler(_: libc::c_int) {
    log::info!("Received stop signal");
    STOP_REQUESTED.store(true, Ordering::Relaxed);
}
