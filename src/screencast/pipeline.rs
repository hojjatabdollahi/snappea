//! GStreamer pipeline construction and management

use anyhow::{Context, Result};
use drm_fourcc::DrmFourcc;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

use super::dmabuf::drm_format_to_gst_format;
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

/// Calculate clamped and aligned crop parameters for video encoding
/// Returns (left, top, width, height, right, bottom) where right/bottom are the amounts to crop from those edges
fn calculate_aligned_crop(
    region: &CropRegion,
    capture_width: u32,
    capture_height: u32,
) -> (u32, u32, u32, u32, u32, u32) {
    // Clamp region to capture bounds to prevent overflow
    let clamped_left = region.left.min(capture_width.saturating_sub(1));
    let clamped_top = region.top.min(capture_height.saturating_sub(1));
    let max_width = capture_width.saturating_sub(clamped_left);
    let max_height = capture_height.saturating_sub(clamped_top);
    let clamped_width = region.width.min(max_width).max(1);
    let clamped_height = region.height.min(max_height).max(1);

    // Round dimensions down to even numbers (required by most video encoders)
    // This prevents green lines on the right/bottom edge caused by YUV padding
    // We crop 1 extra pixel from the right/bottom if dimensions are odd
    let aligned_width = (clamped_width & !1).max(2);
    let aligned_height = (clamped_height & !1).max(2);

    // Calculate right/bottom crop amounts
    // right = total_width - left - desired_output_width
    let right = capture_width
        .saturating_sub(clamped_left)
        .saturating_sub(aligned_width);
    let bottom = capture_height
        .saturating_sub(clamped_top)
        .saturating_sub(aligned_height);

    log::debug!(
        "Crop alignment: input {}x{}, region ({},{} {}x{}) -> aligned {}x{}, crop l={} t={} r={} b={}",
        capture_width,
        capture_height,
        clamped_left,
        clamped_top,
        clamped_width,
        clamped_height,
        aligned_width,
        aligned_height,
        clamped_left,
        clamped_top,
        right,
        bottom
    );

    (
        clamped_left,
        clamped_top,
        aligned_width,
        aligned_height,
        right,
        bottom,
    )
}

/// GStreamer pipeline for encoding screen capture to video file
pub struct Pipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    output_path: PathBuf,
    /// Whether this pipeline is configured for DMA-buf input
    dmabuf_mode: bool,
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

        let gst_format = drm_format_to_gst_format(drm_format).ok_or_else(|| {
            anyhow::anyhow!("Unsupported DRM format for GStreamer: {:?}", drm_format)
        })?;

        let output_size = crop
            .map(|c| (c.width, c.height))
            .unwrap_or((capture_width, capture_height));
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

        // Configure appsrc caps for DMA-buf input at capture size
        // Note: We don't use memory:DMABuf feature because videoconvert doesn't
        // support it directly. GStreamer will auto-map the DMA-buf when needed.
        // This still saves the userspace ABGR→RGBA conversion compared to SHM path.
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", gst_format)
            .field("width", capture_width as i32)
            .field("height", capture_height as i32)
            .field("framerate", gst::Fraction::new(framerate as i32, 1))
            .build();
        appsrc.set_caps(Some(&caps));

        Ok(Self {
            pipeline,
            appsrc,
            output_path: output_path.to_path_buf(),
            dmabuf_mode: true,
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

    /// Push a DMA-buf frame to the pipeline
    ///
    /// This method reads data from a DMA-buf and creates a GStreamer buffer.
    /// While not true zero-copy, this approach:
    /// 1. Avoids userspace ABGR→RGBA conversion (compositor outputs correct format)
    /// 2. Works reliably with all GStreamer elements
    ///
    /// # Arguments
    /// * `fd` - Raw file descriptor of the DMA-buf
    /// * `size` - Size of the buffer in bytes
    /// * `timestamp` - Frame timestamp in nanoseconds
    pub fn push_dmabuf_frame(&self, fd: RawFd, size: usize, timestamp: u64) -> Result<()> {
        // mmap the DMA-buf to read its contents
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(anyhow::anyhow!("Failed to mmap DMA-buf"));
        }

        // Create GStreamer buffer with the data
        let mut buffer =
            gst::Buffer::with_size(size).context("Failed to allocate GStreamer buffer")?;

        {
            let buffer_mut = buffer.get_mut().unwrap();
            buffer_mut.set_pts(gst::ClockTime::from_nseconds(timestamp));
            let mut map = buffer_mut
                .map_writable()
                .context("Failed to map buffer for writing")?;

            // Copy from DMA-buf to GStreamer buffer
            unsafe {
                std::ptr::copy_nonoverlapping(ptr as *const u8, map.as_mut_ptr(), size);
            }
        }

        // Unmap the DMA-buf
        unsafe {
            libc::munmap(ptr, size);
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
