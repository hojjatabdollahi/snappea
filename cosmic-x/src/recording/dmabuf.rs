// SPDX-License-Identifier: GPL-3.0-only

//! GBM-backed DMA-buf allocation for zero-copy capture.

use anyhow::{Context, Result};
use drm_fourcc::{DrmFourcc, DrmModifier};
use gstreamer_video as gst_video;
use std::fs::{File, OpenOptions};
use std::os::fd::OwnedFd;
use std::path::PathBuf;

/// DMA-buf buffer information
pub struct DmabufBuffer {
    /// DMA-buf file descriptor
    pub fd: OwnedFd,
    /// Visible width in pixels: what the compositor renders into.
    pub width: u32,
    /// Width the tiler actually laid out, `stride / 4`. The VA driver imports a
    /// buffer only when described at this width, so this is what the encoder
    /// side sees, with the padding cropped off again.
    pub padded_width: u32,
    /// Buffer height in pixels
    pub height: u32,
    /// Buffer stride in bytes
    pub stride: u32,
    /// Offset of the single plane, in bytes
    pub offset: u32,
    /// DRM format fourcc
    pub format: DrmFourcc,
    /// DRM format modifier
    pub modifier: DrmModifier,
    /// Total buffer size in bytes
    pub size: usize,
}

/// Context for DMA-buf buffer management using GBM
pub struct DmabufContext {
    /// GBM device for buffer allocation
    gbm_device: gbm::Device<File>,
}

impl DmabufContext {
    /// Create a new DMA-buf context by opening a GPU render node
    pub fn new() -> Result<Self> {
        let render_node = find_render_node().context("Failed to find GPU render node")?;

        log::info!("Using GPU render node: {}", render_node.display());

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&render_node)
            .with_context(|| format!("Failed to open render node: {}", render_node.display()))?;

        let gbm_device = gbm::Device::new(file)
            .map_err(|e| anyhow::anyhow!("Failed to create GBM device: {e:?}"))?;

