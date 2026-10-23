// SPDX-License-Identifier: GPL-3.0-only

//! The recording loop.

use anyhow::{Context, Result};
use drm_fourcc::{DrmFourcc, DrmModifier};
use futures::executor::block_on;
use gstreamer as gst;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wayland_client::Connection;
use wayland_client::protocol::wl_shm;

use super::dmabuf::{DmabufContext, TripleBufferPool, select_zero_copy_source_format};
use super::encoder::{Codec, EncoderInfo, detect_encoders, min_input_size};
use super::pipeline::{
    CropRegion, Pipeline, crop_touches_trailing_edge, pipeline_output_size, probe_import_fourcc,
    vapostproc_dmabuf_formats,
};
use crate::config::Container;
use crate::wayland::{CaptureSource, Rect, WaylandHelper};

/// Global flag for graceful shutdown on SIGTERM
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

fn compatible_software_encoder(
    encoders: &[EncoderInfo],
    container: Container,
    requested_codec: Codec,
) -> Option<EncoderInfo> {
    let find = |element_name: &str| {
        encoders
            .iter()
            .find(|encoder| !encoder.hardware && encoder.gst_element == element_name)
            .cloned()
    };

    match container {
        Container::Webm => find("vp9enc"),
        Container::Mp4 => find("x264enc"),
        Container::Mkv => match requested_codec {
            Codec::VP9 | Codec::AV1 => find("vp9enc").or_else(|| find("x264enc")),
            Codec::H264 | Codec::H265 => find("x264enc").or_else(|| find("vp9enc")),
        },
    }
}

/// Pick the best available encoder whose codec is compatible with `container`,
/// preferring hardware encoders. `encoders` is assumed sorted by priority.
fn compatible_encoder(encoders: &[EncoderInfo], container: Container) -> Option<EncoderInfo> {
    encoders
        .iter()
        .find(|e| e.hardware && e.codec.supports_container(container))
        .or_else(|| {
            encoders
                .iter()
                .find(|e| e.codec.supports_container(container))
        })
        .cloned()
}

/// Ensure the requested encoder can actually be muxed into the chosen container,
/// substituting a compatible encoder when it cannot.
fn ensure_container_compatible(
    requested: EncoderInfo,
    encoders: &[EncoderInfo],
    container: Container,
) -> Result<EncoderInfo> {
    if requested.codec.supports_container(container) {
        return Ok(requested);
    }

    let fallback = compatible_encoder(encoders, container).with_context(|| {
        format!(
            "Encoder '{}' ({:?}) cannot be muxed into a {:?} container, and no compatible \
             encoder is installed. Install a suitable GStreamer encoder (e.g. vp9enc for WebM).",
            requested.gst_element, requested.codec, container
        )
    })?;

    log::warn!(
        "Encoder '{}' ({:?}) is not compatible with the {:?} container; using '{}' ({:?}) instead",
        requested.gst_element,
        requested.codec,
        container,
        fallback.gst_element,
        fallback.codec
    );

    Ok(fallback)
}

/// Work around `GStreamer` VA-API H.265 encoders rounding widths that are not a
/// multiple of 64 up without a cropping window: use a hardware H.264 encoder instead.
fn substitute_h265_for_unaligned_width(
    requested: EncoderInfo,
    encoders: &[EncoderInfo],
    width: u32,
) -> EncoderInfo {
    if requested.codec != Codec::H265 || width.is_multiple_of(64) {
        return requested;
    }

    let Some(h264) = encoders
        .iter()
        .find(|e| e.hardware && e.codec == Codec::H264)
        .or_else(|| encoders.iter().find(|e| e.codec == Codec::H264))
    else {
        log::warn!(
            "H.265 width {width}px is not a multiple of 64 (GStreamer VA encoder mis-sizes it), \
             but no H.264 encoder is available to substitute; output may be too wide"
        );
        return requested;
    };

    log::info!(
        "Substituting '{}' for '{}': H.265 width {}px is not 64-aligned, which the GStreamer \
         VA H.265 encoder encodes at the wrong (rounded-up) width",
        h264.gst_element,
        requested.gst_element,
        width
    );
    h264.clone()
}

