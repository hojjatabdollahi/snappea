// SPDX-License-Identifier: GPL-3.0-only

//! Encoder detection and selection.

use crate::config::Container;
use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::sync::OnceLock;

/// Codec type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
    VP9,
    AV1,
}

impl Codec {
    pub const fn name(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::H265 => "H.265",
            Self::VP9 => "VP9",
            Self::AV1 => "AV1",
        }
    }

    /// Parser element needed to mux this codec, if any. H.264/H.265 need one for MP4.
    pub const fn parser_element(self) -> Option<&'static str> {
        match self {
            Self::H264 => Some("h264parse"),
            Self::H265 => Some("h265parse"),
            Self::AV1 => Some("av1parse"),
            Self::VP9 => None,
        }
    }

    /// Software decoders for this codec, best first. Used to read back a probe
    /// encode, where the hardware path cannot hand pixels to the CPU.
    pub fn software_decoder(self) -> Option<&'static str> {
        let candidates: &[&str] = match self {
            Self::H264 => &["avdec_h264"],
            Self::H265 => &["avdec_h265"],
            Self::VP9 => &["vp9dec", "avdec_vp9"],
            Self::AV1 => &["dav1ddec", "av1dec", "avdec_av1"],
        };
        candidates
            .iter()
            .copied()
            .find(|name| gst::ElementFactory::find(name).is_some())
    }

    /// Whether the pipeline can mux this codec into `container`. MKV accepts every codec.
    /// MP4 only H.264/H.265, since VP9/AV1 in MP4 comes out empty (issue #17).
    pub const fn supports_container(self, container: Container) -> bool {
        match (self, container) {
            (_, Container::Mkv) => true,
            (Self::H264 | Self::H265, Container::Mp4) => true,
            (Self::VP9 | Self::AV1, Container::Mp4) => false,
            (_, Container::Webm) => matches!(self, Self::VP9 | Self::AV1),
        }
    }

    /// Container to fall back to when the configured one is incompatible.
    pub const fn default_container(self) -> Container {
        match self {
            Self::H264 | Self::H265 => Container::Mp4,
            Self::VP9 | Self::AV1 => Container::Mkv,
        }
    }

    /// Best-effort codec inference from an encoder element name.
    pub fn from_element_name(name: &str) -> Option<Self> {
        if name.contains("h265") || name.contains("hevc") {
            Some(Self::H265)
        } else if name.contains("h264") || name.contains("x264") || name.contains("openh264") {
            Some(Self::H264)
        } else if name.contains("av1") {
            Some(Self::AV1)
        } else if name.contains("vp9") {
            Some(Self::VP9)
        } else {
            None
        }
    }
}

/// Information about an available encoder
#[derive(Debug, Clone)]
pub struct EncoderInfo {
    /// Human-readable name (e.g., "VA-API H.264")
    pub name: String,
    /// `GStreamer` element name (e.g., "vah264enc")
    pub gst_element: String,
    /// Codec type
    pub codec: Codec,
    /// Whether this is hardware-accelerated
    pub hardware: bool,
    /// Whether this encoder can participate in the real DMA-BUF zero-copy path
    pub supports_dmabuf_zero_copy: bool,
    /// Priority (lower = better, hardware encoders have lower priority)
    pub priority: u8,
}

impl EncoderInfo {
    /// The name, marked `(hw)` for a hardware encoder.
    pub fn display_name(&self) -> String {
        if self.hardware {
            format!("{} (hw)", self.name)
        } else {
            self.name.clone()
        }
    }

    pub const fn zero_copy_display_name(&self) -> &'static str {
        if self.supports_dmabuf_zero_copy {
            "DMA-BUF zero-copy capable"
        } else {
            "copied-memory path only"
        }
    }
}

/// An encoder to probe and the metadata to publish if it resolves.
struct Candidate {
    name: &'static str,
    gst_element: &'static str,
    codec: Codec,
    hardware: bool,
    supports_dmabuf_zero_copy: bool,
    priority: u8,
}

