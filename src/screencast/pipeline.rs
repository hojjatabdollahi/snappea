//! GStreamer pipeline construction and management

use anyhow::{Context, Result};
use drm_fourcc::DrmFourcc;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_allocators as gst_allocators;
use gstreamer_allocators::DmaBufAllocatorExtManual;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use super::dmabuf::{DmabufBuffer, drm_format_to_gst_format, drm_format_to_gst_video_format};
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
    if end <= start {
        end = (start + 1).min(capture_length);
    }

    // Prefer even encoded dimensions. If an odd-sized crop hits the far edge,
    // shift it inward instead of advertising a size the crop cannot produce.
    if capture_length > 1 && ((end - start) & 1) == 1 {
        if end < capture_length {
            end += 1;
        } else if start > 0 {
            start -= 1;
        } else if end - start > 1 {
            end -= 1;
        }
    }

    (start, end - start)
}

/// Calculate clamped and aligned crop parameters for video encoding
/// Returns (left, top, width, height, right, bottom) where right/bottom are the amounts to crop from those edges
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

pub(crate) fn aligned_crop_output_size(
    region: &CropRegion,
    capture_width: u32,
    capture_height: u32,
) -> (u32, u32) {
    let (_, _, width, height, _, _) = calculate_aligned_crop(region, capture_width, capture_height);
    (width, height)
}

