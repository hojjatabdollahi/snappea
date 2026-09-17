// SPDX-License-Identifier: GPL-3.0-only

//! `GStreamer` pipeline construction.

use anyhow::{Context, Result};
use drm_fourcc::{DrmFourcc, DrmModifier};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_allocators as gst_allocators;
use gstreamer_allocators::DmaBufAllocatorExtManual;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use gstreamer_video::prelude::*;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use super::dmabuf::{
    DmabufBuffer, DmabufContext, byte_reversed, drm_format_to_gst_video_format, red_pixel,
};
use super::encoder::EncoderInfo;
use crate::config::Container;

/// Region to crop from captured frame
#[derive(Clone, Copy, Debug)]
pub struct CropRegion {
    /// Left offset in pixels
    pub left: u32,
    /// Top offset in pixels
    pub top: u32,
    /// Width of region
    pub width: u32,
    /// Height of region
    pub height: u32,
}

fn align_crop_axis(offset: u32, length: u32, capture_length: u32) -> (u32, u32) {
    if capture_length == 0 {
        return (0, 0);
    }

    let mut start = offset.min(capture_length.saturating_sub(1));
    let mut end = start.saturating_add(length.max(1)).min(capture_length);

    if capture_length > 1 && start % 2 == 1 {
        start -= 1;
    }

    if end <= start {
        end = (start + 1).min(capture_length);
    }

    // Even crop origins and sizes for hardware post-processing. Shrink inside the frame if needed.
    if capture_length > 1 && end % 2 == 1 {
        if end < capture_length {
            end += 1;
        } else if end > start + 1 {
            end -= 1;
        }
    }

    (start, end - start)
}

/// Clamp and align the crop region. Returns `(left, top, width, height, right, bottom)`,
/// where `right` and `bottom` are the amounts left over on those edges.
fn calculate_aligned_crop(
    region: &CropRegion,
    capture_width: u32,
    capture_height: u32,
) -> (u32, u32, u32, u32, u32, u32) {
    let requested_left = region.left.min(capture_width.saturating_sub(1));
    let requested_top = region.top.min(capture_height.saturating_sub(1));
    let requested_width = region
        .width
        .min(capture_width.saturating_sub(requested_left))
        .max(1);
    let requested_height = region
        .height
        .min(capture_height.saturating_sub(requested_top))
        .max(1);

    let (aligned_left, aligned_width) = align_crop_axis(region.left, region.width, capture_width);
    let (aligned_top, aligned_height) = align_crop_axis(region.top, region.height, capture_height);

    // Calculate right/bottom crop amounts
    // right = total_width - left - desired_output_width
    let right = capture_width
        .saturating_sub(aligned_left)
        .saturating_sub(aligned_width);
    let bottom = capture_height
        .saturating_sub(aligned_top)
        .saturating_sub(aligned_height);

    log::debug!(
        "Crop alignment: input {}x{}, region ({},{} {}x{}) -> requested ({},{} {}x{}) -> aligned ({},{} {}x{}), crop l={} t={} r={} b={}",
        capture_width,
        capture_height,
        region.left,
        region.top,
        region.width,
        region.height,
        requested_left,
        requested_top,
        requested_width,
        requested_height,
        aligned_left,
        aligned_top,
        aligned_width,
        aligned_height,
        aligned_left,
        aligned_top,
        right,
        bottom
    );

    (
        aligned_left,
        aligned_top,
        aligned_width,
        aligned_height,
        right,
        bottom,
    )
}

pub fn aligned_crop_output_size(
    region: &CropRegion,
    capture_width: u32,
    capture_height: u32,
) -> (u32, u32) {
    let (_, _, width, height, _, _) = calculate_aligned_crop(region, capture_width, capture_height);
    (width, height)
}

pub fn pipeline_output_size(
    crop: Option<CropRegion>,
    capture_width: u32,
    capture_height: u32,
) -> (u32, u32) {
    crop.as_ref()
        .map_or((capture_width, capture_height), |region| {
            aligned_crop_output_size(region, capture_width, capture_height)
        })
}

pub fn crop_touches_trailing_edge(
    region: &CropRegion,
    capture_width: u32,
    capture_height: u32,
) -> bool {
    let (_, _, _, _, right, bottom) = calculate_aligned_crop(region, capture_width, capture_height);
    right == 0 || bottom == 0
}

fn encoder_input_caps(encoder: &EncoderInfo, width: u32, height: u32, framerate: u32) -> gst::Caps {
    let mut caps = gst::Caps::builder("video/x-raw")
        .field("width", width as i32)
        .field("height", height as i32)
        .field("framerate", gst::Fraction::new(framerate as i32, 1))
        .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
        .field("colorimetry", "bt709");

    if !encoder.hardware {
        caps = caps.field("format", "I420");
    }

    caps.build()
}