fn select_effective_encoder(
    requested: EncoderInfo,
    encoders: &[EncoderInfo],
    container: Container,
    copied_path_reason: Option<&'static str>,
    output_size: (u32, u32),
) -> EncoderInfo {
    let Some(reason) = copied_path_reason else {
        return requested;
    };

    if !requested.hardware {
        return requested;
    }

    if let Some(fallback) = compatible_software_encoder(encoders, container, requested.codec) {
        log::info!(
            "Using software encoder '{}' for {} {}x{}; copied fallback cannot reliably feed the VAAPI hardware encoder",
            fallback.gst_element,
            reason,
            output_size.0,
            output_size.1
        );
        fallback
    } else {
        log::warn!(
            "{} {}x{} selected but no compatible software encoder is available; trying '{}' without DMA-BUF zero-copy",
            reason,
            output_size.0,
            output_size.1,
            requested.gst_element
        );
        requested
    }
}

/// Record from the command line (`--record`). `region` is `(x, y, w, h)` in
/// logical coordinates. `toplevel_index` captures a window instead. Stops on
/// SIGTERM or SIGINT.
pub fn start_recording(
    output_file: PathBuf,
    output_name: String,
    region: (i32, i32, u32, u32),
    logical_size: (u32, u32),
    encoder: String,
    container: Container,
    framerate: u32,
    toplevel_index: Option<usize>,
    show_cursor: bool,
    system_audio: bool,
    microphone: bool,
) -> Result<()> {
    setup_signal_handler();

    let conn = Connection::connect_to_env()
        .context("Failed to connect to Wayland compositor. Is a Wayland session running?")?;
    let wayland_helper = WaylandHelper::new(conn);

    let output = wayland_helper
        .outputs()
        .into_iter()
        .find(|o| {
            wayland_helper
                .output_info(o)
                .is_some_and(|info| info.name.as_deref() == Some(output_name.as_str()))
        })
        .with_context(|| format!("Output '{output_name}' not found"))?;

    let capture_source = match toplevel_index {
        Some(idx) => {
            // Toplevels arrive asynchronously. Index into all of them, minimized included.
            if !wayland_helper.wait_for_toplevels(Duration::from_secs(2)) {
                anyhow::bail!("Timeout waiting for toplevel info from compositor");
            }
            let toplevels = wayland_helper.all_toplevels();
            let toplevel = toplevels.get(idx).cloned().with_context(|| {
                format!(
                    "Toplevel index {idx} out of range ({} toplevels)",
                    toplevels.len()
                )
            })?;
            CaptureSource::Toplevel(toplevel)
        }
        None => CaptureSource::Output(output),
    };

    record(
        wayland_helper,
        capture_source,
        output_file,
        region,
        logical_size,
        encoder,
        container,
        framerate,
        show_cursor,
        system_audio,
        microphone,
        &STOP_REQUESTED,
    )
}

/// Frame slots shared with the compositor over `wl_shm`. Four: one being
/// captured, one in flight to the encoder, up to three sitting in the leaky
/// queue, and the last one kept for the closing frame.
const SHM_SLOTS: usize = 4;

/// A frame slot shared with the compositor over `wl_shm`. The memfd is mapped
/// once and wrapped as a `GstBuffer` without copying. The compositor may write
/// into it only while `GStreamer` holds no reference.
struct ShmSlot {
    wl_buffer: wayland_client::protocol::wl_buffer::WlBuffer,
    buffer: gst::Buffer,
}

impl ShmSlot {
    fn new(helper: &WaylandHelper, width: u32, height: u32) -> Result<Self> {
        let fd = crate::wayland::create_memfd(width, height);
        // ABGR8888 is R, G, B, A in memory: RGBA to GStreamer.
        let wl_buffer =
            helper.create_shm_buffer(&fd, width, height, width * 4, wl_shm::Format::Abgr8888);
        let map = unsafe { memmap2::Mmap::map(&fd) }.context("Failed to map shm frame")?;
        Ok(Self {
            wl_buffer,
            buffer: gst::Buffer::from_slice(map),
        })
    }

