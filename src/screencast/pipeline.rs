//! GStreamer pipeline construction and management

use anyhow::{Context, Result};
use drm_fourcc::DrmFourcc;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};

use crate::config::Container;
use super::encoder::EncoderInfo;
use super::dmabuf::drm_format_to_gst_format;

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
    /// * `width` - Video width
    /// * `height` - Video height
    /// * `framerate` - Frames per second
    pub fn new(
        encoder: &EncoderInfo,
        container: Container,
        output_path: &Path,
        width: u32,
        height: u32,
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

        let encoder_elem = gst::ElementFactory::make(&encoder.gst_element)
            .build()
            .with_context(|| format!("Failed to create encoder: {}", encoder.gst_element))?;

        let muxer = gst::ElementFactory::make(container.muxer_element())
            .build()
            .with_context(|| format!("Failed to create muxer: {}", container.muxer_element()))?;

        let filesink = gst::ElementFactory::make("filesink")
            .property("location", output_path.to_str().unwrap())
            .build()
            .context("Failed to create filesink element")?;

        // Add elements to pipeline
        pipeline.add_many([
            appsrc.upcast_ref(),
            &videoconvert,
            &encoder_elem,
            &muxer,
            &filesink,
        ])?;

        // Link elements
        gst::Element::link_many([
            appsrc.upcast_ref(),
            &videoconvert,
            &encoder_elem,
            &muxer,
            &filesink,
        ])?;

        // Configure appsrc caps (raw RGBA video)
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "RGBA")
            .field("width", width as i32)
            .field("height", height as i32)
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
    /// * `width` - Video width
    /// * `height` - Video height
    /// * `framerate` - Frames per second
    /// * `drm_format` - DRM fourcc format of input buffers
    pub fn new_dmabuf(
        encoder: &EncoderInfo,
        container: Container,
        output_path: &Path,
        width: u32,
        height: u32,
        framerate: u32,
        drm_format: DrmFourcc,
    ) -> Result<Self> {
        gst::init().context("Failed to initialize GStreamer")?;

        let gst_format = drm_format_to_gst_format(drm_format)
            .ok_or_else(|| anyhow::anyhow!("Unsupported DRM format for GStreamer: {:?}", drm_format))?;

        log::info!(
            "Creating DMA-buf pipeline: {}x{} @ {} fps, format={:?} ({})",
            width,
            height,
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

        let encoder_elem = gst::ElementFactory::make(&encoder.gst_element)
            .build()
            .with_context(|| format!("Failed to create encoder: {}", encoder.gst_element))?;

        let muxer = gst::ElementFactory::make(container.muxer_element())
            .build()
            .with_context(|| format!("Failed to create muxer: {}", container.muxer_element()))?;

        let filesink = gst::ElementFactory::make("filesink")
            .property("location", output_path.to_str().unwrap())
            .build()
            .context("Failed to create filesink element")?;

        // Add elements to pipeline
        pipeline.add_many([
            appsrc.upcast_ref(),
            &videoconvert,
            &encoder_elem,
            &muxer,
            &filesink,
        ])?;

        // Link elements
        gst::Element::link_many([
            appsrc.upcast_ref(),
            &videoconvert,
            &encoder_elem,
            &muxer,
            &filesink,
        ])?;

        // Configure appsrc caps for DMA-buf input
        // Note: We don't use memory:DMABuf feature because videoconvert doesn't
        // support it directly. GStreamer will auto-map the DMA-buf when needed.
        // This still saves the userspace ABGR→RGBA conversion compared to SHM path.
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", gst_format)
            .field("width", width as i32)
            .field("height", height as i32)
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
        let mut buffer = gst::Buffer::with_size(data.len())
            .context("Failed to allocate GStreamer buffer")?;

        {
            let buffer_mut = buffer.get_mut().unwrap();
            buffer_mut.set_pts(gst::ClockTime::from_nseconds(timestamp));
            let mut map = buffer_mut.map_writable()
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
        let mut buffer = gst::Buffer::with_size(size)
            .context("Failed to allocate GStreamer buffer")?;

        {
            let buffer_mut = buffer.get_mut().unwrap();
            buffer_mut.set_pts(gst::ClockTime::from_nseconds(timestamp));
            let mut map = buffer_mut.map_writable()
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
        self.appsrc.end_of_stream()
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
                    if state_change.src().map(|s| s.name().as_str() == "pipeline0").unwrap_or(false) {
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

        self.pipeline.set_state(gst::State::Null)
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

        let metadata = std::fs::metadata(&self.output_path)
            .with_context(|| format!("Failed to read output file metadata: {}", self.output_path.display()))?;

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