/// The DMA-BUF formats and modifiers `vapostproc` can import, read from its sink
/// template. The `va` plugin fills that in from the driver at registration, so
/// this is per GPU. Empty when the element is missing.
pub fn vapostproc_dmabuf_formats() -> Vec<(DrmFourcc, Vec<DrmModifier>)> {
    let Some(factory) = gst::ElementFactory::find("vapostproc") else {
        return Vec::new();
    };
    let mut out: Vec<(DrmFourcc, Vec<DrmModifier>)> = Vec::new();
    for template in factory.static_pad_templates() {
        if template.direction() != gst::PadDirection::Sink {
            continue;
        }
        for (s, features) in template.caps().iter_with_features() {
            if !features.contains("memory:DMABuf") || s.get::<&str>("format") != Ok("DMA_DRM") {
                continue;
            }
            let Ok(value) = s.value("drm-format") else {
                continue;
            };
            let entries: Vec<String> = value.get::<gst::List>().map_or_else(
                |_| value.get::<String>().into_iter().collect(),
                |list| list.iter().filter_map(|v| v.get::<String>().ok()).collect(),
            );
            for entry in entries {
                let Some((fourcc, modifier)) = parse_drm_format(&entry) else {
                    continue;
                };
                match out.iter_mut().find(|(f, _)| *f == fourcc) {
                    Some((_, mods)) => {
                        if !mods.contains(&modifier) {
                            mods.push(modifier);
                        }
                    }
                    None => out.push((fourcc, vec![modifier])),
                }
            }
        }
    }
    out
}

/// `XR24` or `XR24:0x0200000010401b04`, as `GStreamer` prints DRM formats. No
/// modifier means linear.
fn parse_drm_format(s: &str) -> Option<(DrmFourcc, DrmModifier)> {
    let (fourcc, modifier) = s.split_once(':').unwrap_or((s, "0x0"));
    let fourcc = u32::from_le_bytes(fourcc.as_bytes().try_into().ok()?);
    let fourcc = DrmFourcc::try_from(fourcc).ok()?;
    let modifier = u64::from_str_radix(modifier.strip_prefix("0x")?, 16).ok()?;
    Some((fourcc, DrmModifier::from(modifier)))
}

/// Tune `elem` for live screen capture: constant quality, no B-frames, a
/// keyframe every two seconds, and the fastest presets. Every property is set
/// by name only if the element has it, since names differ across plugins and
/// versions.
fn configure_encoder(elem: &gst::Element, encoder: &EncoderInfo, framerate: u32) {
    let keyint = (framerate * 2).to_string();
    let settings: &[(&str, &str)] = match encoder.gst_element.as_str() {
        "x264enc" => &[
            ("speed-preset", "ultrafast"),
            ("tune", "zerolatency"),
            ("pass", "qual"),
            ("quantizer", "22"),
            ("bframes", "0"),
            ("key-int-max", &keyint),
        ],
        name if name.starts_with("va") => &[
            ("rate-control", "cqp"),
            ("qpi", "22"),
            ("qpp", "22"),
            ("qpb", "22"),
            ("qp", "22"),
            ("b-frames", "0"),
            // 1 is best quality, 7 fastest.
            ("target-usage", "6"),
            ("key-int-max", &keyint),
        ],
        name if name.starts_with("nv") => &[
            ("preset", "low-latency-hq"),
            ("zerolatency", "true"),
            ("bframes", "0"),
            ("gop-size", &keyint),
        ],
        _ => &[],
    };
    for (name, value) in settings {
        if elem.has_property(name) {
            elem.set_property_from_str(name, value);
        } else {
            log::debug!("{} has no `{name}` property; skipped", encoder.gst_element);
        }
    }
}

/// A short leaky queue after `appsrc`, so a slow encoder drops frames instead
/// of stalling capture.
fn frame_queue() -> Result<gst::Element> {
    gst::ElementFactory::make("queue")
        .property("max-size-buffers", 3u32)
        .property("max-size-bytes", 0u32)
        .property("max-size-time", 0u64)
        .property_from_str("leaky", "downstream")
        .build()
        .context("Failed to create queue element")
}

/// The container muxer. MP4 is written fragmented so a crash mid-recording
/// still leaves a playable file.
fn make_muxer(container: Container) -> Result<gst::Element> {
    let muxer = gst::ElementFactory::make(container.muxer_element())
        .build()
        .with_context(|| format!("Failed to create muxer: {}", container.muxer_element()))?;
    if container == Container::Mp4 && muxer.has_property("fragment-mode") {
        muxer.set_property("fragment-duration", 500u32);
        muxer.set_property_from_str("fragment-mode", "first-moov-then-finalise");
    }
    Ok(muxer)
}