    /// Nothing downstream references the buffer or its memory any more.
    fn is_free(&self) -> bool {
        unreferenced(&self.buffer)
    }
}

/// Whether we hold the only reference to `buffer` and to its memory. A shallow
/// copy shares the memory, so the buffer alone being writable is not enough.
fn unreferenced(buffer: &gst::Buffer) -> bool {
    buffer.is_writable()
        && buffer.n_memory() > 0
        && unsafe {
            gst::ffi::gst_mini_object_is_writable(buffer.peek_memory(0).as_ptr().cast()) != 0
        }
}

/// Capture frames into free slots as the compositor produces them and hand
/// each to the encoder thread. Runs until `stop` or until the compositor keeps
/// failing.
fn shm_capture_loop(
    session: &crate::wayland::Session,
    mut slots: Vec<ShmSlot>,
    tx: &std::sync::mpsc::SyncSender<gst::Buffer>,
    stop: &AtomicBool,
    start: Instant,
) {
    let damage = [Rect {
        x: 0,
        y: 0,
        width: i32::MAX,
        height: i32::MAX,
    }];
    let mut failures = 0u32;
    let mut last_error_log = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let Some(slot) = slots.iter_mut().find(|s| s.is_free()) else {
            // The encoder holds every slot, so it is behind. Wait for it.
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        match block_on(session.capture_wl_buffer(&slot.wl_buffer, &damage)) {
            Ok(_frame) => {
                failures = 0;
                let pts = gst::ClockTime::from_nseconds(start.elapsed().as_nanos() as u64);
                slot.buffer.get_mut().unwrap().set_pts(pts);
                // A full channel means the encoder thread has not taken the previous
                // frame yet. Dropping this one keeps latency at one frame.
                let _ = tx.try_send(slot.buffer.clone());
            }
            Err(e) => {
                failures += 1;
                if last_error_log.elapsed() > Duration::from_secs(2) {
                    log::warn!("Frame capture failed ({failures} in a row): {e:?}");
                    last_error_log = Instant::now();
                }
                if failures >= 100 {
                    log::error!("Giving up after {failures} consecutive capture failures");
                    break;
                }
                std::thread::sleep(Duration::from_millis((10 << failures.min(6)).min(500)));
            }
        }
    }
    for slot in slots {
        slot.wl_buffer.destroy();
    }
}

/// Select the best DMA-buf format from available formats
fn select_dmabuf_format(
    formats: &cosmic_client_toolkit::screencopy::Formats,
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

    let importer = vapostproc_dmabuf_formats();
    log::debug!("vapostproc DMA-BUF formats: {importer:?}");
    let result = select_zero_copy_source_format(&available_formats, &importer);
    match result {
        Some((format, modifier)) => log::info!(
            "Selected zero-copy DMA-BUF source format: {format:?}, modifier={modifier:?}"
        ),
        None => log::info!("No DMA-BUF format is shared by the compositor and vapostproc"),
    }
    result
}

/// Everything the zero-copy path needs, or `None` with the reason logged: the
/// GBM context, the format and modifier the compositor renders into, and the
/// fourcc the importer wants those buffers described as.
fn zero_copy_setup(
    wayland_helper: &WaylandHelper,
    formats: &cosmic_client_toolkit::screencopy::Formats,
    encoder: &EncoderInfo,
) -> Option<(DmabufContext, DrmFourcc, DrmModifier, DrmFourcc)> {
    if !encoder.supports_dmabuf_zero_copy {
        log::info!(
            "Zero-copy off: {} takes no DMA-BUF input",
            encoder.gst_element
        );
        return None;
    }
    if !wayland_helper.has_dmabuf_support() {
        log::info!("Zero-copy off: the compositor offers no linux-dmabuf");
        return None;
    }
    let ctx = match DmabufContext::new() {
        Ok(ctx) => ctx,
        Err(e) => {
            log::warn!("Zero-copy off: {e}");
            return None;
        }
    };
    let (format, modifier) = select_dmabuf_format(formats, &ctx)?;
    // The driver may read the imported buffer in another byte order than the
    // compositor writes it. Probe for the description that reads correctly.
    let import = match probe_import_fourcc(&ctx, format, modifier, encoder) {
        Ok(import) => import,
        Err(e) => {
            log::warn!("Zero-copy off: {e:#}");
            return None;
        }
    };
    if import != format {
        log::info!("DMA-BUF {format:?} buffers are described to the encoder as {import:?}");
    }
    Some((ctx, format, modifier, import))
}