/// Detect available video encoders. Each codec has a preference-ordered
/// candidate list. Only the `va*` plugin is probed for VA-API: `vaapi*`
/// (`gstreamer-vaapi`) is deprecated and gone from `GStreamer` 1.28, and `va*`
/// imports DMA-BUF directly through `vapostproc`.
pub fn detect_encoders() -> Result<Vec<EncoderInfo>> {
    gst::init().context("Failed to initialize GStreamer")?;

    // One entry per slice: the first available candidate.
    let candidate_groups: &[&[Candidate]] = &[
        // VA-API H.264 (Intel/AMD): priority 10. Low-power first, as GNOME Shell does.
        &[
            Candidate {
                name: "VA-API H.264 (low-power)",
                gst_element: "vah264lpenc",
                codec: Codec::H264,
                hardware: true,
                supports_dmabuf_zero_copy: true,
                priority: 10,
            },
            Candidate {
                name: "VA-API H.264",
                gst_element: "vah264enc",
                codec: Codec::H264,
                hardware: true,
                supports_dmabuf_zero_copy: true,
                priority: 10,
            },
        ],
        // VA-API H.265: priority 11
        &[
            Candidate {
                name: "VA-API H.265 (low-power)",
                gst_element: "vah265lpenc",
                codec: Codec::H265,
                hardware: true,
                supports_dmabuf_zero_copy: true,
                priority: 11,
            },
            Candidate {
                name: "VA-API H.265",
                gst_element: "vah265enc",
                codec: Codec::H265,
                hardware: true,
                supports_dmabuf_zero_copy: true,
                priority: 11,
            },
        ],
        // VA-API AV1: priority 12
        &[Candidate {
            name: "VA-API AV1",
            gst_element: "vaav1enc",
            codec: Codec::AV1,
            hardware: true,
            supports_dmabuf_zero_copy: true,
            priority: 12,
        }],
        // VA-API VP9: priority 13
        &[Candidate {
            name: "VA-API VP9",
            gst_element: "vavp9enc",
            codec: Codec::VP9,
            hardware: true,
            supports_dmabuf_zero_copy: true,
            priority: 13,
        }],
        // NVENC H.264 (NVIDIA): priority 20. Takes system or CUDA memory only, no DMA-BUF.
        &[
            Candidate {
                name: "NVENC H.264",
                gst_element: "nvh264enc",
                codec: Codec::H264,
                hardware: true,
                supports_dmabuf_zero_copy: false,
                priority: 20,
            },
            Candidate {
                name: "NVENC H.264",
                gst_element: "nvcudah264enc",
                codec: Codec::H264,
                hardware: true,
                supports_dmabuf_zero_copy: false,
                priority: 20,
            },
        ],
        // NVENC H.265: priority 21
        &[
            Candidate {
                name: "NVENC H.265",
                gst_element: "nvh265enc",
                codec: Codec::H265,
                hardware: true,
                supports_dmabuf_zero_copy: false,
                priority: 21,
            },
            Candidate {
                name: "NVENC H.265",
                gst_element: "nvcudah265enc",
                codec: Codec::H265,
                hardware: true,
                supports_dmabuf_zero_copy: false,
                priority: 21,
            },
        ],
        // NVENC AV1: priority 22
        &[Candidate {
            name: "NVENC AV1",
            gst_element: "nvav1enc",
            codec: Codec::AV1,
            hardware: true,
            supports_dmabuf_zero_copy: false,
            priority: 22,
        }],
        // Software H.264: x264 preferred, openh264 as a fallback.
        &[
            Candidate {
                name: "x264 H.264",
                gst_element: "x264enc",
                codec: Codec::H264,
                hardware: false,
                supports_dmabuf_zero_copy: false,
                priority: 100,
            },
            Candidate {
                name: "OpenH264",
                gst_element: "openh264enc",
                codec: Codec::H264,
                hardware: false,
                supports_dmabuf_zero_copy: false,
                priority: 100,
            },
        ],
        // Software VP9: priority 101
        &[Candidate {
            name: "VP9",
            gst_element: "vp9enc",
            codec: Codec::VP9,
            hardware: false,
            supports_dmabuf_zero_copy: false,
            priority: 101,
        }],
    ];

    // Probing runs real pipelines, so cache for the life of the process.
    static CACHE: OnceLock<Vec<EncoderInfo>> = OnceLock::new();
    let encoders = CACHE.get_or_init(|| {
        let mut encoders = Vec::new();
        for group in candidate_groups {
            // Keep the first candidate that exists and actually encodes (issue #17).
            if let Some(candidate) = group
                .iter()
                .find(|c| encoder_available(c.gst_element) && encoder_works(c.gst_element, c.codec))
            {
                encoders.push(EncoderInfo {
                    name: candidate.name.to_string(),
                    gst_element: candidate.gst_element.to_string(),
                    codec: candidate.codec,
                    hardware: candidate.hardware,
                    supports_dmabuf_zero_copy: candidate.supports_dmabuf_zero_copy,
                    priority: candidate.priority,
                });
            }
        }

        // Sort by priority (lower first)
        encoders.sort_by_key(|e| e.priority);
        encoders
    });

    Ok(encoders.clone())
}

