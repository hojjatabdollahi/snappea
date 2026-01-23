//! DMA-buf buffer management for zero-copy screen capture
//!
//! This module provides GBM device initialization and DMA-buf buffer
//! allocation for efficient zero-copy frame capture.

use anyhow::{Context, Result};
use drm_fourcc::{DrmFourcc, DrmModifier};
use std::fs::{File, OpenOptions};
use std::os::fd::OwnedFd;
use std::path::PathBuf;

/// DMA-buf buffer information
pub struct DmabufBuffer {
    /// DMA-buf file descriptor
    pub fd: OwnedFd,
    /// Buffer width in pixels
    pub width: u32,
    /// Buffer height in pixels
    pub height: u32,
    /// Buffer stride in bytes
    pub stride: u32,
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
    /// Path to the render node
    render_node: PathBuf,
}

impl DmabufContext {
    /// Create a new DMA-buf context by opening a GPU render node
    pub fn new() -> Result<Self> {
        let render_node = find_render_node()
            .context("Failed to find GPU render node")?;

        log::info!("Using GPU render node: {}", render_node.display());

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&render_node)
            .with_context(|| format!("Failed to open render node: {}", render_node.display()))?;

        let gbm_device = gbm::Device::new(file)
            .map_err(|e| anyhow::anyhow!("Failed to create GBM device: {:?}", e))?;

        Ok(Self {
            gbm_device,
            render_node,
        })
    }

    /// Get the render node path
    pub fn render_node(&self) -> &PathBuf {
        &self.render_node
    }

    /// Allocate a DMA-buf buffer with the specified dimensions and format
    pub fn allocate_buffer(
        &self,
        width: u32,
        height: u32,
        format: DrmFourcc,
        modifier: DrmModifier,
    ) -> Result<DmabufBuffer> {
        let usage = gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR;

        let bo = if modifier == DrmModifier::Linear || modifier == DrmModifier::Invalid {
            // Use simple allocation for linear/implicit modifier
            self.gbm_device
                .create_buffer_object::<()>(width, height, format, usage)
                .map_err(|e| anyhow::anyhow!("Failed to create GBM buffer object: {:?}", e))?
        } else {
            // Use modifier-aware allocation
            self.gbm_device
                .create_buffer_object_with_modifiers2::<()>(
                    width,
                    height,
                    format,
                    [modifier].into_iter(),
                    usage,
                )
                .map_err(|e| anyhow::anyhow!("Failed to create GBM buffer object with modifier: {:?}", e))?
        };

        let stride = bo.stride();
        let fd = bo.fd().map_err(|e| anyhow::anyhow!("Failed to export DMA-buf fd: {:?}", e))?;
        // Get the actual modifier used by GBM (may differ from requested)
        let actual_modifier = bo.modifier();

        // Calculate buffer size
        let size = (stride * height) as usize;

        log::debug!(
            "Allocated DMA-buf: {}x{}, format={:?}, modifier={:?}, stride={}, size={}",
            width,
            height,
            format,
            actual_modifier,
            stride,
            size
        );

        Ok(DmabufBuffer {
            fd,
            width,
            height,
            stride,
            format,
            modifier: actual_modifier,
            size,
        })
    }

    /// Check if a specific format/modifier combination is supported
    pub fn is_format_supported(&self, format: DrmFourcc, modifier: DrmModifier) -> bool {
        // Try to create a small test buffer
        let usage = gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR;

        if modifier == DrmModifier::Linear || modifier == DrmModifier::Invalid {
            self.gbm_device
                .create_buffer_object::<()>(64, 64, format, usage)
                .is_ok()
        } else {
            self.gbm_device
                .create_buffer_object_with_modifiers2::<()>(
                    64,
                    64,
                    format,
                    [modifier].into_iter(),
                    usage,
                )
                .is_ok()
        }
    }
}