        Ok(Self { gbm_device })
    }

    /// Allocate a DMA-buf buffer with the specified dimensions and format
    pub fn allocate_buffer(
        &self,
        width: u32,
        height: u32,
        format: DrmFourcc,
        modifier: DrmModifier,
    ) -> Result<DmabufBuffer> {
        self.allocate(width, height, format, modifier, None)
    }

    /// Allocate a buffer with every pixel set to `pixel`, written through GBM's
    /// tiling-aware map so the layout is right whatever the modifier.
    pub fn allocate_filled(
        &self,
        width: u32,
        height: u32,
        format: DrmFourcc,
        modifier: DrmModifier,
        pixel: [u8; 4],
    ) -> Result<DmabufBuffer> {
        self.allocate(width, height, format, modifier, Some(&|_, _| pixel))
    }

    fn allocate(
        &self,
        width: u32,
        height: u32,
        format: DrmFourcc,
        modifier: DrmModifier,
        fill: Option<&dyn Fn(u32, u32) -> [u8; 4]>,
    ) -> Result<DmabufBuffer> {
        let mut bo = self.create_bo(width, height, format, modifier)?;
        // Tiled layouts pad the pitch past the width. Allocate at the padded width
        // instead, so the pitch is exactly `padded_width * 4` and imports cleanly.
        let padded_width = bo.stride() / 4;
        if padded_width != width {
            bo = self.create_bo(padded_width, height, format, modifier)?;
            if bo.stride() != padded_width * 4 {
                anyhow::bail!(
                    "GBM pitch {} for width {padded_width} is not the width itself",
                    bo.stride()
                );
            }
        }
        if let Some(pixel) = fill {
            bo.map_mut(0, 0, padded_width, height, |m| {
                let stride = m.stride() as usize;
                for y in 0..height as usize {
                    let row =
                        &mut m.buffer_mut()[y * stride..y * stride + padded_width as usize * 4];
                    for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        *px = pixel(x as u32, y as u32);
                    }
                }
            })
            .context("Failed to map GBM buffer for filling")?;
        }

        // Get the actual modifier used by GBM (may differ from requested)
        let actual_modifier = bo.modifier();
        if bo.plane_count() != 1 {
            anyhow::bail!(
                "Modifier {actual_modifier:?} lays {format:?} out in {} planes; only single-plane buffers are handled",
                bo.plane_count()
            );
        }
        let stride = bo.stride();
        let offset = bo.offset(0);
        let fd = bo
            .fd()
            .map_err(|e| anyhow::anyhow!("Failed to export DMA-buf fd: {e:?}"))?;
        // Tiled layouts pad beyond stride * height. Query the fd for the real size.
        let size = rustix::fs::seek(&fd, rustix::fs::SeekFrom::End(0))
            .context("Failed to query DMA-buf size")? as usize;

        log::debug!(
            "Allocated DMA-buf: {width}x{height} (padded to {padded_width}), format={format:?}, modifier={actual_modifier:?}, stride={stride}, size={size}"
        );

        Ok(DmabufBuffer {
            fd,
            width,
            padded_width,
            height,
            stride,
            offset,
            format,
            modifier: actual_modifier,
            size,
        })
    }

    fn create_bo(
        &self,
        width: u32,
        height: u32,
        format: DrmFourcc,
        modifier: DrmModifier,
    ) -> Result<gbm::BufferObject<()>> {
        let usage = gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR;

        let bo = if modifier == DrmModifier::Invalid {
            // Implicit modifier: plain allocation, layout is the driver's business
            self.gbm_device
                .create_buffer_object::<()>(width, height, format, usage)
                .map_err(|e| anyhow::anyhow!("Failed to create GBM buffer object: {e:?}"))?
        } else {
            // An explicit modifier must go through the modifier-aware API so the exported
            // dmabuf carries it (Vulkan compositors reject implicit ones), and must not be
            // combined with the LINEAR usage flag (Mesa rejects that).
            self.gbm_device
                .create_buffer_object_with_modifiers2::<()>(
                    width,
                    height,
                    format,
                    std::iter::once(modifier),
                    usage - gbm::BufferObjectFlags::LINEAR,
                )
                .map_err(|e| {
                    anyhow::anyhow!("Failed to create GBM buffer object with modifier: {e:?}")
                })?
        };
        Ok(bo)
    }

    /// Check if a specific format/modifier combination is supported
    pub fn is_format_supported(&self, format: DrmFourcc, modifier: DrmModifier) -> bool {
        // Try to create a small test buffer
        let usage = gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR;

        if modifier == DrmModifier::Invalid {
            self.gbm_device
                .create_buffer_object::<()>(64, 64, format, usage)
                .is_ok()
        } else {
            self.gbm_device
                .create_buffer_object_with_modifiers2::<()>(
                    64,
                    64,
                    format,
                    std::iter::once(modifier),
                    usage - gbm::BufferObjectFlags::LINEAR,
                )
                .is_ok()
        }
    }
}

/// Find a usable GPU render node (typically /dev/dri/renderD128)
fn find_render_node() -> Result<PathBuf> {
    let dri_path = PathBuf::from("/dev/dri");

    if !dri_path.exists() {
        return Err(anyhow::anyhow!(
            "/dev/dri does not exist - no GPU available?"
        ));
    }

    // Look for render nodes (renderD*)
    for entry in std::fs::read_dir(&dri_path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with("renderD") {
            let path = entry.path();
            // Check if we can open it
            if OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .is_ok()
            {
                return Ok(path);
            }
        }
    }

    // Fall back to card nodes if no render node available
    for entry in std::fs::read_dir(&dri_path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with("card") {
            let path = entry.path();
            if OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .is_ok()
            {
                log::warn!("Using card node {} instead of render node", path.display());
                return Ok(path);
            }
        }
    }

    Err(anyhow::anyhow!("No usable GPU device found in /dev/dri"))
}