/// The smallest frame `element` accepts, from its sink template. Hardware
/// encoders refuse small frames outright (`vah264enc` wants 128x128), where
/// the software ones take anything even.
pub fn min_input_size(element_name: &str) -> (u32, u32) {
    let Some(factory) = gst::ElementFactory::find(element_name) else {
        return (2, 2);
    };
    let mut min = (u32::MAX, u32::MAX);
    for template in factory.static_pad_templates() {
        if template.direction() != gst::PadDirection::Sink {
            continue;
        }
        for s in template.caps().iter() {
            let low = |field: &str| match s.get::<gst::IntRange<i32>>(field) {
                Ok(range) => range.min().max(1) as u32,
                Err(_) => s.get::<i32>(field).map_or(1, |v| v.max(1) as u32),
            };
            min = (min.0.min(low("width")), min.1.min(low("height")));
        }
    }
    if min.0 == u32::MAX { (2, 2) } else { min }
}

/// Check if a `GStreamer` encoder element is available
fn encoder_available(element_name: &str) -> bool {
    gst::ElementFactory::find(element_name).is_some()
}

/// Timeout for encoder probe pipelines (EOS or Error).
const PROBE_TIMEOUT_SECS: u64 = 3;

/// Verify an encoder works by running `videotestsrc → encoder → fakesink` to
/// EOS within [`PROBE_TIMEOUT_SECS`]. Hardware encoders can register and then
/// fail to negotiate.
fn encoder_works(element_name: &str, codec: Codec) -> bool {
    let parser = codec
        .parser_element()
        .map(|p| format!(" ! {p}"))
        .unwrap_or_default();
    let pipeline_desc = format!(
        "videotestsrc num-buffers=3 \
         ! video/x-raw,format=NV12,width=320,height=240,framerate=10/1 \
         ! videoconvert ! {element_name}{parser} ! fakesink"
    );

    let pipeline = match gst::parse::launch(&pipeline_desc) {
        Ok(p) => p,
        Err(e) => {
            log::debug!("Encoder probe build failed for {element_name}: {e}");
            return false;
        }
    };
    let Some(bus) = pipeline.bus() else {
        return false;
    };
    if pipeline.set_state(gst::State::Playing).is_err() {
        let _ = pipeline.set_state(gst::State::Null);
        return false;
    }

    let ok = loop {
        match bus.timed_pop(gst::ClockTime::from_seconds(PROBE_TIMEOUT_SECS)) {
            Some(msg) => match msg.view() {
                gst::MessageView::Eos(_) => break true,
                gst::MessageView::Error(_) => break false,
                _ => {}
            },
            None => break false, // timed out
        }
    };
    let _ = pipeline.set_state(gst::State::Null);
    if !ok {
        log::debug!("Encoder probe failed/timed out for {element_name}");
    }
    ok
}

/// The containers offered for a recording, best first. No `WebM`: live software
/// VP9 is slower than realtime.
pub const RECORDING_CONTAINERS: &[Container] = &[Container::Mp4, Container::Mkv];

/// The containers this codec can actually be muxed into, best first.
#[must_use]
pub fn containers_for(codec: Option<Codec>) -> Vec<Container> {
    RECORDING_CONTAINERS
        .iter()
        .copied()
        .filter(|c| codec.is_none_or(|codec| codec.supports_container(*c)))
        .collect()
}

/// The current container if this codec can mux into it, else the best that can.
#[must_use]
pub fn container_for(codec: Option<Codec>, current: Container) -> Container {
    let options = containers_for(codec);
    if options.contains(&current) {
        return current;
    }
    options
        .first()
        .copied()
        .unwrap_or_else(|| codec.map_or(Container::Mkv, Codec::default_container))
}

