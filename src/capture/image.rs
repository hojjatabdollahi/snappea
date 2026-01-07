//! Screenshot image type for captured screenshots

use image::RgbaImage;
use rustix::fd::AsFd;

use crate::wayland::ShmImage;

/// A captured screenshot image with both raw RGBA data and a display handle
#[derive(Clone, Debug)]
pub struct ScreenshotImage {
    pub rgba: RgbaImage,
    pub handle: cosmic::widget::image::Handle,
}

impl ScreenshotImage {
    /// Create a new ScreenshotImage from a wayland ShmImage
    pub fn new<T: AsFd>(img: ShmImage<T>) -> anyhow::Result<Self> {
        let rgba = img.image_transformed()?;
        log::debug!(
            "ScreenshotImage captured: {}x{} pixels",
            rgba.width(),
            rgba.height()
        );
        let handle = cosmic::widget::image::Handle::from_rgba(
            rgba.width(),
            rgba.height(),
            rgba.clone().into_vec(),
        );
        Ok(Self { rgba, handle })
    }

    /// Get the width of the image
    pub fn width(&self) -> u32 {
        self.rgba.width()
    }

    /// Get the height of the image
    pub fn height(&self) -> u32 {
        self.rgba.height()
    }
}