pub(crate) fn pipeline_output_size(
    crop: Option<CropRegion>,
    capture_width: u32,
    capture_height: u32,
) -> (u32, u32) {
    crop.as_ref()
        .map(|region| aligned_crop_output_size(region, capture_width, capture_height))
        .unwrap_or((capture_width, capture_height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_alignment_expands_odd_width_inside_bounds() {
        let region = CropRegion {
            left: 10,
            top: 5,
            width: 1,
            height: 20,
        };

        assert_eq!(
            calculate_aligned_crop(&region, 100, 100),
            (10, 5, 2, 20, 88, 75)
        );
    }

    #[test]
    fn crop_alignment_shifts_one_pixel_selection_at_right_edge() {
        let region = CropRegion {
            left: 99,
            top: 5,
            width: 1,
            height: 20,
        };

        assert_eq!(
            calculate_aligned_crop(&region, 100, 100),
            (98, 5, 2, 20, 0, 75)
        );
    }

    #[test]
    fn crop_alignment_shifts_one_pixel_selection_at_bottom_edge() {
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
    fn crop_alignment_handles_region_overflow_at_edge() {
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
    fn crop_alignment_preserves_even_region_inside_bounds() {
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

/// GStreamer pipeline for encoding screen capture to video file
pub struct Pipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    output_path: PathBuf,
    /// Whether this pipeline is configured for DMA-buf input
    dmabuf_mode: bool,
    dmabuf_allocator: Option<gst_allocators::DmaBufAllocator>,
}

impl Pipeline {
    /// Create a new encoding pipeline
    ///
    /// # Arguments
    /// * `encoder` - Encoder to use
    /// * `container` - Container format
    /// * `output_path` - Output file path
    /// * `capture_width` - Width of input frames
    /// * `capture_height` - Height of input frames
    /// * `crop` - Optional region to crop from input
    /// * `framerate` - Frames per second
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

        // Add parser element for certain codecs (required for proper muxing)
        let parser_elem = match encoder.codec {
            super::encoder::Codec::H264 => Some(
                gst::ElementFactory::make("h264parse")
                    .build()
                    .context("Failed to create h264parse element")?,
            ),
            super::encoder::Codec::H265 => Some(
                gst::ElementFactory::make("h265parse")
                    .build()
                    .context("Failed to create h265parse element")?,
            ),
            _ => None,
        };

        let muxer = gst::ElementFactory::make(container.muxer_element())
            .build()
            .with_context(|| format!("Failed to create muxer: {}", container.muxer_element()))?;

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
                "Adding crop: left={}, top={}, right={}, bottom={} (output: {}x{})",
                clamped_left,
                clamped_top,
                right,
                bottom,
                clamped_width,
                clamped_height
            );

            // Create capsfilter to enforce exact output dimensions
            // This ensures the encoder receives the exact even-aligned dimensions
            let scale_caps = gst::Caps::builder("video/x-raw")
                .field("width", clamped_width as i32)
                .field("height", clamped_height as i32)
                .build();
            let capsfilter = gst::ElementFactory::make("capsfilter")
                .property("caps", &scale_caps)
                .build()
                .context("Failed to create capsfilter element")?;

            // Add elements to pipeline
            pipeline.add_many([
                appsrc.upcast_ref(),
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
            // Add elements to pipeline without crop
            pipeline.add_many([
                appsrc.upcast_ref(),
                &videoconvert,
                &videoscale,
                &encoder_elem,
            ])?;
            if let Some(ref parser) = parser_elem {
                pipeline.add(parser)?;
            }
            pipeline.add_many([&muxer, &filesink])?;

            // Link elements
            gst::Element::link_many([
                appsrc.upcast_ref(),
                &videoconvert,
                &videoscale,
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
        })
    }

    /// Create a new encoding pipeline configured for DMA-buf input
    ///
    /// This pipeline can receive frames directly from DMA-buf file descriptors
    /// without CPU copies, enabling zero-copy capture from the compositor.
    ///
    /// # Arguments
    /// * `encoder` - Encoder to use
    /// * `container` - Container format
    /// * `output_path` - Output file path
    /// * `capture_width` - Width of input frames
    /// * `capture_height` - Height of input frames
    /// * `crop` - Optional region to crop from input
    /// * `framerate` - Frames per second
    /// * `drm_format` - DRM fourcc format of input buffers
    pub fn new_dmabuf(
        encoder: &EncoderInfo,
        container: Container,
        output_path: &Path,
        capture_width: u32,
        capture_height: u32,
        crop: Option<CropRegion>,
        framerate: u32,
        drm_format: DrmFourcc,
    ) -> Result<Self> {
        gst::init().context("Failed to initialize GStreamer")?;

        if !encoder.supports_dmabuf_zero_copy {
            anyhow::bail!(
                "Encoder {} is not wired for the real DMA-BUF zero-copy path",
                encoder.gst_element
            );
        }

        let gst_format = drm_format_to_gst_format(drm_format).ok_or_else(|| {
            anyhow::anyhow!("Unsupported DRM format for GStreamer: {:?}", drm_format)
        })?;

        let output_size = pipeline_output_size(crop, capture_width, capture_height);
        log::info!(
            "Creating DMA-buf pipeline: capture {}x{}, output {}x{} @ {} fps, format={:?} ({})",
            capture_width,
            capture_height,
            output_size.0,
            output_size.1,
            framerate,
            drm_format,
            gst_format
        );

        let pipeline = gst::Pipeline::new();
        let dmabuf_allocator = gst_allocators::DmaBufAllocator::new();

        // Create elements
        let appsrc = gst_app::AppSrc::builder()
            .name("screen-source")
            .is_live(true)
            .format(gst::Format::Time)
            .build();

        let vaapipostproc = gst::ElementFactory::make("vaapipostproc")
            .build()
            .context("Failed to create vaapipostproc element for zero-copy path")?;

        let encoder_elem = gst::ElementFactory::make(&encoder.gst_element)
            .build()
            .with_context(|| format!("Failed to create encoder: {}", encoder.gst_element))?;

        // Add parser element for certain codecs (required for proper muxing)
        let parser_elem = match encoder.codec {
            super::encoder::Codec::H264 => Some(
                gst::ElementFactory::make("h264parse")
                    .build()
                    .context("Failed to create h264parse element")?,
            ),
            super::encoder::Codec::H265 => Some(
                gst::ElementFactory::make("h265parse")
                    .build()
                    .context("Failed to create h265parse element")?,
            ),
            _ => None,
        };

        let muxer = gst::ElementFactory::make(container.muxer_element())
            .build()
            .with_context(|| format!("Failed to create muxer: {}", container.muxer_element()))?;

        let filesink = gst::ElementFactory::make("filesink")
            .property("location", output_path.to_str().unwrap())
            .build()
            .context("Failed to create filesink element")?;

        let (clamped_left, clamped_top, clamped_width, clamped_height, right, bottom) = crop
            .as_ref()
            .map_or((0, 0, capture_width, capture_height, 0, 0), |region| {
                calculate_aligned_crop(region, capture_width, capture_height)
            });

        if crop.is_some() {
            log::info!(
                "Zero-copy crop: left={}, top={}, right={}, bottom={} (output {}x{})",
                clamped_left,
                clamped_top,
                right,
                bottom,
                clamped_width,
                clamped_height
            );
            vaapipostproc.set_property("crop-left", clamped_left);
            vaapipostproc.set_property("crop-top", clamped_top);
            vaapipostproc.set_property("crop-right", right);
            vaapipostproc.set_property("crop-bottom", bottom);
        }

        let input_structure = gst::Structure::builder("video/x-raw")
            .field("format", gst_format)
            .field("width", capture_width as i32)
            .field("height", capture_height as i32)
            .field("framerate", gst::Fraction::new(framerate as i32, 1))
            .build();
        let input_caps: gst::Caps = [(
            input_structure,
            gst_allocators::CAPS_FEATURES_MEMORY_DMABUF.clone(),
        )]
        .into();
        appsrc.set_caps(Some(&input_caps));

        let output_structure = gst::Structure::builder("video/x-raw")
            .field("format", "NV12")
            .field("width", clamped_width as i32)
            .field("height", clamped_height as i32)
            .field("framerate", gst::Fraction::new(framerate as i32, 1))
            .build();
        let output_caps: gst::Caps = [(
            output_structure,
            gst::CapsFeatures::new(["memory:VASurface"]),
        )]
        .into();
        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property("caps", &output_caps)
            .build()
            .context("Failed to create zero-copy capsfilter element")?;

        log::info!("Zero-copy input caps: {}", input_caps);
        log::info!("Zero-copy output caps (postproc->encoder): {}", output_caps);

        pipeline.add_many([
            appsrc.upcast_ref(),
            &vaapipostproc,
            &capsfilter,
            &encoder_elem,
        ])?;
        if let Some(ref parser) = parser_elem {
            pipeline.add(parser)?;
        }
        pipeline.add_many([&muxer, &filesink])?;

        gst::Element::link_many([
            appsrc.upcast_ref(),
            &vaapipostproc,
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
        })
    }

    /// Check if this pipeline is configured for DMA-buf input
    pub fn is_dmabuf_mode(&self) -> bool {
        self.dmabuf_mode
    }

    /// Start the pipeline
    pub fn start(&self) -> Result<()> {
        self.pipeline
            .set_state(gst::State::Playing)
            .context("Failed to start pipeline")?;
        Ok(())
    }

    /// Push a video frame to the pipeline
    ///
    /// # Arguments
    /// * `data` - Raw RGBA frame data
    /// * `timestamp` - Frame timestamp in nanoseconds
    pub fn push_frame(&self, data: &[u8], timestamp: u64) -> Result<()> {
        let mut buffer =
            gst::Buffer::with_size(data.len()).context("Failed to allocate GStreamer buffer")?;

        {
            let buffer_mut = buffer.get_mut().unwrap();
            buffer_mut.set_pts(gst::ClockTime::from_nseconds(timestamp));
            let mut map = buffer_mut
                .map_writable()
                .context("Failed to map buffer for writing")?;
            map.copy_from_slice(data);
        }

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
        let video_format = drm_format_to_gst_video_format(dmabuf.format).ok_or_else(|| {
            anyhow::anyhow!(
                "Unsupported DMA-BUF format for zero-copy metadata: {:?}",
                dmabuf.format
            )
        })?;

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
                video_format,
                dmabuf.width,
                dmabuf.height,
                &[0],
                &[dmabuf.stride as i32],
            )
            .context("Failed to attach video metadata to DMA-BUF buffer")?;
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
                MessageView::StateChanged(state_change) => {
                    if state_change
                        .src()
                        .map(|s| s.name().as_str() == "pipeline0")
                        .unwrap_or(false)
                    {
                        log::debug!(
                            "Pipeline state changed: {:?} -> {:?}",
                            state_change.old(),
                            state_change.current()
                        );
                    }
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