/// Capture a frame through the zero-copy DMA-buf path, rotating three buffers
/// so compositor and encoder work in parallel.
fn capture_frame_dmabuf_triple(
    wayland_helper: &WaylandHelper,
    session: &crate::wayland::Session,
    pool: &mut TripleBufferPool,
    pipeline: &Pipeline,
    timestamp: u64,
) -> Result<()> {
    // Get current buffer from the pool
    let dmabuf = pool.current();

    // Create wl_buffer from DMA-buf fd using linux-dmabuf protocol
    let fourcc = dmabuf.format as u32;
    let modifier = u64::from(dmabuf.modifier);

    let wl_buffer = wayland_helper
        .create_dmabuf_buffer(
            dmabuf.fd.as_fd(),
            dmabuf.width,
            dmabuf.height,
            dmabuf.stride,
            dmabuf.offset,
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
        .map_err(|e| anyhow::anyhow!("DMA-buf screencopy failed: {e:?}"))?;

    // Cleanup wl_buffer (the underlying DMA-buf fd is still valid)
    wl_buffer.destroy();

    // Push DMA-buf-backed GstMemory to GStreamer without copying it to CPU memory.
    pipeline.push_dmabuf_frame(dmabuf, timestamp)?;

    // Advance to next buffer for next frame
    pool.advance();

    Ok(())
}

/// Set up signal handler for SIGTERM
fn setup_signal_handler() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| unsafe {
        let handler = sigterm_handler as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    });
}

/// Only an atomic store, since nothing else is async-signal-safe.
extern "C" fn sigterm_handler(_: libc::c_int) {
    STOP_REQUESTED.store(true, Ordering::Relaxed);
}