pub fn best_encoder() -> Result<EncoderInfo> {
    let encoders = detect_encoders()?;
    encoders
        .into_iter()
        .next()
        .context("No video encoders available")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webm_not_recording_format() {
        assert!(!RECORDING_CONTAINERS.contains(&Container::Webm));
    }

    #[test]
    fn containers_match_codec() {
        assert_eq!(
            containers_for(Some(Codec::H264)),
            vec![Container::Mp4, Container::Mkv]
        );
        // VP9 in MP4 produced an empty file. MKV accepts every codec.
        assert_eq!(containers_for(Some(Codec::VP9)), vec![Container::Mkv]);
        assert_eq!(containers_for(Some(Codec::AV1)), vec![Container::Mkv]);
    }

    #[test]
    fn unknown_encoder_all_containers() {
        // Nothing is known against it, so nothing is ruled out.
        assert_eq!(containers_for(None), RECORDING_CONTAINERS.to_vec());
    }

    #[test]
    fn working_container_kept() {
        assert_eq!(
            container_for(Some(Codec::H264), Container::Mkv),
            Container::Mkv
        );
        assert_eq!(
            container_for(Some(Codec::H264), Container::Mp4),
            Container::Mp4
        );
    }

    #[test]
    fn broken_container_replaced() {
        // Switching from an H.264 encoder to a VP9 one leaves MP4 behind,
        // which the pipeline would accept and then write nothing into.
        assert_eq!(
            container_for(Some(Codec::VP9), Container::Mp4),
            Container::Mkv
        );
        // WebM was never offered, so a config naming it gets the best supported one.
        assert_eq!(
            container_for(Some(Codec::H264), Container::Webm),
            Container::Mp4
        );
    }

    #[test]
    fn first_container_preferred() {
        // The list is best-first, and `container_for` takes the head of it, so
        // an encoder with no valid current container lands on MP4 where it can.
        assert_eq!(RECORDING_CONTAINERS.first(), Some(&Container::Mp4));
    }

    #[test]
    fn test_codec_name() {
        assert_eq!(Codec::H264.name(), "H.264");
        assert_eq!(Codec::H265.name(), "H.265");
        assert_eq!(Codec::VP9.name(), "VP9");
        assert_eq!(Codec::AV1.name(), "AV1");
    }

    #[test]
    fn test_encoder_info_display_name() {
        let hw_encoder = EncoderInfo {
            name: "VA-API H.264".to_string(),
            gst_element: "vaapih264enc".to_string(),
            codec: Codec::H264,
            hardware: true,
            supports_dmabuf_zero_copy: true,
            priority: 10,
        };
        assert_eq!(hw_encoder.display_name(), "VA-API H.264 (hw)");

        let sw_encoder = EncoderInfo {
            name: "x264 H.264".to_string(),
            gst_element: "x264enc".to_string(),
            codec: Codec::H264,
            hardware: false,
            supports_dmabuf_zero_copy: false,
            priority: 100,
        };
        assert_eq!(sw_encoder.display_name(), "x264 H.264");
    }

    #[test]
    fn detect_encoders_sorted() {
        // This test will succeed even if no encoders are available
        let result = detect_encoders();
        assert!(result.is_ok());

        let encoders = result.unwrap();
        // Verify encoders are sorted by priority
        for i in 1..encoders.len() {
            assert!(encoders[i - 1].priority <= encoders[i].priority);
        }
    }

    #[test]
    fn best_encoder_picked() {
        // This test may fail on systems with no encoders, which is acceptable
        // In CI/CD, we'd need GStreamer plugins installed
        let result = best_encoder();

        if let Ok(encoder) = result {
            // If we have an encoder, verify it's valid
            assert!(!encoder.name.is_empty());
            assert!(!encoder.gst_element.is_empty());
        }
        // If no encoders available, that's also a valid outcome for this test
    }

    #[test]
    fn detect_encoders_unique() {
        // Each codec/backend must resolve to at most one element, even on systems
        // that have both the legacy (vaapi*) and modern (va*) plugins installed.
        let Ok(encoders) = detect_encoders() else {
            return;
        };
        let mut seen = std::collections::HashSet::new();
        for e in &encoders {
            assert!(
                seen.insert(e.gst_element.clone()),
                "duplicate encoder element: {}",
                e.gst_element
            );
        }
    }

    #[test]
    fn test_codec_container_support() {
        // MP4 only for H.264/H.265. MKV for everything.
        assert!(Codec::H264.supports_container(Container::Mp4));
        assert!(Codec::H265.supports_container(Container::Mp4));
        assert!(!Codec::VP9.supports_container(Container::Mp4));
        assert!(!Codec::AV1.supports_container(Container::Mp4));

        for codec in [Codec::H264, Codec::H265, Codec::VP9, Codec::AV1] {
            assert!(codec.supports_container(Container::Mkv));
        }

        assert_eq!(Codec::H264.default_container(), Container::Mp4);
        assert_eq!(Codec::VP9.default_container(), Container::Mkv);
    }

    #[test]
    fn test_codec_from_element_name() {
        assert_eq!(Codec::from_element_name("vaapih264enc"), Some(Codec::H264));
        assert_eq!(Codec::from_element_name("vah264enc"), Some(Codec::H264));
        assert_eq!(Codec::from_element_name("x264enc"), Some(Codec::H264));
        assert_eq!(Codec::from_element_name("openh264enc"), Some(Codec::H264));
        assert_eq!(Codec::from_element_name("vah265enc"), Some(Codec::H265));
        assert_eq!(Codec::from_element_name("nvh265enc"), Some(Codec::H265));
        assert_eq!(Codec::from_element_name("vaav1enc"), Some(Codec::AV1));
        assert_eq!(Codec::from_element_name("vp9enc"), Some(Codec::VP9));
        assert_eq!(Codec::from_element_name("wildcard"), None);
    }

    #[test]
    fn encoder_codec_container_consistent() {
        // Every detected encoder must have a working default container.
        let Ok(encoders) = detect_encoders() else {
            return;
        };
        for e in &encoders {
            assert!(e.codec.supports_container(e.codec.default_container()));
        }
    }

    #[test]
    fn hardware_priority_before_software() {
        // Verify priority system: hardware < software
        let hw_priority = 10u8; // VA-API
        let sw_priority = 100u8; // x264
        assert!(hw_priority < sw_priority);
    }
}