/// The VA encoders in `GStreamer` 1.24 ignore the timestamps they are given and
/// stamp output as `frame index / framerate`, on a timeline they shift by a
/// large constant (and announce in their segment). Screen capture is
/// damage-driven, so that plays a mostly static desktop back at fast-forward.
/// Remember each input PTS and put it back on the matching output buffer,
/// keeping the encoder's shift. With no B-frames the order is unchanged, so DTS
/// is the same value.
/// Workaround for the va plugin. Drop once the encoders keep PTS.
fn restamp_from_input(encoder: &gst::Element) -> Result<()> {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct State {
        input: VecDeque<gst::ClockTime>,
        /// Encoder output timeline minus ours, taken from the first frame.
        shift: Option<gst::ClockTime>,
    }
    let state: Arc<Mutex<State>> = Arc::default();
    let sink = encoder
        .static_pad("sink")
        .context("encoder has no sink pad")?;
    let src = encoder
        .static_pad("src")
        .context("encoder has no src pad")?;
    let s = state.clone();
    sink.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
        if let Some(gst::PadProbeData::Buffer(buffer)) = &info.data
            && let Some(pts) = buffer.pts()
        {
            s.lock().unwrap().input.push_back(pts);
        }
        gst::PadProbeReturn::Ok
    });
    src.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
        if let Some(gst::PadProbeData::Buffer(buffer)) = &mut info.data {
            let mut s = state.lock().unwrap();
            if let (Some(input), Some(out)) = (s.input.pop_front(), buffer.pts()) {
                let shift = *s.shift.get_or_insert_with(|| out.saturating_sub(input));
                let pts = input + shift;
                let buffer = buffer.make_mut();
                buffer.set_pts(pts);
                buffer.set_dts(pts);
            }
        }
        gst::PadProbeReturn::Ok
    });
    Ok(())
}

/// `GStreamer` pipeline for encoding screen capture to video file
pub struct Pipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    output_path: PathBuf,
    /// Whether this pipeline is configured for DMA-buf input
    dmabuf_mode: bool,
    dmabuf_allocator: Option<gst_allocators::DmaBufAllocator>,
    /// `(x, y, w, h)` attached to every DMA-BUF frame. `vapostproc` crops to it.
    crop_meta: Option<(u32, u32, u32, u32)>,
    /// How DMA-BUF frames are described to the importer. May differ from their
    /// memory layout, see [`probe_import_fourcc`].
    import_format: Option<gst_video::VideoFormat>,
}

/// Wrap a DMA-BUF as a `GstBuffer` described as `format`, without copying.
fn dmabuf_gst_buffer(
    allocator: &gst_allocators::DmaBufAllocator,
    dmabuf: &DmabufBuffer,
    format: gst_video::VideoFormat,
    timestamp: u64,
) -> Result<gst::Buffer> {
    let owned_fd = unsafe { libc::dup(dmabuf.fd.as_raw_fd()) };
    if owned_fd < 0 {
        return Err(anyhow::anyhow!("Failed to dup DMA-BUF fd for GStreamer"));
    }
    let memory = unsafe { allocator.alloc_dmabuf(owned_fd, dmabuf.size) }
        .context("Failed to wrap DMA-BUF fd as GStreamer memory")?;
    let mut buffer = gst::Buffer::new();
    {
        let buffer_mut = buffer.get_mut().unwrap();
        buffer_mut.append_memory(memory);
        buffer_mut.set_pts(gst::ClockTime::from_nseconds(timestamp));
        gst_video::VideoMeta::add_full(
            buffer_mut,
            gst_video::VideoFrameFlags::empty(),
            format,
            dmabuf.padded_width,
            dmabuf.height,
            &[dmabuf.offset as usize],
            &[dmabuf.stride as i32],
        )
        .context("Failed to attach video metadata to DMA-BUF buffer")?;
    }
    Ok(buffer)
}

/// `appsrc` caps for DMA-BUF frames: `vapostproc` only imports them as `DMA_DRM`,
/// with the fourcc and modifier in the caps and the plane layout in `VideoMeta`.
fn dmabuf_caps(
    format: gst_video::VideoFormat,
    fourcc: DrmFourcc,
    modifier: DrmModifier,
    width: u32,
    height: u32,
    framerate: u32,
) -> Result<gst::Caps> {
    let info = gst_video::VideoInfo::builder(format, width, height)
        .fps(gst::Fraction::new(framerate as i32, 1))
        .build()
        .context("Invalid video info for the DMA-BUF caps")?;
    gst_video::VideoInfoDmaDrm::new(info, fourcc as u32, u64::from(modifier))
        .to_caps()
        .context("Failed to build DMA-BUF caps")
}

/// How to describe an RGB DMA-BUF laid out as `memory` so the VA driver reads it
/// correctly. Some drivers (radeonsi on AMD, at least) read every imported
/// fourcc byte-reversed, so a buffer the compositor fills as AR24 has to be
/// called BA24. Fills a small buffer red and looks at what `vapostproc` makes
/// of it under each description.
pub fn probe_import_fourcc(
    ctx: &DmabufContext,
    memory: DrmFourcc,
    modifier: DrmModifier,
    encoder: &EncoderInfo,
) -> Result<DrmFourcc> {
    gst::init().context("Failed to initialize GStreamer")?;
    let importer = vapostproc_dmabuf_formats();
    let accepted = |f: DrmFourcc| {
        importer
            .iter()
            .any(|(g, mods)| *g == f && mods.contains(&modifier))
    };
    let red = red_pixel(memory).context("not an RGB format")?;
    let buffer = ctx.allocate_filled(128, 128, memory, modifier, red)?;
    for candidate in std::iter::once(memory).chain(byte_reversed(memory)) {
        if !accepted(candidate) {
            continue;
        }
        match import_reads_red(&buffer, candidate, modifier, encoder) {
            Ok(true) => return Ok(candidate),
            Ok(false) => log::info!("DMA-BUF import as {candidate:?} reads wrong colors"),
            Err(e) => log::warn!("DMA-BUF import probe as {candidate:?} failed: {e:#}"),
        }
    }
    anyhow::bail!("no fourcc description makes vapostproc read a {memory:?} DMA-BUF correctly")
}