/// `GStreamer` `VideoFormat` with the byte layout of a single-plane DRM fourcc.
pub fn drm_format_to_gst_video_format(format: DrmFourcc) -> Option<gst_video::VideoFormat> {
    gst_video::dma_drm_fourcc_to_format(format as u32).ok()
}

/// The fourcc with the same channels in the opposite byte order.
pub const fn byte_reversed(format: DrmFourcc) -> Option<DrmFourcc> {
    Some(match format {
        DrmFourcc::Xrgb8888 => DrmFourcc::Bgrx8888,
        DrmFourcc::Bgrx8888 => DrmFourcc::Xrgb8888,
        DrmFourcc::Argb8888 => DrmFourcc::Bgra8888,
        DrmFourcc::Bgra8888 => DrmFourcc::Argb8888,
        DrmFourcc::Xbgr8888 => DrmFourcc::Rgbx8888,
        DrmFourcc::Rgbx8888 => DrmFourcc::Xbgr8888,
        DrmFourcc::Abgr8888 => DrmFourcc::Rgba8888,
        DrmFourcc::Rgba8888 => DrmFourcc::Abgr8888,
        _ => return None,
    })
}

/// Pure opaque red as four bytes in `format`'s memory layout.
pub const fn red_pixel(format: DrmFourcc) -> Option<[u8; 4]> {
    Some(match format {
        // B, G, R, X
        DrmFourcc::Xrgb8888 | DrmFourcc::Argb8888 => [0, 0, 255, 255],
        // R, G, B, X
        DrmFourcc::Xbgr8888 | DrmFourcc::Abgr8888 => [255, 0, 0, 255],
        // X, B, G, R
        DrmFourcc::Rgbx8888 | DrmFourcc::Rgba8888 => [255, 0, 0, 255],
        // X, R, G, B
        DrmFourcc::Bgrx8888 | DrmFourcc::Bgra8888 => [255, 255, 0, 0],
        _ => return None,
    })
}

/// The DMA-BUF format both sides agree on: one the compositor can render into
/// and `importer` (the VA post-processor) can import. Opaque RGB first, since
/// alpha can send drivers down blending paths. The linear modifier when both
/// offer it, else the importer's first choice.
pub fn select_zero_copy_source_format(
    compositor: &[(DrmFourcc, Vec<DrmModifier>)],
    importer: &[(DrmFourcc, Vec<DrmModifier>)],
) -> Option<(DrmFourcc, DrmModifier)> {
    let preferred = [
        DrmFourcc::Xrgb8888,
        DrmFourcc::Xbgr8888,
        DrmFourcc::Argb8888,
        DrmFourcc::Abgr8888,
    ];
    preferred.iter().find_map(|fourcc| {
        let ours = &compositor.iter().find(|(f, _)| f == fourcc)?.1;
        let theirs = &importer.iter().find(|(f, _)| f == fourcc)?.1;
        let common: Vec<DrmModifier> = theirs
            .iter()
            .copied()
            .filter(|m| ours.contains(m))
            .collect();
        let modifier = common
            .iter()
            .copied()
            .find(|m| *m == DrmModifier::Linear)
            .or_else(|| common.first().copied())?;
        Some((*fourcc, modifier))
    })
}

/// Pool of three buffers: one being captured, one being encoded, one free.
pub struct TripleBufferPool {
    buffers: [DmabufBuffer; 3],
    current_index: usize,
}

impl TripleBufferPool {
    /// Create a new triple buffer pool
    pub fn new(
        ctx: &DmabufContext,
        width: u32,
        height: u32,
        format: DrmFourcc,
        modifier: DrmModifier,
    ) -> Result<Self> {
        log::info!("Allocating triple buffer pool: {width}x{height}, format={format:?}");

        let buffer0 = ctx
            .allocate_buffer(width, height, format, modifier)
            .context("Failed to allocate buffer 0")?;
        let buffer1 = ctx
            .allocate_buffer(width, height, format, modifier)
            .context("Failed to allocate buffer 1")?;
        let buffer2 = ctx
            .allocate_buffer(width, height, format, modifier)
            .context("Failed to allocate buffer 2")?;

        log::info!("Triple buffer pool allocated successfully");

        Ok(Self {
            buffers: [buffer0, buffer1, buffer2],
            current_index: 0,
        })
    }

