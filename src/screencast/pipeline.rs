//! GStreamer pipeline construction and management

use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::path::Path;

use crate::config::Container;
use super::encoder::EncoderInfo;

/// GStreamer pipeline for encoding screen capture to video file
pub struct Pipeline {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
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

        Ok(Self { pipeline, appsrc })
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

    /// Signal end of stream and finalize the video file
    pub fn finish(&self) -> Result<()> {
        self.appsrc.end_of_stream()
            .map_err(|_| anyhow::anyhow!("Failed to send EOS"))?;

        // Wait for EOS to propagate through pipeline
        let bus = self.pipeline.bus().unwrap();
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(5)) {
            use gst::MessageView;
            match msg.view() {
                MessageView::Eos(..) => break,
                MessageView::Error(err) => {
                    return Err(anyhow::anyhow!(
                        "Pipeline error: {} ({})",
                        err.error(),
                        err.debug().unwrap_or_default()
                    ));
                }
                _ => {}
            }
        }

        self.pipeline.set_state(gst::State::Null)
            .context("Failed to stop pipeline")?;

        Ok(())
    }
}