/// The recording loop: capture `capture_source` into `output_file` until `stop`
/// is set. `region` is `(x, y, w, h)` in logical coordinates and is ignored for
/// toplevels, which are recorded whole.
pub fn record(
    wayland_helper: WaylandHelper,
    capture_source: CaptureSource,
    output_file: PathBuf,
    region: (i32, i32, u32, u32),
    logical_size: (u32, u32),
    encoder: String,
    container: crate::config::Container,
    framerate: u32,
    show_cursor: bool,
    system_audio: bool,
    microphone: bool,
    stop: &AtomicBool,
) -> Result<()> {
    let is_toplevel = matches!(capture_source, CaptureSource::Toplevel(_));

    if is_toplevel {
        log::info!(
            "Starting toplevel recording: output={}, encoder={}, fps={}",
            output_file.display(),
            encoder,
            framerate
        );
    } else {
        log::info!(
            "Starting region recording: output={}, region={:?}, encoder={}, fps={}",
            output_file.display(),
            region,
            encoder,
            framerate
        );
    }

    log::info!("Cursor visibility in recording: {show_cursor}");
    log::info!("Audio recording: {system_audio:?}, {microphone:?}");

    let session = wayland_helper.capture_source_session(capture_source, show_cursor);

    // Wait for formats to be negotiated
    log::info!("Waiting for screencopy formats...");
    let formats = block_on(session.wait_for_formats(std::clone::Clone::clone))
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

    // A configured encoder can vanish with a GStreamer upgrade (`vaapih264enc`
    // did). Fall back to the best one rather than refusing to record.
    let requested_encoder_info =
        if let Some(info) = encoders.iter().find(|e| e.gst_element == encoder) {
            info.clone()
        } else {
            let best = encoders
                .first()
                .cloned()
                .context("No video encoders available")?;
            log::warn!(
                "Encoder '{encoder}' is not available; using '{}' instead",
                best.gst_element
            );
            best
        };

    // Make sure the encoder's codec can actually be muxed into the chosen container
    // (e.g. an H.264 encoder cannot go into WebM). Otherwise swap to a compatible one.
    let requested_encoder_info =
        ensure_container_compatible(requested_encoder_info, &encoders, container)?;

    // Calculate crop region if needed
    let (region_x, region_y, record_width, record_height) = region;
    let (logical_width, logical_height) = logical_size;

    let scale_x = if logical_width > 0 {
        f64::from(buffer_width) / f64::from(logical_width)
    } else {
        1.0
    };
    let scale_y = if logical_height > 0 {
        f64::from(buffer_height) / f64::from(logical_height)
    } else {
        1.0
    };

    let physical_x = (f64::from(region_x) * scale_x).round() as u32;
    let physical_y = (f64::from(region_y) * scale_y).round() as u32;
    let physical_width = (f64::from(record_width) * scale_x).round() as u32;
    let physical_height = (f64::from(record_height) * scale_y).round() as u32;

    // For toplevel capture, record entire window (no crop)
    let crop = if is_toplevel {
        log::info!("Toplevel capture: recording entire window ({buffer_width}x{buffer_height})");
        None
    } else if physical_x != 0
        || physical_y != 0
        || physical_width != buffer_width
        || physical_height != buffer_height
    {
        Some(CropRegion {
            left: physical_x,
            top: physical_y,
            width: physical_width,
            height: physical_height,
        })
    } else {
        None
    };

    let output_size = pipeline_output_size(crop, buffer_width, buffer_height);
    let trailing_edge_crop = crop
        .as_ref()
        .is_some_and(|region| crop_touches_trailing_edge(region, buffer_width, buffer_height));
    let min_size = min_input_size(&requested_encoder_info.gst_element);
    let copied_path_reason = if trailing_edge_crop {
        Some("trailing-edge crop")
    } else if crop.is_some() && (output_size.0 < min_size.0 || output_size.1 < min_size.1) {
        Some("crop below the hardware encoder's minimum size")
    } else {
        None
    };
    let encoder_info = select_effective_encoder(
        requested_encoder_info,
        &encoders,
        container,
        copied_path_reason,
        output_size,
    );
    let encoder_info = substitute_h265_for_unaligned_width(encoder_info, &encoders, output_size.0);
    let zero_copy_allowed = copied_path_reason.is_none();

    log::info!(
        "Using encoder: {} ({:?}) for output {}x{}",
        encoder_info.gst_element,
        encoder_info.codec,
        output_size.0,
        output_size.1
    );

    if let Some(reason) = copied_path_reason {
        log::info!(
            "Disabling DMA-BUF zero-copy for {} {}x{}; using copied capture path",
            reason,
            output_size.0,
            output_size.1
        );
    }

    let zero_copy = if zero_copy_allowed {
        zero_copy_setup(&wayland_helper, &formats, &encoder_info)
    } else {
        None
    };

    // The buffers first: the tiler decides the padded width the pipeline's caps
    // have to describe.
    let mut buffer_pool = zero_copy.as_ref().and_then(|(ctx, format, modifier, _)| {
        match TripleBufferPool::new(ctx, buffer_width, buffer_height, *format, *modifier) {
            Ok(pool) => Some(pool),
            Err(e) => {
                log::warn!("Failed to allocate zero-copy buffers: {e}. Falling back to SHM.");
                None
            }
        }
    });
    let mut pipeline = None;
    if let (Some(pool), Some((_, _, modifier, import))) = (&buffer_pool, &zero_copy) {
        match Pipeline::new_dmabuf(
            &encoder_info,
            container,
            &output_file,
            buffer_width,
            buffer_height,
            pool.padded_width(),
            crop,
            framerate,
            *import,
            *modifier,
            system_audio,
            microphone,
        ) {
            Ok(p) => pipeline = Some(p),
            Err(err) => log::warn!(
                "Failed to create zero-copy DMA-BUF pipeline: {err}. Falling back to copied SHM path."
            ),
        }
    }
    if pipeline.is_none() {
        buffer_pool = None;
    }
    let pipeline = match pipeline {
        Some(pipeline) => pipeline,
        None => Pipeline::new(
            &encoder_info,
            container,
            &output_file,
            buffer_width,
            buffer_height,
            crop,
            framerate,
            system_audio,
            microphone,
        )
        .context("Failed to create GStreamer pipeline")?,
    };

    pipeline
        .start()
        .context("Failed to start GStreamer pipeline")?;

    let actual_dmabuf = buffer_pool.is_some();
    log::info!(
        "Recording with {} through the {} path",
        encoder_info.gst_element,
        if actual_dmabuf {
            "zero-copy DMA-BUF"
        } else {
            "copied SHM"
        }
    );

    // Main recording loop using SHM capture
    let frame_duration = Duration::from_secs_f64(1.0 / f64::from(framerate));
    let mut frame_count = 0u64;
    let mut consecutive_errors = 0u32;
    const MAX_CONSECUTIVE_ERRORS: u32 = 10;
    let start_time = Instant::now();
    let mut last_frame: Option<gst::Buffer> = None;

    if actual_dmabuf {
        while !stop.load(Ordering::Relaxed) {
            let frame_start = Instant::now();
            // Wall-clock timestamps, so a lagging capture yields a shorter video at the right speed.
            let timestamp = start_time.elapsed().as_nanos() as u64;

            let pool = buffer_pool.as_mut().unwrap();
            match capture_frame_dmabuf_triple(&wayland_helper, &session, pool, &pipeline, timestamp)
            {
                Ok(()) => {
                    consecutive_errors = 0;
                    frame_count += 1;
                }
                Err(e) => {
                    log::error!("Failed to capture/push frame: {e}");
                    consecutive_errors += 1;
                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        break;
                    }
                }
            }

            let frame_elapsed = frame_start.elapsed();
            if frame_elapsed < frame_duration {
                std::thread::sleep(frame_duration.checked_sub(frame_elapsed).unwrap());
            }
        }
    } else {
        let slots = (0..SHM_SLOTS)
            .map(|_| ShmSlot::new(&wayland_helper, buffer_width, buffer_height))
            .collect::<Result<Vec<_>>>()?;
        let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<gst::Buffer>(1);
        let stop_capture = std::sync::Arc::new(AtomicBool::new(false));
        let capture_thread = {
            let stop_capture = stop_capture.clone();
            std::thread::spawn(move || {
                shm_capture_loop(&session, slots, &frame_tx, &stop_capture, start_time);
            })
        };

        while !stop.load(Ordering::Relaxed) {
            match frame_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(buffer) => {
                    last_frame = Some(buffer.clone());
                    match pipeline.push_buffer(buffer) {
                        Ok(()) => {
                            consecutive_errors = 0;
                            frame_count += 1;
                            if frame_count.is_multiple_of(60) {
                                let fps = frame_count as f64 / start_time.elapsed().as_secs_f64();
                                log::info!("Recording: {frame_count} frames ({fps:.1} fps, shm)");
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to push frame: {e}");
                            consecutive_errors += 1;
                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                break;
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    log::error!("Capture thread stopped; ending the recording");
                    break;
                }
            }
        }

        // The capture thread may be blocked waiting for a frame the compositor
        // never sends. Give it a moment, then leave it to finish on its own.
        stop_capture.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + Duration::from_secs(1);
        while !capture_thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if capture_thread.is_finished() {
            let _ = capture_thread.join();
        } else {
            log::warn!("Capture thread still waiting on the compositor; not joining it");
        }
    }

    // A last frame stamped with the stop time, so the video runs to the moment
    // recording stopped rather than to the last change on screen.
    let stop_pts = start_time.elapsed().as_nanos() as u64;
    if frame_count > 0 {
        let tail = if actual_dmabuf {
            pipeline.push_dmabuf_frame(buffer_pool.as_ref().unwrap().last(), stop_pts)
        } else if let Some(last) = &last_frame {
            let mut tail = last.copy();
            tail.get_mut()
                .unwrap()
                .set_pts(gst::ClockTime::from_nseconds(stop_pts));
            pipeline.push_buffer(tail)
        } else {
            Ok(())
        };
        if let Err(e) = tail {
            log::warn!("Failed to push the closing frame: {e}");
        }
    }

    // Graceful shutdown
    log::info!("Stopping recording... ({frame_count} frames captured)");
    pipeline.finish()?;
    log::info!("Recording finished: {}", output_file.display());

    Ok(())
}