/// Find a usable GPU render node (typically /dev/dri/renderD128)
fn find_render_node() -> Result<PathBuf> {
    let dri_path = PathBuf::from("/dev/dri");

    if !dri_path.exists() {
        return Err(anyhow::anyhow!("/dev/dri does not exist - no GPU available?"));
    }

    // Look for render nodes (renderD*)
    for entry in std::fs::read_dir(&dri_path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with("renderD") {
            let path = entry.path();
            // Check if we can open it
            if OpenOptions::new().read(true).write(true).open(&path).is_ok() {
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
            if OpenOptions::new().read(true).write(true).open(&path).is_ok() {
                log::warn!("Using card node {} instead of render node", path.display());
                return Ok(path);
            }
        }
    }

    Err(anyhow::anyhow!("No usable GPU device found in /dev/dri"))
}

/// Select the best DRM format for video encoding based on available formats
/// and encoder capabilities
pub fn select_best_format(
    available_formats: &[(DrmFourcc, Vec<DrmModifier>)],
    prefer_hardware_encoder: bool,
) -> Option<(DrmFourcc, DrmModifier)> {
    // Priority order depends on encoder type
    let preferred_formats: Vec<DrmFourcc> = if prefer_hardware_encoder {
        // Hardware encoders prefer YUV formats (less conversion needed)
        vec![
            DrmFourcc::Nv12,
            DrmFourcc::Xrgb8888,
            DrmFourcc::Argb8888,
            DrmFourcc::Xbgr8888,
            DrmFourcc::Abgr8888,
        ]
    } else {
        // Software encoders work well with RGB
        vec![
            DrmFourcc::Xrgb8888,
            DrmFourcc::Argb8888,
            DrmFourcc::Xbgr8888,
            DrmFourcc::Abgr8888,
            DrmFourcc::Nv12,
        ]
    };

    for preferred in &preferred_formats {
        for (format, modifiers) in available_formats {
            if format == preferred {
                // Prefer linear modifier for maximum compatibility
                let modifier = modifiers
                    .iter()
                    .find(|&&m| m == DrmModifier::Linear)
                    .copied()
                    .or_else(|| modifiers.first().copied())
                    .unwrap_or(DrmModifier::Invalid);

                return Some((*format, modifier));
            }
        }
    }

    // Fall back to first available format
    available_formats.first().and_then(|(format, modifiers)| {
        let modifier = modifiers.first().copied().unwrap_or(DrmModifier::Invalid);
        Some((*format, modifier))
    })
}

/// Convert DRM fourcc to GStreamer video format string
pub fn drm_format_to_gst_format(format: DrmFourcc) -> Option<&'static str> {
    match format {
        DrmFourcc::Xrgb8888 => Some("BGRx"),
        DrmFourcc::Argb8888 => Some("BGRA"),
        DrmFourcc::Xbgr8888 => Some("RGBx"),
        DrmFourcc::Abgr8888 => Some("RGBA"),
        DrmFourcc::Nv12 => Some("NV12"),
        DrmFourcc::Yuyv => Some("YUY2"),
        _ => None,
    }
}

/// Triple buffer pool for efficient frame capture
///
/// Uses 3 buffers to allow:
/// - Buffer 0: Being captured by compositor
/// - Buffer 1: Being encoded by GStreamer
/// - Buffer 2: Ready for next capture
pub struct TripleBufferPool {
    buffers: [DmabufBuffer; 3],
    current_index: usize,
}

impl TripleBufferPool {
    /// Create a new triple buffer pool
    pub fn new(ctx: &DmabufContext, width: u32, height: u32, format: DrmFourcc, modifier: DrmModifier) -> Result<Self> {
        log::info!("Allocating triple buffer pool: {}x{}, format={:?}", width, height, format);

        let buffer0 = ctx.allocate_buffer(width, height, format, modifier)
            .context("Failed to allocate buffer 0")?;
        let buffer1 = ctx.allocate_buffer(width, height, format, modifier)
            .context("Failed to allocate buffer 1")?;
        let buffer2 = ctx.allocate_buffer(width, height, format, modifier)
            .context("Failed to allocate buffer 2")?;

        log::info!("Triple buffer pool allocated successfully");

        Ok(Self {
            buffers: [buffer0, buffer1, buffer2],
            current_index: 0,
        })
    }

    /// Get the current buffer for capture
    pub fn current(&self) -> &DmabufBuffer {
        &self.buffers[self.current_index]
    }

    /// Advance to the next buffer
    pub fn advance(&mut self) {
        self.current_index = (self.current_index + 1) % 3;
    }

    /// Get buffer info (format, modifier, etc.) - all buffers have same properties
    pub fn format(&self) -> DrmFourcc {
        self.buffers[0].format
    }

    pub fn modifier(&self) -> DrmModifier {
        self.buffers[0].modifier
    }

    pub fn width(&self) -> u32 {
        self.buffers[0].width
    }

    pub fn height(&self) -> u32 {
        self.buffers[0].height
    }

    pub fn stride(&self) -> u32 {
        self.buffers[0].stride
    }

    pub fn size(&self) -> usize {
        self.buffers[0].size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_render_node() {
        // This test may fail on systems without a GPU
        if let Ok(node) = find_render_node() {
            assert!(node.exists());
            assert!(node.to_string_lossy().contains("dri"));
        }
    }
}
