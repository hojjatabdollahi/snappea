//! Main recording loop

use anyhow::{Context, Result};
use drm_fourcc::{DrmFourcc, DrmModifier};
use futures::executor::block_on;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wayland_client::Connection;

use crate::wayland::{CaptureSource, Rect, WaylandHelper};
use super::dmabuf::{DmabufContext, DmabufBuffer, select_best_format, drm_format_to_gst_format};
use super::Pipeline;
use super::encoder::{detect_encoders, EncoderInfo};

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

    // Try to set up DMA-buf capture for zero-copy performance
    let dmabuf_context = match DmabufContext::new() {
        Ok(ctx) => {
            log::info!("DMA-buf context initialized successfully");
            Some(ctx)
        }
        Err(e) => {
            log::warn!("Failed to initialize DMA-buf context: {}. Falling back to SHM.", e);
            None
        }
    };

    // Check for DMA-buf support from both compositor and Wayland protocol
    let wayland_dmabuf_supported = wayland_helper.has_dmabuf_support();
    log::info!("Wayland linux-dmabuf protocol: {}", if wayland_dmabuf_supported { "available" } else { "not available" });

    // Check for DMA-buf format support from screencopy
    let dmabuf_format = if wayland_dmabuf_supported {
        dmabuf_context.as_ref().and_then(|ctx| {
            select_dmabuf_format(&formats, &encoder_info, ctx)
        })
    } else {
        None
    };

    // Determine if we can use full zero-copy DMA-buf path
    let use_dmabuf = dmabuf_format.is_some() && dmabuf_context.is_some() && wayland_dmabuf_supported;

    // Create GStreamer pipeline
    log::info!("Creating GStreamer pipeline...");
    let (_, _, record_width, record_height) = region;

    let pipeline = if use_dmabuf {
        let (drm_format, _modifier) = dmabuf_format.unwrap();
        log::info!("Creating DMA-buf pipeline with format {:?}", drm_format);
        Pipeline::new_dmabuf(
            &encoder_info,
            container,
            &output_file,
            record_width,
            record_height,
            framerate,
            drm_format,
        )
        .context("Failed to create DMA-buf GStreamer pipeline")?
    } else {
        Pipeline::new(
            &encoder_info,
            container,
            &output_file,
            record_width,
            record_height,
            framerate,
        )
        .context("Failed to create GStreamer pipeline. Check logs for details.")?
    };

    pipeline.start()
        .context("Failed to start GStreamer pipeline")?;
    log::info!("Recording started successfully!");

    // Main recording loop
    let frame_duration = Duration::from_secs_f64(1.0 / framerate as f64);
    let mut frame_count = 0u64;
    let mut consecutive_errors = 0u32;
    const MAX_CONSECUTIVE_ERRORS: u32 = 10;
    let start_time = Instant::now();

    // Allocate DMA-buf buffer if using zero-copy path
    let dmabuf_buffer: Option<DmabufBuffer> = if use_dmabuf {
        let (drm_format, modifier) = dmabuf_format.unwrap();
        let ctx = dmabuf_context.as_ref().unwrap();
        log::info!(
            "Attempting DMA-buf allocation: {}x{}, format={:?}, modifier={:?}",
            buffer_width, buffer_height, drm_format, modifier
        );
        match ctx.allocate_buffer(buffer_width, buffer_height, drm_format, modifier) {
            Ok(buf) => {
                log::info!(
                    "Allocated DMA-buf buffer: {}x{}, format={:?} (0x{:08x}), modifier=0x{:016x}, stride={}, size={}",
                    buf.width, buf.height, buf.format, buf.format as u32, u64::from(buf.modifier), buf.stride, buf.size
                );
                Some(buf)
            }
            Err(e) => {
                log::warn!("Failed to allocate DMA-buf buffer: {}. Falling back to SHM.", e);
                None
            }
        }
    } else {
        None
    };

    // Determine actual capture mode
    let actual_dmabuf = dmabuf_buffer.is_some() && pipeline.is_dmabuf_mode();
    if actual_dmabuf {
        log::info!("Using DMA-buf zero-copy capture path");
    } else {
        log::info!("Using SHM capture path");
    }

    while !STOP_REQUESTED.load(Ordering::Relaxed) {
        let frame_start = Instant::now();

        // Calculate timestamp (in nanoseconds)
        let timestamp = frame_count * 1_000_000_000 / framerate as u64;

        // Capture and push frame
        let capture_result = if actual_dmabuf {
            let dmabuf = dmabuf_buffer.as_ref().unwrap();
            capture_frame_dmabuf(&wayland_helper, &session, dmabuf, &pipeline, timestamp)
        } else {
            capture_frame_shm(&wayland_helper, &session, &formats)
                .and_then(|frame_data| pipeline.push_frame(&frame_data, timestamp))
        };

        match capture_result {
            Ok(()) => {
                consecutive_errors = 0;
                frame_count += 1;

                // Log progress every 60 frames
                if frame_count % 60 == 0 {
                    let elapsed = start_time.elapsed();
                    let fps = frame_count as f64 / elapsed.as_secs_f64();
                    log::info!(
                        "Recording: {} frames captured ({:.1} fps, {})",
                        frame_count,
                        fps,
                        if actual_dmabuf { "DMA-buf" } else { "SHM" }
                    );
                }
            }
            Err(e) => {
                log::error!("Failed to capture/push frame: {}", e);
                consecutive_errors += 1;
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    log::error!(
                        "Too many consecutive errors ({}), stopping recording",
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

/// Select the best DMA-buf format from available formats
fn select_dmabuf_format(
    formats: &cosmic_client_toolkit::screencopy::Formats,
    encoder_info: &EncoderInfo,
    dmabuf_ctx: &DmabufContext,
) -> Option<(DrmFourcc, DrmModifier)> {
    // Check if compositor advertises any DMA-buf formats
    if formats.dmabuf_formats.is_empty() {
        log::debug!("Compositor does not support DMA-buf screencopy");
        return None;
    }

    // Convert compositor formats to our format
    let available_formats: Vec<(DrmFourcc, Vec<DrmModifier>)> = formats
        .dmabuf_formats
        .iter()
        .filter_map(|(fourcc, modifiers)| {
            // Try to parse the fourcc as a DRM format
            let drm_format = DrmFourcc::try_from(*fourcc).ok()?;

            // Filter to modifiers we can actually use
            let valid_modifiers: Vec<DrmModifier> = modifiers
                .iter()
                .filter_map(|&m| {
                    let modifier = DrmModifier::from(m);
                    // Check if GBM can allocate this format/modifier combo
                    if dmabuf_ctx.is_format_supported(drm_format, modifier) {
                        Some(modifier)
                    } else {
                        None
                    }
                })
                .collect();

            if valid_modifiers.is_empty() {
                None
            } else {
                Some((drm_format, valid_modifiers))
            }
        })
        .collect();

    if available_formats.is_empty() {
        log::debug!("No usable DMA-buf formats found");
        return None;
    }

    log::debug!(
        "Available DMA-buf formats: {:?}",
        available_formats.iter().map(|(f, _)| f).collect::<Vec<_>>()
    );

    // Select the best format for the encoder
    let prefer_hardware = encoder_info.hardware;
    let result = select_best_format(&available_formats, prefer_hardware);

    if let Some((format, _modifier)) = &result {
        // Verify GStreamer can handle this format
        if drm_format_to_gst_format(*format).is_none() {
            log::debug!(
                "Selected format {:?} not supported by GStreamer, skipping DMA-buf",
                format
            );
            return None;
        }
    }

    result
}

/// Capture a frame using DMA-buf zero-copy path
///
/// This captures directly into a DMA-buf, which can then be passed to GStreamer
/// without any CPU copies. The flow is:
/// 1. Create wl_buffer from DMA-buf fd
/// 2. Screencopy captures into the DMA-buf (GPU operation)
/// 3. Pass same DMA-buf fd to GStreamer (GPU encodes directly)
fn capture_frame_dmabuf(
    wayland_helper: &WaylandHelper,
    session: &crate::wayland::Session,
    dmabuf: &DmabufBuffer,
    pipeline: &Pipeline,
    timestamp: u64,
) -> Result<()> {
    // Create wl_buffer from DMA-buf fd using linux-dmabuf protocol
    let fourcc = dmabuf.format as u32;
    let modifier = u64::from(dmabuf.modifier);

    log::debug!(
        "Creating wl_buffer from DMA-buf: {}x{}, format=0x{:08x}, modifier=0x{:016x}, stride={}",
        dmabuf.width, dmabuf.height, fourcc, modifier, dmabuf.stride
    );

    let wl_buffer = wayland_helper
        .create_dmabuf_buffer(
            dmabuf.fd.as_fd(),
            dmabuf.width,
            dmabuf.height,
            dmabuf.stride,
            fourcc,
            modifier,
        )
        .ok_or_else(|| anyhow::anyhow!("Failed to create wl_buffer from DMA-buf"))?;

    // Full frame damage
    let damage = &[Rect {
        x: 0,
        y: 0,
        width: dmabuf.width as i32,
        height: dmabuf.height as i32,
    }];

    // Capture into the DMA-buf (zero-copy from compositor)
    let _frame = block_on(session.capture_wl_buffer(&wl_buffer, damage))
        .map_err(|e| anyhow::anyhow!("DMA-buf screencopy failed: {:?}", e))?;

    // Cleanup wl_buffer (the underlying DMA-buf fd is still valid)
    wl_buffer.destroy();

    // Push DMA-buf fd to GStreamer (zero-copy to encoder)
    use std::os::fd::AsRawFd;
    pipeline.push_dmabuf_frame(dmabuf.fd.as_raw_fd(), dmabuf.size, timestamp)?;

    Ok(())
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