    /// Get the current buffer for capture
    pub const fn current(&self) -> &DmabufBuffer {
        &self.buffers[self.current_index]
    }

    /// Width the buffers were laid out at. The pipeline's caps describe this.
    pub const fn padded_width(&self) -> u32 {
        self.buffers[0].padded_width
    }

    /// The buffer captured before the current one: the last frame pushed.
    pub const fn last(&self) -> &DmabufBuffer {
        &self.buffers[(self.current_index + 2) % 3]
    }

    /// Advance to the next buffer
    pub const fn advance(&mut self) {
        self.current_index = (self.current_index + 1) % 3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual GPU check for the zero-copy import: the probe must find a fourcc
    /// description under which a red buffer reads back red, and a recording made
    /// with it must be red. Run with
    /// `cargo test -p cosmic-x dmabuf_roundtrip -- --ignored --nocapture`, then
    /// decode `/tmp/cosmic-x-roundtrip.mp4`.
    #[test]
    #[ignore = "needs a GPU, a VA driver and vapostproc"]
    fn dmabuf_roundtrip() {
        use super::super::encoder::{Codec, EncoderInfo};
        use super::super::pipeline::{Pipeline, probe_import_fourcc, vapostproc_dmabuf_formats};

        let _ = env_logger::builder().is_test(true).try_init();
        gstreamer::init().unwrap();
        let ctx = DmabufContext::new().unwrap();
        let importer = vapostproc_dmabuf_formats();
        // Pretend the compositor offers exactly what the importer takes.
        let (format, modifier) = select_zero_copy_source_format(&importer, &importer)
            .expect("vapostproc offers no RGB DMA-BUF format");
        let encoder = EncoderInfo {
            name: "VA-API H.264".into(),
            gst_element: "vah264enc".into(),
            codec: Codec::H264,
            hardware: true,
            supports_dmabuf_zero_copy: true,
            priority: 10,
        };
        let import = probe_import_fourcc(&ctx, format, modifier, &encoder).unwrap();
        eprintln!("memory {format:?} modifier {modifier:?} imported as {import:?}");

        // `ROUNDTRIP_SIZE=WxH` tries a real screen size.
        let (w, h) = std::env::var("ROUNDTRIP_SIZE")
            .ok()
            .map_or((256u32, 256u32), |s| {
                let (w, h) = s.split_once('x').unwrap();
                (w.parse().unwrap(), h.parse().unwrap())
            });
        let buffer = ctx
            .allocate_filled(w, h, format, modifier, red_pixel(format).unwrap())
            .unwrap();
        let pipeline = Pipeline::new_dmabuf(
            &encoder,
            crate::config::Container::Mp4,
            std::path::Path::new("/tmp/cosmic-x-roundtrip.mp4"),
            w,
            h,
            buffer.padded_width,
            None,
            30,
            import,
            modifier,
        )
        .unwrap();
        pipeline.start().unwrap();
        // `PTS_STEP_MS` spaces the frames out, to check timestamps survive to the file.
        let step_ns: u64 = std::env::var("PTS_STEP_MS")
            .ok()
            .map_or(33_333_333, |s| s.parse::<u64>().unwrap() * 1_000_000);
        for i in 0..30u64 {
            pipeline.push_dmabuf_frame(&buffer, i * step_ns).unwrap();
            // Real time, or the live queue drops the burst.
            std::thread::sleep(std::time::Duration::from_nanos(step_ns));
        }
        pipeline.finish().unwrap();
    }

    /// Manual GPU check for zero-copy cropping: quadrants red, green, blue, white
    /// cropped to a wide strip across all four. Decode `/tmp/cosmic-x-crop.mp4`:
    /// it must be 320x128, red|green on top and blue|white below, split at x=128.
    #[test]
    #[ignore = "needs a GPU, a VA driver and vapostproc"]
    fn dmabuf_crop_roundtrip() {
        use super::super::encoder::{Codec, EncoderInfo};
        use super::super::pipeline::{
            CropRegion, Pipeline, probe_import_fourcc, vapostproc_dmabuf_formats,
        };

        let _ = env_logger::builder().is_test(true).try_init();
        gstreamer::init().unwrap();
        let ctx = DmabufContext::new().unwrap();
        let importer = vapostproc_dmabuf_formats();
        let (format, modifier) = select_zero_copy_source_format(&importer, &importer).unwrap();
        let encoder = EncoderInfo {
            name: "VA-API H.264".into(),
            gst_element: "vah264enc".into(),
            codec: Codec::H264,
            hardware: true,
            supports_dmabuf_zero_copy: true,
            priority: 10,
        };
        let import = probe_import_fourcc(&ctx, format, modifier, &encoder).unwrap();
        let (w, h) = (512u32, 512u32);
        // B, G, R, X byte order (the memory format is ARGB or XRGB).
        let quadrants = |x: u32, y: u32| -> [u8; 4] {
            match (x < 256, y < 256) {
                (true, true) => [0, 0, 255, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [255, 0, 0, 255],
                (false, false) => [255, 255, 255, 255],
            }
        };
        let buffer = ctx
            .allocate(w, h, format, modifier, Some(&quadrants))
            .unwrap();
        // `CROP=left,top,width,height` overrides the region, to try odd sizes.
        let crop = std::env::var("CROP").ok().map_or(
            CropRegion {
                left: 128,
                top: 192,
                width: 320,
                height: 128,
            },
            |s| {
                let v: Vec<u32> = s.split(',').map(|n| n.parse().unwrap()).collect();
                CropRegion {
                    left: v[0],
                    top: v[1],
                    width: v[2],
                    height: v[3],
                }
            },
        );
        let pipeline = Pipeline::new_dmabuf(
            &encoder,
            crate::config::Container::Mp4,
            std::path::Path::new("/tmp/cosmic-x-crop.mp4"),
            w,
            h,
            buffer.padded_width,
            Some(crop),
            30,
            import,
            modifier,
        )
        .unwrap();
        pipeline.start().unwrap();
        for i in 0..30u64 {
            pipeline.push_dmabuf_frame(&buffer, i * 33_333_333).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        pipeline.finish().unwrap();
    }

    #[test]
    fn zero_copy_format_needs_both_sides() {
        let tiled = DrmModifier::from(0x0200_0000_1040_1b04);
        let compositor = vec![
            (DrmFourcc::Argb8888, vec![DrmModifier::Linear, tiled]),
            (DrmFourcc::Xrgb8888, vec![DrmModifier::Linear, tiled]),
        ];
        // The importer only takes tiled RGB: pick the opaque format with that modifier.
        let importer = vec![
            (DrmFourcc::Xrgb8888, vec![tiled]),
            (DrmFourcc::Argb8888, vec![tiled]),
        ];
        assert_eq!(
            select_zero_copy_source_format(&compositor, &importer),
            Some((DrmFourcc::Xrgb8888, tiled))
        );
        // Linear is chosen when both offer it.
        let importer = vec![(DrmFourcc::Xrgb8888, vec![tiled, DrmModifier::Linear])];
        assert_eq!(
            select_zero_copy_source_format(&compositor, &importer),
            Some((DrmFourcc::Xrgb8888, DrmModifier::Linear))
        );
        // No common format, so no zero-copy.
        let importer = vec![(DrmFourcc::Nv12, vec![tiled])];
        assert_eq!(select_zero_copy_source_format(&compositor, &importer), None);
    }

    #[test]
    fn test_find_render_node() {
        // This test may fail on systems without a GPU
        if let Ok(node) = find_render_node() {
            assert!(node.exists());
            assert!(node.to_string_lossy().contains("dri"));
        }
    }
}