/// Push `buffer` through the zero-copy encode path described as `fourcc`, decode
/// the result in software, and check that the center pixel comes back red. The
/// detour through the encoder is deliberate: downloading an imported tiled
/// buffer straight out of `vapostproc` crashes the AMD driver.
fn import_reads_red(
    buffer: &DmabufBuffer,
    fourcc: DrmFourcc,
    modifier: DrmModifier,
    encoder: &EncoderInfo,
) -> Result<bool> {
    let format = drm_format_to_gst_video_format(fourcc).context("unsupported fourcc")?;
    let decoder = encoder
        .codec
        .software_decoder()
        .with_context(|| format!("no software decoder for {:?}", encoder.codec))?;
    let appsrc = gst_app::AppSrc::builder()
        .caps(&dmabuf_caps(
            format,
            fourcc,
            modifier,
            buffer.padded_width,
            buffer.height,
            30,
        )?)
        .format(gst::Format::Time)
        .build();
    let make = |name: &str| {
        gst::ElementFactory::make(name)
            .build()
            .with_context(|| format!("Failed to create {name}"))
    };
    let vapostproc = make("vapostproc")?;
    let to_encoder = gst::ElementFactory::make("capsfilter")
        .property(
            "caps",
            gst::Caps::from_iter([(
                gst::Structure::builder("video/x-raw")
                    .field("format", "NV12")
                    .build(),
                gst::CapsFeatures::new(["memory:VAMemory"]),
            )]),
        )
        .build()?;
    let encoder_elem = make(&encoder.gst_element)?;
    let parser = encoder.codec.parser_element().map(make).transpose()?;
    let decoder = make(decoder)?;
    // Software decoders emit I420. Read NV12 regardless of the codec.
    let convert = make("videoconvert")?;
    let appsink = gst_app::AppSink::builder()
        .caps(
            &gst::Caps::builder("video/x-raw")
                .field("format", "NV12")
                .build(),
        )
        .sync(false)
        .build();
    let pipeline = gst::Pipeline::new();
    let mut chain: Vec<&gst::Element> =
        vec![appsrc.upcast_ref(), &vapostproc, &to_encoder, &encoder_elem];
    chain.extend(parser.as_ref());
    chain.extend([&decoder, &convert, appsink.upcast_ref()]);
    pipeline.add_many(chain.iter().copied())?;
    gst::Element::link_many(chain.iter().copied())?;
    pipeline.set_state(gst::State::Playing)?;
    let result = (|| {
        let allocator = gst_allocators::DmaBufAllocator::new();
        for i in 0..3u64 {
            appsrc
                .push_buffer(dmabuf_gst_buffer(
                    &allocator,
                    buffer,
                    format,
                    i * 33_333_333,
                )?)
                .map_err(|e| anyhow::anyhow!("push failed: {e:?}"))?;
        }
        let _ = appsrc.end_of_stream();
        let sample = appsink
            .try_pull_sample(gst::ClockTime::from_seconds(3))
            .ok_or_else(|| {
                let error = pipeline
                    .bus()
                    .and_then(|bus| bus.pop_filtered(&[gst::MessageType::Error]))
                    .and_then(|msg| match msg.view() {
                        gst::MessageView::Error(e) => {
                            Some(format!("{} ({})", e.error(), e.debug().unwrap_or_default()))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| "no error on the bus".into());
                anyhow::anyhow!("probe pipeline produced no frame: {error}")
            })?;
        let caps = sample.caps().context("sample without caps")?;
        let info = gst_video::VideoInfo::from_caps(caps)?;
        let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(
            sample.buffer().context("sample without buffer")?,
            &info,
        )?;
        let (x, y) = (buffer.width as usize / 2, buffer.height as usize / 2);
        let luma = frame.plane_data(0)?[y * frame.plane_stride()[0] as usize + x];
        let chroma = frame.plane_data(1)?;
        let at = (y / 2) * frame.plane_stride()[1] as usize + (x / 2) * 2;
        let (u, v) = (chroma[at], chroma[at + 1]);
        log::debug!("DMA-BUF import as {fourcc:?}: red reads back as YUV ({luma}, {u}, {v})");
        // Red is dark, blue-poor and very red-different: roughly (81, 90, 240) in BT.601.
        Ok(luma < 130 && u < 110 && v > 200)
    })();
    let _ = pipeline.set_state(gst::State::Null);
    result
}

impl Pipeline {
    /// Create an encoding pipeline for CPU frames of `capture_width` x `capture_height`.
    pub fn new(
        encoder: &EncoderInfo,
        container: Container,
        output_path: &Path,
        capture_width: u32,
        capture_height: u32,
        crop: Option<CropRegion>,
        framerate: u32,
    ) -> Result<Self> {
        gst::init().context("Failed to initialize GStreamer")?;

        let pipeline = gst::Pipeline::new();

        // Create elements
        let appsrc = gst_app::AppSrc::builder()
            .name("screen-source")
            .is_live(true)
            .format(gst::Format::Time)
            .build();
        let queue = frame_queue()?;

        let videoconvert = gst::ElementFactory::make("videoconvert")
            .build()
            .context("Failed to create videoconvert element")?;

        // Add videoscale to handle dimension alignment for hardware encoders
        let videoscale = gst::ElementFactory::make("videoscale")
            .build()
            .context("Failed to create videoscale element")?;

        let encoder_elem = gst::ElementFactory::make(&encoder.gst_element)
            .build()
            .with_context(|| format!("Failed to create encoder: {}", encoder.gst_element))?;
        configure_encoder(&encoder_elem, encoder, framerate);
        if encoder.gst_element.starts_with("va") {
            restamp_from_input(&encoder_elem)?;
        }

        // Parser between encoder and muxer, where the codec needs one.
        let parser_elem = encoder
            .codec
            .parser_element()
            .map(|p| {
                gst::ElementFactory::make(p)
                    .build()
                    .with_context(|| format!("Failed to create {p} element"))
            })
            .transpose()?;

        let muxer = make_muxer(container)?;

        let filesink = gst::ElementFactory::make("filesink")
            .property("location", output_path.to_str().unwrap())
            .build()
            .context("Failed to create filesink element")?;

        // Add cropping element if needed
        if let Some(ref region) = crop {
            let (clamped_left, clamped_top, clamped_width, clamped_height, right, bottom) =
                calculate_aligned_crop(region, capture_width, capture_height);

            log::info!(
                "Crop region requested: ({}, {}, {}x{}), clamped to capture {}x{}: ({}, {}, {}x{}) [even-aligned]",
                region.left,
                region.top,
                region.width,
                region.height,
                capture_width,
                capture_height,
                clamped_left,
                clamped_top,
                clamped_width,
                clamped_height
            );

            let videocrop = gst::ElementFactory::make("videocrop")
                .property("left", clamped_left as i32)
                .property("top", clamped_top as i32)
                .property("right", right as i32)
                .property("bottom", bottom as i32)
                .build()
                .context("Failed to create videocrop element")?;

            log::info!(
                "Adding crop: left={clamped_left}, top={clamped_top}, right={right}, bottom={bottom} (output: {clamped_width}x{clamped_height})"
            );

            // Create capsfilter to enforce exact output dimensions
            // This ensures the encoder receives the exact even-aligned dimensions
            let scale_caps = encoder_input_caps(encoder, clamped_width, clamped_height, framerate);
            let capsfilter = gst::ElementFactory::make("capsfilter")
                .property("caps", &scale_caps)
                .build()
                .context("Failed to create capsfilter element")?;

            // Add elements to pipeline
            pipeline.add_many([
                appsrc.upcast_ref(),
                &queue,
                &videocrop,
                &videoconvert,
                &videoscale,
                &capsfilter,
                &encoder_elem,
            ])?;
            if let Some(ref parser) = parser_elem {
                pipeline.add(parser)?;
            }
            pipeline.add_many([&muxer, &filesink])?;

            // Link elements with crop
            gst::Element::link_many([
                appsrc.upcast_ref(),
                &queue,
                &videocrop,
                &videoconvert,
                &videoscale,
                &capsfilter,
                &encoder_elem,
            ])?;
            if let Some(ref parser) = parser_elem {
                encoder_elem.link(parser)?;
                parser.link(&muxer)?;
            } else {
                encoder_elem.link(&muxer)?;
            }
            muxer.link(&filesink)?;
        } else {
            let encoder_caps =
                encoder_input_caps(encoder, capture_width, capture_height, framerate);
            let capsfilter = gst::ElementFactory::make("capsfilter")
                .property("caps", &encoder_caps)
                .build()
                .context("Failed to create capsfilter element")?;

            // Add elements to pipeline without crop
            pipeline.add_many([
                appsrc.upcast_ref(),
                &queue,
                &videoconvert,
                &videoscale,
                &capsfilter,
                &encoder_elem,
            ])?;
            if let Some(ref parser) = parser_elem {
                pipeline.add(parser)?;
            }
            pipeline.add_many([&muxer, &filesink])?;

            // Link elements
            gst::Element::link_many([
                appsrc.upcast_ref(),
                &queue,
                &videoconvert,
                &videoscale,
                &capsfilter,
                &encoder_elem,
            ])?;
            if let Some(ref parser) = parser_elem {
                encoder_elem.link(parser)?;
                parser.link(&muxer)?;
            } else {
                encoder_elem.link(&muxer)?;
            }
            muxer.link(&filesink)?;
        }

        // Configure appsrc caps (raw RGBA video at capture size)
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("width", capture_width as i32)
            .field("height", capture_height as i32)
            .field("framerate", gst::Fraction::new(framerate as i32, 1))
            .build();
        appsrc.set_caps(Some(&caps));

        Ok(Self {
            pipeline,
            appsrc,
            output_path: output_path.to_path_buf(),
            dmabuf_mode: false,
            dmabuf_allocator: None,
            crop_meta: None,
            import_format: None,
        })
    }

    /// Create an encoding pipeline fed from DMA-buf file descriptors, without CPU copies.
    pub fn new_dmabuf(
        encoder: &EncoderInfo,
        container: Container,
        output_path: &Path,
        capture_width: u32,
        capture_height: u32,
        padded_width: u32,
        crop: Option<CropRegion>,
        framerate: u32,
        import_fourcc: DrmFourcc,
        modifier: DrmModifier,
    ) -> Result<Self> {
        gst::init().context("Failed to initialize GStreamer")?;

        if !encoder.supports_dmabuf_zero_copy {
            anyhow::bail!(
                "Encoder {} is not wired for the real DMA-BUF zero-copy path",
                encoder.gst_element
            );
        }

        let video_format = drm_format_to_gst_video_format(import_fourcc).ok_or_else(|| {
            anyhow::anyhow!("Unsupported DRM format for GStreamer: {import_fourcc:?}")
        })?;

        let output_size = pipeline_output_size(crop, capture_width, capture_height);
        log::info!(
            "Creating DMA-buf pipeline: capture {}x{}, output {}x{} @ {} fps, format={:?}:{:?} ({:?})",
            capture_width,
            capture_height,
            output_size.0,
            output_size.1,
            framerate,
            import_fourcc,
            modifier,
            video_format
        );

        let pipeline = gst::Pipeline::new();
        let dmabuf_allocator = gst_allocators::DmaBufAllocator::new();

        // Create elements
        let appsrc = gst_app::AppSrc::builder()
            .name("screen-source")
            .is_live(true)
            .format(gst::Format::Time)
            .build();
        let queue = frame_queue()?;

        // Imports the DMA-BUF, crops (from the buffers' `VideoCropMeta`) and converts
        // to NV12 on the GPU.
        let vapostproc = gst::ElementFactory::make("vapostproc")
            .build()
            .context("Failed to create vapostproc element for zero-copy path")?;

        let encoder_elem = gst::ElementFactory::make(&encoder.gst_element)
            .build()
            .with_context(|| format!("Failed to create encoder: {}", encoder.gst_element))?;
        configure_encoder(&encoder_elem, encoder, framerate);
        if encoder.gst_element.starts_with("va") {
            restamp_from_input(&encoder_elem)?;
        }

        // Parser between encoder and muxer, where the codec needs one.
        let parser_elem = encoder
            .codec
            .parser_element()
            .map(|p| {
                gst::ElementFactory::make(p)
                    .build()
                    .with_context(|| format!("Failed to create {p} element"))
            })
            .transpose()?;

        let muxer = make_muxer(container)?;

        let filesink = gst::ElementFactory::make("filesink")
            .property("location", output_path.to_str().unwrap())
            .build()
            .context("Failed to create filesink element")?;

        let (clamped_left, clamped_top, clamped_width, clamped_height, right, bottom) = crop
            .as_ref()
            .map_or((0, 0, capture_width, capture_height, 0, 0), |region| {
                calculate_aligned_crop(region, capture_width, capture_height)
            });

        // Always crop: at the least it trims the tiler's padding off the right edge.
        log::info!(
            "Zero-copy crop: left={clamped_left}, top={clamped_top}, right={right}, bottom={bottom} (output {clamped_width}x{clamped_height}, buffer {padded_width}x{capture_height})"
        );
        let crop_meta = Some((clamped_left, clamped_top, clamped_width, clamped_height));

        let input_caps = dmabuf_caps(
            video_format,
            import_fourcc,
            modifier,
            padded_width,
            capture_height,
            framerate,
        )?;
        appsrc.set_caps(Some(&input_caps));

        // Square pixels, or `vapostproc` keeps the uncropped frame's display aspect
        // by stretching the crop's pixel aspect ratio.
        let output_structure = gst::Structure::builder("video/x-raw")
            .field("format", "NV12")
            .field("width", clamped_width as i32)
            .field("height", clamped_height as i32)
            .field("framerate", gst::Fraction::new(framerate as i32, 1))
            .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
            .field("colorimetry", "bt709")
            .build();
        let output_caps: gst::Caps = [(
            output_structure,
            gst::CapsFeatures::new(["memory:VAMemory"]),
        )]
        .into();
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", &output_caps)
            .build()
            .context("Failed to create zero-copy capsfilter element")?;

        log::info!("Zero-copy input caps: {input_caps}");
        log::info!("Zero-copy output caps (postproc->encoder): {output_caps}");

        pipeline.add_many([
            appsrc.upcast_ref(),
            &queue,
            &vapostproc,
            &capsfilter,
            &encoder_elem,
        ])?;
        if let Some(ref parser) = parser_elem {
            pipeline.add(parser)?;
        }
        pipeline.add_many([&muxer, &filesink])?;

        gst::Element::link_many([
            appsrc.upcast_ref(),
            &queue,
            &vapostproc,
            &capsfilter,
            &encoder_elem,
        ])?;
        if let Some(ref parser) = parser_elem {
            encoder_elem.link(parser)?;
            parser.link(&muxer)?;
        } else {
            encoder_elem.link(&muxer)?;
        }
        muxer.link(&filesink)?;

        Ok(Self {
            pipeline,
            appsrc,
            output_path: output_path.to_path_buf(),
            dmabuf_mode: true,
            dmabuf_allocator: Some(dmabuf_allocator),
            crop_meta,
            import_format: Some(video_format),
        })
    }

    /// Check if this pipeline is configured for DMA-buf input
    pub const fn is_dmabuf_mode(&self) -> bool {
        self.dmabuf_mode
    }

    /// Start the pipeline
    pub fn start(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Playing)
            .context("Failed to start pipeline")?;
        Ok(())
    }

    /// Push a ready-made buffer. Its PTS must be set.
    pub fn push_buffer(&self, buffer: gst::Buffer) -> Result<()> {
        self.appsrc
            .push_buffer(buffer)
            .map_err(|_| anyhow::anyhow!("Failed to push buffer to pipeline"))?;
        Ok(())
    }

    /// Push a DMA-buf frame to the pipeline without copying it into system memory.
    pub fn push_dmabuf_frame(&self, dmabuf: &DmabufBuffer, timestamp: u64) -> Result<()> {
        let allocator = self
            .dmabuf_allocator
            .as_ref()
            .context("DMA-BUF allocator not available for zero-copy pipeline")?;
        let format = self
            .import_format
            .context("pipeline is not in DMA-BUF mode")?;
        let mut buffer = dmabuf_gst_buffer(allocator, dmabuf, format, timestamp)?;
        if let Some(rect) = self.crop_meta {
            gst_video::VideoCropMeta::add(buffer.get_mut().unwrap(), rect);
        }
        self.appsrc
            .push_buffer(buffer)
            .map_err(|_| anyhow::anyhow!("Failed to push buffer to pipeline"))?;
        Ok(())
    }

    /// Signal end of stream and finalize the video file
    pub fn finish(&self) -> Result<()> {
        log::info!("Sending EOS signal to pipeline...");
        self.appsrc
            .end_of_stream()
            .map_err(|_| anyhow::anyhow!("Failed to send EOS"))?;

        // Wait for EOS to propagate through pipeline (30 seconds for long recordings)
        log::info!("Waiting for pipeline to finish (up to 30 seconds)...");
        let bus = self.pipeline.bus().unwrap();
        let mut eos_received = false;
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(30)) {
            use gst::MessageView;
            match msg.view() {
                MessageView::Eos(..) => {
                    log::info!("EOS received, finalizing...");
                    eos_received = true;
                    break;
                }
                MessageView::Error(err) => {
                    return Err(anyhow::anyhow!(
                        "Pipeline error: {} ({})",
                        err.error(),
                        err.debug().unwrap_or_default()
                    ));
                }
                MessageView::StateChanged(state_change)
                    if state_change
                        .src()
                        .is_some_and(|s| s.name().as_str() == "pipeline0") =>
                {
                    log::debug!(
                        "Pipeline state changed: {:?} -> {:?}",
                        state_change.old(),
                        state_change.current()
                    );
                }
                _ => {}
            }
        }

        if !eos_received {
            log::warn!("EOS timeout reached, forcing pipeline shutdown");
        }

        self.pipeline
            .set_state(gst::State::Null)
            .context("Failed to stop pipeline")?;

        // Verify output file exists and has data
        self.verify_output()?;

        Ok(())
    }

    /// Verify that the output file exists and has data
    fn verify_output(&self) -> Result<()> {
        if !self.output_path.exists() {
            return Err(anyhow::anyhow!(
                "Output file was not created: {}",
                self.output_path.display()
            ));
        }

        let metadata = std::fs::metadata(&self.output_path).with_context(|| {
            format!(
                "Failed to read output file metadata: {}",
                self.output_path.display()
            )
        })?;

        if metadata.len() == 0 {
            return Err(anyhow::anyhow!(
                "Output file is empty: {}",
                self.output_path.display()
            ));
        }

        log::info!(
            "Output file verified: {} ({} bytes)",
            self.output_path.display(),
            metadata.len()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual check that the copied path keeps the timestamps it is given: 30
    /// frames 100 ms apart must make a 3 s file, not a half-second one.
    #[test]
    #[ignore = "needs GStreamer with x264enc"]
    fn copied_path_keeps_timestamps() {
        gst::init().unwrap();
        let encoder = EncoderInfo {
            name: "x264".into(),
            gst_element: "x264enc".into(),
            codec: super::super::encoder::Codec::H264,
            hardware: false,
            supports_dmabuf_zero_copy: false,
            priority: 100,
        };
        let pipeline = Pipeline::new(
            &encoder,
            Container::Mp4,
            std::path::Path::new("/tmp/cosmic-x-copied-pts.mp4"),
            128,
            128,
            None,
            60,
        )
        .unwrap();
        pipeline.start().unwrap();
        let frame = vec![0x80u8; 128 * 128 * 4];
        for i in 0..30u64 {
            let mut buffer = gst::Buffer::from_slice(frame.clone());
            buffer
                .get_mut()
                .unwrap()
                .set_pts(gst::ClockTime::from_nseconds(i * 100_000_000));
            pipeline.push_buffer(buffer).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        pipeline.finish().unwrap();
    }

    #[test]
    fn parses_gstreamer_drm_format_strings() {
        assert_eq!(
            parse_drm_format("XR24:0x0200000010401b04"),
            Some((
                DrmFourcc::Xrgb8888,
                DrmModifier::from(0x0200_0000_1040_1b04)
            ))
        );
        assert_eq!(
            parse_drm_format("YUYV"),
            Some((DrmFourcc::Yuyv, DrmModifier::Linear))
        );
        assert_eq!(parse_drm_format("nonsense"), None);
    }

    #[test]
    fn odd_width_expands() {
        let region = CropRegion {
            left: 10,
            top: 5,
            width: 1,
            height: 20,
        };

        assert_eq!(
            calculate_aligned_crop(&region, 100, 100),
            (10, 4, 2, 22, 88, 74)
        );
    }

    #[test]
    fn odd_origin_expands() {
        let region = CropRegion {
            left: 11,
            top: 13,
            width: 20,
            height: 30,
        };

        assert_eq!(
            calculate_aligned_crop(&region, 100, 100),
            (10, 12, 22, 32, 68, 56)
        );
    }

    #[test]
    fn right_edge_pixel_shifts() {
        let region = CropRegion {
            left: 99,
            top: 5,
            width: 1,
            height: 20,
        };

        assert_eq!(
            calculate_aligned_crop(&region, 100, 100),
            (98, 4, 2, 22, 0, 74)
        );
    }

    #[test]
    fn bottom_edge_pixel_shifts() {
        let region = CropRegion {
            left: 10,
            top: 99,
            width: 20,
            height: 1,
        };

        assert_eq!(
            calculate_aligned_crop(&region, 100, 100),
            (10, 98, 20, 2, 70, 0)
        );
    }

    #[test]
    fn trailing_edge_at_bottom() {
        let region = CropRegion {
            left: 10,
            top: 99,
            width: 20,
            height: 1,
        };

        assert!(crop_touches_trailing_edge(&region, 100, 100));
    }

    #[test]
    fn trailing_edge_ignores_inner() {
        let region = CropRegion {
            left: 10,
            top: 12,
            width: 40,
            height: 30,
        };

        assert!(!crop_touches_trailing_edge(&region, 100, 100));
    }

    #[test]
    fn software_encoder_forces_i420() {
        gst::init().unwrap();

        let encoder = EncoderInfo {
            name: "x264 H.264".to_string(),
            gst_element: "x264enc".to_string(),
            codec: super::super::encoder::Codec::H264,
            hardware: false,
            supports_dmabuf_zero_copy: false,
            priority: 100,
        };

        let caps = encoder_input_caps(&encoder, 626, 94, 30).to_string();
        assert!(caps.contains("format=(string)I420"));
        assert!(caps.contains("width=(int)626"));
        assert!(caps.contains("height=(int)94"));
    }

    #[test]
    fn hardware_encoder_keeps_format() {
        gst::init().unwrap();

        let encoder = EncoderInfo {
            name: "VA-API H.264".to_string(),
            gst_element: "vah264enc".to_string(),
            codec: super::super::encoder::Codec::H264,
            hardware: true,
            supports_dmabuf_zero_copy: true,
            priority: 10,
        };

        let caps = encoder_input_caps(&encoder, 626, 94, 30).to_string();
        assert!(!caps.contains("format=(string)I420"));
    }

    #[test]
    fn overflow_clamped_at_edge() {
        let region = CropRegion {
            left: 99,
            top: 99,
            width: 20,
            height: 20,
        };

        assert_eq!(
            calculate_aligned_crop(&region, 100, 100),
            (98, 98, 2, 2, 0, 0)
        );
    }

    #[test]
    fn even_region_unchanged() {
        let region = CropRegion {
            left: 10,
            top: 12,
            width: 40,
            height: 30,
        };

        assert_eq!(
            calculate_aligned_crop(&region, 100, 100),
            (10, 12, 40, 30, 50, 58)
        );
    }
}
