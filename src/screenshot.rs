#![allow(dead_code, unused_variables)]

use cosmic::cosmic_config::CosmicConfigEntry;
use cosmic::iced::clipboard::mime::AsMimeTypes;
use cosmic::iced::keyboard::{Key, key::Named};
use cosmic::iced::{Limits, window};
use cosmic::iced_core::Length;
use cosmic::iced_runtime::clipboard;
use cosmic::iced_runtime::platform_specific::wayland::layer_surface::{
    IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced_winit::commands::layer_surface::{destroy_layer_surface, get_layer_surface};
use cosmic::widget::horizontal_space;
use cosmic_client_toolkit::sctk::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};
use futures::stream::{FuturesUnordered, StreamExt};
use image::RgbaImage;
use rustix::fd::AsFd;
use std::borrow::Cow;
use std::num::NonZeroU32;
use std::{collections::HashMap, io, path::PathBuf};
use tokio::sync::mpsc::Sender;

use wayland_client::protocol::wl_output::WlOutput;
use zbus::zvariant;

use crate::app::{App, OutputState};
use crate::wayland::{CaptureSource, ShmImage, WaylandHelper};
use crate::widget::{keyboard_wrapper::KeyboardWrapper, rectangle_selection::DragState};
use crate::{PortalResponse, fl};

#[derive(Clone, Debug)]
pub struct ScreenshotImage {
    pub rgba: RgbaImage,
    pub handle: cosmic::widget::image::Handle,
}

impl ScreenshotImage {
    fn new<T: AsFd>(img: ShmImage<T>) -> anyhow::Result<Self> {
        let rgba = img.image_transformed()?;
        let handle = cosmic::widget::image::Handle::from_rgba(
            rgba.width(),
            rgba.height(),
            rgba.clone().into_vec(),
        );
        Ok(Self { rgba, handle })
    }

    pub fn width(&self) -> u32 {
        self.rgba.width()
    }

    pub fn height(&self) -> u32 {
        self.rgba.height()
    }
}

/// Detected QR code with position and content
#[derive(Clone, Debug)]
pub struct DetectedQrCode {
    /// Center position in logical coordinates (relative to output)
    pub center_x: f32,
    pub center_y: f32,
    /// The decoded content of the QR code
    pub content: String,
    /// Which output this QR code is on
    pub output_name: String,
}

/// OCR text overlay metadata
#[derive(Clone, Debug, PartialEq)]
pub struct OcrTextOverlay {
    /// Bounding box in logical coordinates (relative to output)
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    /// Recognized text for this region
    pub text: String,
    /// Block number for coloring
    pub block_num: i32,
    /// Which output this overlay belongs to
    pub output_name: String,
}

/// Detect QR codes in an image at a specific resolution
/// max_dim: maximum dimension to downsample to (0 = no downsampling)
pub fn detect_qr_codes_at_resolution(
    img: &RgbaImage, 
    output_name: &str, 
    scale: f32,
    max_dim: u32,
) -> Vec<DetectedQrCode> {
    use rqrr::PreparedImage;
    
    let (orig_w, orig_h) = (img.width(), img.height());
    let downsample_factor = if max_dim > 0 && (orig_w > max_dim || orig_h > max_dim) {
        orig_w.max(orig_h) as f32 / max_dim as f32
    } else {
        1.0
    };
    
    let gray = if downsample_factor > 1.0 {
        let new_w = (orig_w as f32 / downsample_factor) as u32;
        let new_h = (orig_h as f32 / downsample_factor) as u32;
        let resized = image::imageops::resize(img, new_w, new_h, image::imageops::FilterType::Nearest);
        image::DynamicImage::ImageRgba8(resized).to_luma8()
    } else {
        image::DynamicImage::ImageRgba8(img.clone()).to_luma8()
    };
    
    let mut prepared = PreparedImage::prepare(gray);
    let grids = prepared.detect_grids();
    
    let mut results = Vec::new();
    for grid in grids {
        if let Ok((_, content)) = grid.decode() {
            let bounds = &grid.bounds;
            let cx = (bounds[0].x + bounds[1].x + bounds[2].x + bounds[3].x) as f32 / 4.0;
            let cy = (bounds[0].y + bounds[1].y + bounds[2].y + bounds[3].y) as f32 / 4.0;
            
            results.push(DetectedQrCode {
                center_x: (cx * downsample_factor) / scale,
                center_y: (cy * downsample_factor) / scale,
                content,
                output_name: output_name.to_string(),
            });
        }
    }
    
    results
}

/// Check if a QR code is a duplicate (same content at similar position)
fn is_duplicate_qr(existing: &[DetectedQrCode], new: &DetectedQrCode) -> bool {
    const POSITION_THRESHOLD: f32 = 50.0; // pixels
    existing.iter().any(|e| {
        e.content == new.content 
            && e.output_name == new.output_name
            && (e.center_x - new.center_x).abs() < POSITION_THRESHOLD
            && (e.center_y - new.center_y).abs() < POSITION_THRESHOLD
    })
}

// ============================================================================
// OCR Backend: rusty-tesseract (default)
// ============================================================================
#[cfg(not(feature = "ocrs"))]
fn models_need_download() -> bool {
    // rusty-tesseract uses system tesseract, no model download needed
    false
}

#[cfg(not(feature = "ocrs"))]
fn run_ocr_on_image_with_status(img: &RgbaImage, mapping: OcrMapping) -> OcrStatus {
    use rusty_tesseract::{Args, Image};
    use std::collections::HashMap;
    
    if mapping.scale <= 0.0 {
        return OcrStatus::Error("Invalid OCR mapping scale".to_string());
    }

    log::info!("Running OCR with rusty-tesseract on {}x{} image...", img.width(), img.height());
    
    // Convert RgbaImage to DynamicImage
    let dynamic_img = image::DynamicImage::ImageRgba8(img.clone());
    
    // For small images, upscale to improve OCR accuracy on small text
    // Tesseract works best with text that's at least 10-12 pixels tall
    let min_dimension = img.width().min(img.height());
    let (processed_img, upscale_factor) = if min_dimension < 100 {
        // Very small selection - upscale 4x
        let new_width = img.width() * 4;
        let new_height = img.height() * 4;
        log::info!("Upscaling small image 4x to {}x{}", new_width, new_height);
        (dynamic_img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3), 4.0_f32)
    } else if min_dimension < 200 {
        // Small selection - upscale 2x
        let new_width = img.width() * 2;
        let new_height = img.height() * 2;
        log::info!("Upscaling small image 2x to {}x{}", new_width, new_height);
        (dynamic_img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3), 2.0_f32)
    } else {
        (dynamic_img, 1.0_f32)
    };
    
    // Create rusty-tesseract Image from DynamicImage
    let tess_img = match Image::from_dynamic_image(&processed_img) {
        Ok(img) => img,
        Err(e) => {
            return OcrStatus::Error(format!("Failed to create tesseract image: {}", e));
        }
    };
    
    // Configure tesseract arguments
    // Use higher DPI for better small text recognition
    let dpi = if min_dimension < 200 { 300 } else { 150 };
    let args = Args {
        lang: "eng".to_string(),
        config_variables: HashMap::new(),
        dpi: Some(dpi),
        psm: Some(3), // Fully automatic page segmentation
        oem: Some(3), // Default OCR Engine Mode
    };
    
    // Run OCR for text
    let text_result = rusty_tesseract::image_to_string(&tess_img, &args);
    let data_result = rusty_tesseract::image_to_data(&tess_img, &args);

    let mut overlays = Vec::new();
    if let Ok(data_output) = data_result {
        log::info!("Tesseract returned {} data entries", data_output.data.len());
        
        // Group words by block_num to create block-level overlays
        let mut blocks: std::collections::HashMap<i32, Vec<_>> = std::collections::HashMap::new();
        for d in data_output.data.into_iter().filter(|d| !d.text.trim().is_empty() && d.conf > 0.0) {
            blocks.entry(d.block_num).or_default().push(d);
        }
        
        for (block_num, words) in blocks {
            if words.is_empty() {
                continue;
            }
            
            // Calculate bounding box for the entire block
            let mut min_left = i32::MAX;
            let mut min_top = i32::MAX;
            let mut max_right = i32::MIN;
            let mut max_bottom = i32::MIN;
            
            // Sort words by line_num then word_num for proper text ordering
            let mut sorted_words = words;
            sorted_words.sort_by(|a, b| {
                a.line_num.cmp(&b.line_num).then(a.word_num.cmp(&b.word_num))
            });
            
            // Build combined text and bounding box
            let mut text_parts: Vec<String> = Vec::new();
            let mut current_line = -1;
            
            for word in &sorted_words {
                min_left = min_left.min(word.left);
                min_top = min_top.min(word.top);
                max_right = max_right.max(word.left + word.width);
                max_bottom = max_bottom.max(word.top + word.height);
                
                if word.line_num != current_line {
                    if current_line != -1 {
                        text_parts.push(" ".to_string());
                    }
                    current_line = word.line_num;
                } else {
                    text_parts.push(" ".to_string());
                }
                text_parts.push(word.text.clone());
            }
            
            let block_text = text_parts.concat().trim().to_string();
            if block_text.is_empty() {
                continue;
            }
            
            // Convert bounding box to output-relative logical coords
            // Divide by upscale_factor first since tesseract coords are in upscaled image space
            let left = mapping.origin.0 + min_left as f32 / upscale_factor / mapping.scale;
            let top = mapping.origin.1 + min_top as f32 / upscale_factor / mapping.scale;
            let width = (max_right - min_left) as f32 / upscale_factor / mapping.scale;
            let height = (max_bottom - min_top) as f32 / upscale_factor / mapping.scale;
            
            log::info!("OCR block {}: '{}' at ({}, {}, {}x{})", block_num, block_text, left, top, width, height);
            overlays.push(OcrTextOverlay {
                left,
                top,
                width,
                height,
                text: block_text,
                block_num,
                output_name: mapping.output_name.clone(),
            });
        }
        log::info!("Generated {} block-level OCR overlays", overlays.len());
    }

    match text_result {
        Ok(text) => {
            let text = text.trim().to_string();
            let text = if text.is_empty() {
                "No text detected".to_string()
            } else {
                text
            };
            // If no blocks found, create a fallback overlay covering the whole selection
            if overlays.is_empty() && !text.is_empty() && text != "No text detected" {
                overlays.push(OcrTextOverlay {
                    left: mapping.origin.0,
                    top: mapping.origin.1,
                    width: mapping.size.0,
                    height: mapping.size.1,
                    text: text.clone(),
                    block_num: 0,
                    output_name: mapping.output_name.clone(),
                });
            }
            OcrStatus::Done(text, overlays)
        }
        Err(e) => OcrStatus::Error(format!("Tesseract OCR failed: {}", e)),
    }
}

// ============================================================================
// OCR Backend: ocrs (feature = "ocrs")
// ============================================================================
#[cfg(feature = "ocrs")]
const DETECTION_MODEL_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";
#[cfg(feature = "ocrs")]
const RECOGNITION_MODEL_URL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

#[cfg(feature = "ocrs")]
fn get_model_cache_dir() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("blazingshot")
        .join("models")
}

#[cfg(feature = "ocrs")]
fn download_model(url: &str, path: &std::path::Path) -> Result<(), String> {
    log::info!("Downloading OCR model from {} to {:?}", url, path);
    
    // Create parent directories
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create model directory: {}", e))?;
    }
    
    // Use curl or wget via subprocess (blocking, but we're in spawn_blocking)
    let output = std::process::Command::new("curl")
        .args(["-L", "-o"])
        .arg(path)
        .arg(url)
        .output()
        .map_err(|e| format!("Failed to run curl: {}", e))?;
    
    if !output.status.success() {
        return Err(format!(
            "curl failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    
    log::info!("Downloaded model to {:?}", path);
    Ok(())
}

#[cfg(feature = "ocrs")]
fn models_need_download() -> bool {
    let cache_dir = get_model_cache_dir();
    let detection_path = cache_dir.join("text-detection.rten");
    let recognition_path = cache_dir.join("text-recognition.rten");
    !detection_path.exists() || !recognition_path.exists()
}

#[cfg(feature = "ocrs")]
fn ensure_models_downloaded() -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let cache_dir = get_model_cache_dir();
    let detection_path = cache_dir.join("text-detection.rten");
    let recognition_path = cache_dir.join("text-recognition.rten");
    
    if !detection_path.exists() {
        download_model(DETECTION_MODEL_URL, &detection_path)?;
    }
    
    if !recognition_path.exists() {
        download_model(RECOGNITION_MODEL_URL, &recognition_path)?;
    }
    
    Ok((detection_path, recognition_path))
}

#[cfg(feature = "ocrs")]
fn run_ocr_on_image_with_status(img: &RgbaImage, mapping: OcrMapping) -> OcrStatus {
    use ocrs::{OcrEngine, OcrEngineParams, ImageSource};
    use rten::Model;
    
    if mapping.scale <= 0.0 {
        return OcrStatus::Error("Invalid OCR mapping scale".to_string());
    }

    // Ensure models are downloaded
    let (detection_path, recognition_path) = match ensure_models_downloaded() {
        Ok(paths) => paths,
        Err(e) => {
            return OcrStatus::Error(format!("Failed to download OCR models: {}", e));
        }
    };
    
    // Load models
    log::info!("Loading OCR models...");
    let detection_model = match Model::load_file(&detection_path) {
        Ok(m) => m,
        Err(e) => {
            return OcrStatus::Error(format!("Failed to load detection model: {}", e));
        }
    };
    
    let recognition_model = match Model::load_file(&recognition_path) {
        Ok(m) => m,
        Err(e) => {
            return OcrStatus::Error(format!("Failed to load recognition model: {}", e));
        }
    };
    
    // Create OCR engine with loaded models
    let engine = match OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    }) {
        Ok(e) => e,
        Err(e) => {
            return OcrStatus::Error(format!("Failed to create OCR engine: {}", e));
        }
    };
    
    // Convert to RGB8 for OCR
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let (width, height) = rgb.dimensions();
    
    // Create image source from raw pixels
    let img_source = match ImageSource::from_bytes(rgb.as_raw(), (width, height)) {
        Ok(src) => src,
        Err(e) => {
            return OcrStatus::Error(format!("Failed to create image source: {}", e));
        }
    };
    
    // Prepare input
    let ocr_input = match engine.prepare_input(img_source) {
        Ok(input) => input,
        Err(e) => {
            return OcrStatus::Error(format!("Failed to prepare OCR input: {}", e));
        }
    };
    
    // Use the simpler get_text method
    match engine.get_text(&ocr_input) {
        Ok(text) => {
            let text = text.trim().to_string();
            let text = if text.is_empty() {
                "No text detected".to_string()
            } else {
                text
            };

            // We do not have per-box data from ocrs, so create a box covering the whole selection
            let overlays = if !text.is_empty() && text != "No text detected" {
                vec![OcrTextOverlay {
                    left: mapping.origin.0,
                    top: mapping.origin.1,
                    width: mapping.size.0,
                    height: mapping.size.1,
                    text: text.clone(),
                    block_num: 0,
                    output_name: mapping.output_name.clone(),
                }]
            } else {
                vec![]
            };

            OcrStatus::Done(text, overlays)
        }
        Err(e) => OcrStatus::Error(format!("Failed to extract text: {}", e)),
    }
}

#[derive(zvariant::DeserializeDict, zvariant::Type, Clone, Debug)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenshotOptions {
    modal: Option<bool>,
    interactive: Option<bool>,
    choose_destination: Option<bool>,
}

#[derive(zvariant::SerializeDict, zvariant::Type)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenshotResult {
    uri: String,
}

struct ScreenshotBytes {
    bytes: Vec<u8>,
}

impl ScreenshotBytes {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl AsMimeTypes for ScreenshotBytes {
    fn available(&self) -> std::borrow::Cow<'static, [String]> {
        Cow::Owned(vec!["image/png".to_string()])
    }

    fn as_bytes(&self, mime_type: &str) -> Option<std::borrow::Cow<'static, [u8]>> {
        Some(Cow::Owned(self.bytes.clone()))
    }
}

#[derive(zvariant::SerializeDict, zvariant::Type)]
#[zvariant(signature = "a{sv}")]
struct PickColorResult {
    color: (f64, f64, f64),
}

/// Logical Size and Position of a rectangle
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn intersect(&self, other: Rect) -> Option<Rect> {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        let right = self.right.min(other.right);
        let bottom = self.bottom.min(other.bottom);
        if left < right && top < bottom {
            Some(Rect {
                left,
                top,
                right,
                bottom,
            })
        } else {
            None
        }
    }

    fn translate(&self, x: i32, y: i32) -> Rect {
        Rect {
            left: self.left + x,
            top: self.top + y,
            right: self.right + x,
            bottom: self.bottom + y,
        }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn dimensions(self) -> Option<RectDimension> {
        let width = NonZeroU32::new((self.width()).unsigned_abs())?;
        let height = NonZeroU32::new((self.height()).unsigned_abs())?;
        Some(RectDimension { width, height })
    }
}

#[derive(Clone, Copy)]
pub struct RectDimension {
    width: NonZeroU32,
    height: NonZeroU32,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSaveLocation {
    Clipboard,
    #[default]
    Pictures,
    Documents,
}

pub struct Screenshot {
    wayland_helper: WaylandHelper,
    tx: Sender<Event>,
}

impl Screenshot {
    pub fn new(wayland_helper: WaylandHelper, tx: Sender<Event>) -> Self {
        Self { wayland_helper, tx }
    }

    async fn interactive_toplevel_images(
        &self,
        outputs: &[Output],
    ) -> anyhow::Result<HashMap<String, Vec<ScreenshotImage>>> {
        let wayland_helper = self.wayland_helper.clone();
        Ok(outputs
            .iter()
            .map(move |Output { output, name, .. }| {
                let wayland_helper = wayland_helper.clone();
                async move {
                    let frame = wayland_helper
                        .capture_output_toplevels_shm(output, false)
                        .filter_map(|img| async { ScreenshotImage::new(img).ok() })
                        .collect()
                        .await;
                    (name.clone(), frame)
                }
            })
            .collect::<FuturesUnordered<_>>()
            .collect::<HashMap<String, _>>()
            .await)
    }

    async fn interactive_output_images(
        &self,
        outputs: &[Output],
        app_id: &str,
    ) -> anyhow::Result<HashMap<String, ScreenshotImage>> {
        let wayland_helper = self.wayland_helper.clone();

        let mut map = HashMap::with_capacity(outputs.len());
        for Output {
            output,
            logical_position: (output_x, output_y),
            name,
            ..
        } in outputs
        {
            let frame = wayland_helper
                .capture_source_shm(CaptureSource::Output(output.clone()), false)
                .await
                .ok_or_else(|| anyhow::anyhow!("shm screencopy failed"))?;
            map.insert(name.clone(), ScreenshotImage::new(frame)?);
        }

        Ok(map)
    }

    pub fn save_rgba(img: &RgbaImage, path: &PathBuf) -> anyhow::Result<()> {
        let mut file = std::fs::File::create(path)?;
        Ok(write_png(&mut file, img)?)
    }

    pub fn save_rgba_to_buffer(img: &RgbaImage, buffer: &mut Vec<u8>) -> anyhow::Result<()> {
        Ok(write_png(buffer, img)?)
    }

    pub fn get_img_path(location: ImageSaveLocation) -> Option<PathBuf> {
        let mut path = match location {
            ImageSaveLocation::Pictures => {
                dirs::picture_dir().or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
            }
            ImageSaveLocation::Documents => {
                dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
            }
            ImageSaveLocation::Clipboard => None,
        }?;
        let name = chrono::Local::now()
            .format("Screenshot_%Y-%m-%d_%H-%M-%S.png")
            .to_string();
        path.push(name);

        Some(path)
    }

    async fn screenshot_inner(&self, outputs: &[Output], app_id: &str) -> anyhow::Result<PathBuf> {
        let wayland_helper = self.wayland_helper.clone();

        let mut bounds_opt: Option<Rect> = None;
        let mut frames = Vec::with_capacity(outputs.len());
        for Output {
            output,
            logical_position: (output_x, output_y),
            logical_size: (output_w, output_h),
            ..
        } in outputs
        {
            let frame = wayland_helper
                .capture_source_shm(CaptureSource::Output(output.clone()), false)
                .await
                .ok_or_else(|| anyhow::anyhow!("shm screencopy failed"))?;
            let frame_image = frame.image_transformed()?;
            let rect = Rect {
                left: *output_x,
                top: *output_y,
                right: output_x.saturating_add(*output_w),
                bottom: output_y.saturating_add(*output_h),
            };
            bounds_opt = Some(match bounds_opt.take() {
                Some(bounds) => Rect {
                    left: bounds.left.min(rect.left),
                    top: bounds.top.min(rect.top),
                    right: bounds.right.max(rect.right),
                    bottom: bounds.bottom.max(rect.bottom),
                },
                None => rect,
            });
            frames.push((frame_image, rect));
        }

        let (file, path) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let image = combined_image(bounds_opt.unwrap_or_default(), frames);

            let mut file = tempfile::Builder::new()
                .prefix("screenshot-")
                .suffix(".png")
                .tempfile()?;
            {
                write_png(&mut file, &image)?;
            }
            Ok(file.keep()?)
        })
        .await??;

        Ok(path)
    }
}

fn combined_image(bounds: Rect, frames: Vec<(RgbaImage, Rect)>) -> RgbaImage {
    if frames.len() == 1 {
        let (frame_image, rect) = &frames[0];

        let width_scale = frame_image.width() as f64 / rect.width() as f64;
        let height_scale = frame_image.height() as f64 / rect.height() as f64;

        let width = (bounds.width() as f64 * width_scale).max(0.) as u32;
        let height = (bounds.height() as f64 * height_scale).max(0.) as u32;
        let x = ((bounds.left - rect.left) as f64 * width_scale).max(0.) as u32;
        let y = ((bounds.top - rect.top) as f64 * height_scale).max(0.) as u32;

        return image::imageops::crop_imm(frame_image, x, y, width, height).to_image();
    }

    let width = bounds
        .right
        .saturating_sub(bounds.left)
        .try_into()
        .unwrap_or_default();
    let height = bounds
        .bottom
        .saturating_sub(bounds.top)
        .try_into()
        .unwrap_or_default();
    let mut image = image::RgbaImage::new(width, height);
    for (mut frame_image, rect) in frames {
        let width = rect.width() as u32;
        let height = rect.height() as u32;
        if frame_image.dimensions() != (width, height) {
            frame_image = image::imageops::resize(
                &frame_image,
                width,
                height,
                image::imageops::FilterType::Lanczos3,
            );
        };
        let x = i64::from(rect.left) - i64::from(bounds.left);
        let y = i64::from(rect.top) - i64::from(bounds.top);
        image::imageops::overlay(&mut image, &frame_image, x, y);
    }
    image
}

fn write_png<W: io::Write>(w: W, image: &RgbaImage) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(w, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(image.as_raw())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum OcrStatus {
    #[default]
    Idle,
    DownloadingModels,
    Running,
    Done(String, Vec<OcrTextOverlay>),
    Error(String),
}

#[derive(Clone, Debug)]
struct OcrMapping {
    /// Top-left of the cropped OCR region in logical coordinates
    origin: (f32, f32),
    /// Size of the cropped OCR region in logical coordinates
    size: (f32, f32),
    /// Pixels-per-logical-unit for this output image
    scale: f32,
    /// Output name this mapping belongs to
    output_name: String,
}

/// Radial menu option
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialMenuOption {
    Region,
    Window,
    Display,
    Cancel,
}

/// Radial menu state for right-click context menu
#[derive(Debug, Clone, Default)]
pub struct RadialMenuState {
    /// Whether the menu is visible
    pub visible: bool,
    /// Center position of the menu (in global logical coordinates)
    pub center: (f32, f32),
    /// Currently highlighted option (based on mouse position)
    pub highlighted: Option<RadialMenuOption>,
    /// Current output name where menu was opened
    pub output_name: String,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Capture,
    Cancel,
    Choice(Choice),
    OutputChanged(WlOutput),
    WindowChosen(String, usize),
    Location(usize),
    QrCodesDetected(Vec<DetectedQrCode>),
    QrRequested,
    QrCopyAndClose,
    OcrRequested,
    OcrCopyAndClose,
    OcrStatus(OcrStatus),
    OcrStatusClear,
    RadialMenuOpen(f32, f32, String),  // x, y, output_name
    RadialMenuUpdate(Option<RadialMenuOption>),  // highlighted option
    RadialMenuSelect,  // select current highlighted option
}

#[derive(Debug, Clone)]
pub enum Choice {
    Output(String),
    Rectangle(Rect, DragState),
    Window(String, Option<usize>),
}

#[derive(Debug, Clone, Default)]
pub enum Action {
    #[default]
    ReturnPath,
    SaveToClipboard,
    SaveToPictures,
    SaveToDocuments,
    ChooseFolder,
    Choice(Choice),
}

#[derive(Clone, Debug)]
pub struct Args {
    pub handle: zvariant::ObjectPath<'static>,
    pub app_id: String,
    pub parent_window: String,
    pub options: ScreenshotOptions,
    pub output_images: HashMap<String, ScreenshotImage>,
    pub toplevel_images: HashMap<String, Vec<ScreenshotImage>>,
    pub tx: Sender<PortalResponse<ScreenshotResult>>,
    pub choice: Choice,
    pub location: ImageSaveLocation,
    pub action: Action,
    pub qr_codes: Vec<DetectedQrCode>,
    pub qr_scanning: bool,
    pub ocr_status: OcrStatus,
    pub ocr_overlays: Vec<OcrTextOverlay>,
    /// OCR text result for copying (stored separately from status)
    pub ocr_text: Option<String>,
    /// Radial menu state for right-click context menu
    pub radial_menu: RadialMenuState,
}

struct Output {
    output: WlOutput,
    logical_position: (i32, i32),
    logical_size: (i32, i32),
    name: String,
}

#[derive(Clone, Debug)]
pub enum Event {
    Screenshot(Args),
    Init(Sender<Event>),
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl Screenshot {
    async fn screenshot(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        handle: zvariant::ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: ScreenshotOptions,
    ) -> PortalResponse<ScreenshotResult> {
        let mut outputs = Vec::new();
        for output in self.wayland_helper.outputs() {
            let Some(info) = self.wayland_helper.output_info(&output) else {
                log::warn!("Output {:?} has no info", output);
                continue;
            };
            let Some(name) = info.name.clone() else {
                log::warn!("Output {:?} has no name", output);
                continue;
            };
            let Some(logical_position) = info.logical_position else {
                log::warn!("Output {:?} has no position", output);
                continue;
            };
            let Some(logical_size) = info.logical_size else {
                log::warn!("Output {:?} has no size", output);
                continue;
            };
            outputs.push(Output {
                output,
                logical_position,
                logical_size,
                name,
            });
        }
        if outputs.is_empty() {
            log::error!("No output");
            return PortalResponse::Other;
        };

        // Always interactive for blazingshot
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let first_output = &*outputs[0].name;
        let output_images = self
            .interactive_output_images(&outputs, app_id)
            .await
            .unwrap_or_default();
        let toplevel_images = self
            .interactive_toplevel_images(&outputs)
            .await
            .unwrap_or_default();
        
        let choice = Choice::Rectangle(Rect::default(), DragState::default());
        
        // Send UI immediately with empty QR codes, detection happens async
        if let Err(err) = self
            .tx
            .send(Event::Screenshot(Args {
                handle: handle.to_owned(),
                app_id: app_id.to_string(),
                parent_window: parent_window.to_string(),
                action: if options.choose_destination.unwrap_or_default() {
                    Action::SaveToClipboard
                } else {
                    Action::ReturnPath
                },
                options,
                output_images,
                toplevel_images,
                tx,
                location: ImageSaveLocation::Pictures,
                choice,
                qr_codes: Vec::new(),
                qr_scanning: false,
                ocr_status: OcrStatus::Idle,
                ocr_overlays: Vec::new(),
                ocr_text: None,
                radial_menu: RadialMenuState::default(),
            }))
            .await
        {
            log::error!("Failed to send screenshot event, {}", err);
            return PortalResponse::Other;
        }
        if let Some(res) = rx.recv().await {
            return res;
        } else {
            return PortalResponse::Cancelled::<ScreenshotResult>;
        }
    }

    async fn pick_color(
        &self,
        handle: zvariant::ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        option: HashMap<String, zvariant::Value<'_>>,
    ) -> PortalResponse<PickColorResult> {
        // TODO: implement color picker
        PortalResponse::Other
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

pub(crate) fn view(app: &App, id: window::Id) -> cosmic::Element<'_, Msg> {
    let Some((i, output)) = app.outputs.iter().enumerate().find(|(i, o)| o.id == id) else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };
    let Some(args) = app.screenshot_args.as_ref() else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };

    let Some(img) = args.output_images.get(&output.name) else {
        return horizontal_space().width(Length::Fixed(1.0)).into();
    };
    let theme = app.core.system_theme().cosmic();
    let output_name = output.name.clone();
    KeyboardWrapper::new(
        crate::widget::screenshot::ScreenshotSelection::new(
            args.choice.clone(),
            img,
            Msg::Capture,
            Msg::Cancel,
            Msg::OcrRequested,
            Msg::OcrCopyAndClose,
            Msg::QrRequested,
            Msg::QrCopyAndClose,
            output,
            id,
            Msg::OutputChanged,
            Msg::Choice,
            &args.toplevel_images,
            Msg::WindowChosen,
            &app.location_options,
            args.location as usize,
            Msg::Location,
            theme.spacing,
            i as u128,
            &args.qr_codes,
            args.qr_scanning,
            &args.ocr_overlays,
            args.ocr_status.clone(),
            args.ocr_text.is_some(),
            &args.radial_menu,
            move |x, y, name| Msg::RadialMenuOpen(x, y, name),
            Msg::RadialMenuUpdate,
            Msg::RadialMenuSelect,
        ),
        |key| match key {
            Key::Named(Named::Enter) => Some(Msg::Capture),
            Key::Named(Named::Escape) => Some(Msg::Cancel),
            _ => None,
        },
    )
    .into()
}

pub fn update_msg(app: &mut App, msg: Msg) -> cosmic::Task<crate::app::Msg> {
    match msg {
        Msg::Capture => {
            let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = app
                .outputs
                .iter()
                .map(|o| destroy_layer_surface(o.id))
                .collect();
            let Some(args) = app.screenshot_args.take() else {
                log::error!("Failed to find screenshot Args for Capture message.");
                return cosmic::Task::batch(cmds);
            };
            let outputs = app.outputs.clone();
            let Args {
                tx,
                choice,
                output_images: mut images,
                location,
                ..
            } = args;

            let mut success = true;
            let image_path = Screenshot::get_img_path(location);

            match choice {
                Choice::Output(name) => {
                    if let Some(img) = images.remove(&name) {
                        if let Some(ref image_path) = image_path {
                            if let Err(err) = Screenshot::save_rgba(&img.rgba, image_path) {
                                log::error!("Failed to capture screenshot: {:?}", err);
                            };
                        } else {
                            let mut buffer = Vec::new();
                            if let Err(e) = Screenshot::save_rgba_to_buffer(&img.rgba, &mut buffer)
                            {
                                log::error!("Failed to save screenshot to buffer: {:?}", e);
                                success = false;
                            } else {
                                cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)))
                            };
                        }
                    } else {
                        log::error!("Failed to find output {}", name);
                        success = false;
                    }
                }
                Choice::Rectangle(r, s) => {
                    if let Some(RectDimension { width, height }) = r.dimensions() {
                        // Calculate the scale factor from the first intersecting output
                        // to determine target resolution
                        let target_scale = images
                            .iter()
                            .find_map(|(name, raw_img)| {
                                let output = outputs.iter().find(|o| o.name == *name)?;
                                let output_rect = Rect {
                                    left: output.logical_pos.0,
                                    top: output.logical_pos.1,
                                    right: output.logical_pos.0 + output.logical_size.0 as i32,
                                    bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                };
                                r.intersect(output_rect)?;
                                Some(raw_img.rgba.width() as f32 / output.logical_size.0 as f32)
                            })
                            .unwrap_or(1.0);
                        
                        // Scale selection rect to physical coordinates
                        let physical_bounds = Rect {
                            left: (r.left as f32 * target_scale) as i32,
                            top: (r.top as f32 * target_scale) as i32,
                            right: (r.right as f32 * target_scale) as i32,
                            bottom: (r.bottom as f32 * target_scale) as i32,
                        };
                        
                        let frames = images
                            .into_iter()
                            .filter_map(|(name, raw_img)| {
                                let output = outputs.iter().find(|o| o.name == name)?;
                                let pos = output.logical_pos;
                                let output_rect = Rect {
                                    left: pos.0,
                                    top: pos.1,
                                    right: pos.0 + output.logical_size.0 as i32,
                                    bottom: pos.1 + output.logical_size.1 as i32,
                                };

                                let intersect = r.intersect(output_rect)?;
                                
                                // Crop to intersection in physical coordinates
                                let scale_x = raw_img.rgba.width() as f32 / output.logical_size.0 as f32;
                                let scale_y = raw_img.rgba.height() as f32 / output.logical_size.1 as f32;
                                
                                let img_x = ((intersect.left - output_rect.left) as f32 * scale_x) as u32;
                                let img_y = ((intersect.top - output_rect.top) as f32 * scale_y) as u32;
                                let img_w = (intersect.width() as f32 * scale_x) as u32;
                                let img_h = (intersect.height() as f32 * scale_y) as u32;
                                
                                let cropped = image::imageops::crop_imm(&raw_img.rgba, img_x, img_y, img_w, img_h).to_image();
                                
                                // Physical rect for this cropped portion
                                let physical_intersect = Rect {
                                    left: (intersect.left as f32 * target_scale) as i32,
                                    top: (intersect.top as f32 * target_scale) as i32,
                                    right: (intersect.right as f32 * target_scale) as i32,
                                    bottom: (intersect.bottom as f32 * target_scale) as i32,
                                };

                                Some((cropped, physical_intersect))
                            })
                            .collect::<Vec<_>>();
                        let img = combined_image(physical_bounds, frames);

                        if let Some(ref image_path) = image_path {
                            if let Err(err) = Screenshot::save_rgba(&img, image_path) {
                                success = false;
                            }
                        } else {
                            let mut buffer = Vec::new();
                            if let Err(e) = Screenshot::save_rgba_to_buffer(&img, &mut buffer) {
                                log::error!("Failed to save screenshot to buffer: {:?}", e);
                                success = false;
                            } else {
                                cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)))
                            };
                        }
                    } else {
                        success = false;
                    }
                }
                Choice::Window(output, Some(window_i)) => {
                    if let Some(img) = args
                        .toplevel_images
                        .get(&output)
                        .and_then(|imgs| imgs.get(window_i))
                    {
                        if let Some(ref image_path) = image_path {
                            if let Err(err) = Screenshot::save_rgba(&img.rgba, image_path) {
                                log::error!("Failed to capture screenshot: {:?}", err);
                                success = false;
                            }
                        } else {
                            let mut buffer = Vec::new();
                            if let Err(e) = Screenshot::save_rgba_to_buffer(&img.rgba, &mut buffer)
                            {
                                log::error!("Failed to save screenshot to buffer: {:?}", e);
                                success = false;
                            } else {
                                cmds.push(clipboard::write_data(ScreenshotBytes::new(buffer)))
                            };
                        }
                    } else {
                        success = false;
                    }
                }
                _ => {
                    success = false;
                }
            }

            let response = if success && image_path.is_some() {
                PortalResponse::Success(ScreenshotResult {
                    uri: format!("file:///{}", image_path.unwrap().display()),
                })
            } else if success && image_path.is_none() {
                PortalResponse::Success(ScreenshotResult {
                    uri: "clipboard:///".to_string(),
                })
            } else {
                PortalResponse::Other
            };

            tokio::spawn(async move {
                if let Err(err) = tx.send(response).await {
                    log::error!("Failed to send screenshot event");
                }
            });
            cosmic::Task::batch(cmds)
        }
        Msg::Cancel => {
            let cmds = app.outputs.iter().map(|o| destroy_layer_surface(o.id));
            let Some(args) = app.screenshot_args.take() else {
                log::error!("Failed to find screenshot Args for Cancel message.");
                return cosmic::Task::batch(cmds);
            };
            let Args { tx, .. } = args;
            tokio::spawn(async move {
                if let Err(err) = tx.send(PortalResponse::Cancelled).await {
                    log::error!("Failed to send screenshot event");
                }
            });

            cosmic::Task::batch(cmds)
        }
        Msg::Choice(c) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Clear OCR/QR overlays when rectangle changes (new selection started)
                if let Choice::Rectangle(new_r, new_s) = &c {
                    if let Choice::Rectangle(old_r, _) = &args.choice {
                        // If the rectangle position/size changed significantly, clear overlays
                        if new_r.left != old_r.left || new_r.top != old_r.top 
                            || new_r.right != old_r.right || new_r.bottom != old_r.bottom {
                            args.ocr_overlays.clear();
                            args.ocr_status = OcrStatus::Idle;
                            args.qr_codes.clear();
                        }
                    }
                    // Also clear if we're starting a new drag from None state
                    if *new_s != DragState::None {
                        args.ocr_overlays.clear();
                        args.ocr_status = OcrStatus::Idle;
                        args.qr_codes.clear();
                    }
                }
                args.choice = c;
                if let Choice::Rectangle(r, s) = &args.choice {
                    app.prev_rectangle = Some(*r);
                }
            } else {
                log::error!("Failed to find screenshot Args for Choice message.");
            }
            cosmic::Task::none()
        }
        Msg::OutputChanged(wl_output) => {
            if let (Some(args), Some(o)) = (
                app.screenshot_args.as_mut(),
                app
                    .outputs
                    .iter()
                    .find(|o| o.output == wl_output)
                    .map(|o| o.name.clone()),
            ) {
                args.choice = Choice::Output(o);
            } else {
                log::error!(
                    "Failed to find output for OutputChange message: {:?}",
                    wl_output
                );
            }
            app.active_output = Some(wl_output);
            cosmic::Task::none()
        }
        Msg::WindowChosen(name, i) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.choice = Choice::Window(name, Some(i));
            } else {
                log::error!("Failed to find screenshot Args for WindowChosen message.");
            }
            update_msg(app, Msg::Capture)
        }
        Msg::Location(loc) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                let loc = match loc {
                    loc if loc == ImageSaveLocation::Clipboard as usize => {
                        ImageSaveLocation::Clipboard
                    }
                    loc if loc == ImageSaveLocation::Pictures as usize => {
                        ImageSaveLocation::Pictures
                    }
                    loc if loc == ImageSaveLocation::Documents as usize => {
                        ImageSaveLocation::Documents
                    }
                    _ => args.location,
                };
                args.location = loc;
                cosmic::Task::none()
            } else {
                log::error!("Failed to find screenshot Args for Location message.");
                cosmic::Task::none()
            }
        }
        Msg::QrRequested => {
            // Clear previous QR codes and OCR overlays, start scanning
            if let Some(args) = app.screenshot_args.as_mut() {
                args.qr_codes.clear();
                args.qr_scanning = true;
                // Hide OCR overlays when running QR
                args.ocr_overlays.clear();
                args.ocr_status = OcrStatus::Idle;
                args.ocr_text = None;
            }
            
            // Get the rectangle selection and run QR detection on that area
            if let Some(args) = app.screenshot_args.as_ref() {
                if let Choice::Rectangle(rect, _) = &args.choice {
                    if rect.width() > 0 && rect.height() > 0 {
                        // Collect image data for the selected rectangle
                        for output in &app.outputs {
                            if let Some(img) = args.output_images.get(&output.name) {
                                let output_rect = Rect {
                                    left: output.logical_pos.0,
                                    top: output.logical_pos.1,
                                    right: output.logical_pos.0 + output.logical_size.0 as i32,
                                    bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                };
                                
                                if let Some(intersection) = rect.intersect(output_rect) {
                                    // Scale to image coordinates
                                    let scale = img.rgba.width() as f32 / output.logical_size.0 as f32;
                                    let x = ((intersection.left - output_rect.left) as f32 * scale) as u32;
                                    let y = ((intersection.top - output_rect.top) as f32 * scale) as u32;
                                    let w = (intersection.width() as f32 * scale) as u32;
                                    let h = (intersection.height() as f32 * scale) as u32;
                                    
                                    let cropped = image::imageops::crop_imm(&img.rgba, x, y, w, h).to_image();
                                    let output_name = output.name.clone();
                                    
                                    // Origin relative to output for QR overlay positions
                                    let origin_x = (intersection.left - output_rect.left) as f32;
                                    let origin_y = (intersection.top - output_rect.top) as f32;
                                    
                                    // Spawn progressive QR detection tasks (3 passes with increasing resolution)
                                    let resolutions = [500u32, 1500, 0]; // 0 = full resolution
                                    let mut qr_detection_tasks = Vec::new();
                                    
                                    for (pass_idx, max_dim) in resolutions.into_iter().enumerate() {
                                        let cropped_clone = cropped.clone();
                                        let output_name_clone = output_name.clone();
                                        let task = cosmic::Task::perform(
                                            async move {
                                                tokio::task::spawn_blocking(move || {
                                                    // Detect QR in cropped image, then adjust coordinates
                                                    let detected = detect_qr_codes_at_resolution(
                                                        &cropped_clone, 
                                                        &output_name_clone, 
                                                        scale, 
                                                        max_dim
                                                    );
                                                    // Adjust coordinates to be relative to output (add origin offset)
                                                    detected.into_iter().map(|mut qr| {
                                                        qr.center_x += origin_x;
                                                        qr.center_y += origin_y;
                                                        qr
                                                    }).collect::<Vec<_>>()
                                                }).await.unwrap_or_default()
                                            },
                                            move |qr_codes| crate::app::Msg::Screenshot(Msg::QrCodesDetected(qr_codes))
                                        );
                                        qr_detection_tasks.push(task);
                                    }
                                    
                                    return cosmic::Task::batch(qr_detection_tasks);
                                }
                            }
                        }
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::QrCodesDetected(new_qr_codes) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                // Scanning pass completed - hide scanning indicator after first pass
                args.qr_scanning = false;
                
                // Merge new QR codes, avoiding duplicates
                for qr in new_qr_codes {
                    if !is_duplicate_qr(&args.qr_codes, &qr) {
                        args.qr_codes.push(qr);
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::OcrRequested => {
            // Check if models need downloading and set appropriate status
            let needs_download = models_need_download();
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ocr_status = if needs_download {
                    OcrStatus::DownloadingModels
                } else {
                    OcrStatus::Running
                };
                // Clear previous OCR overlays when starting a new run
                args.ocr_overlays.clear();
                args.ocr_text = None;
                // Hide QR codes when running OCR
                args.qr_codes.clear();
            }
            
            // Get the rectangle selection and run OCR on that area
            if let Some(args) = app.screenshot_args.as_ref() {
                if let Choice::Rectangle(rect, _) = &args.choice {
                    if rect.width() > 0 && rect.height() > 0 {
                        // Collect image data for the selected rectangle
                        let mut region_data: Option<(RgbaImage, OcrMapping)> = None;
                        
                        for output in &app.outputs {
                            if let Some(img) = args.output_images.get(&output.name) {
                                let output_rect = Rect {
                                    left: output.logical_pos.0,
                                    top: output.logical_pos.1,
                                    right: output.logical_pos.0 + output.logical_size.0 as i32,
                                    bottom: output.logical_pos.1 + output.logical_size.1 as i32,
                                };
                                
                                if let Some(intersection) = rect.intersect(output_rect) {
                                    // Scale to image coordinates
                                    let scale = img.rgba.width() as f32 / output.logical_size.0 as f32;
                                    let x = ((intersection.left - output_rect.left) as f32 * scale) as u32;
                                    let y = ((intersection.top - output_rect.top) as f32 * scale) as u32;
                                    let w = (intersection.width() as f32 * scale) as u32;
                                    let h = (intersection.height() as f32 * scale) as u32;
                                    
                                    let cropped = image::imageops::crop_imm(&img.rgba, x, y, w, h).to_image();
                                    
                                    // Coordinates relative to this output's origin (like QR codes)
                                    let origin_x = (intersection.left - output_rect.left) as f32;
                                    let origin_y = (intersection.top - output_rect.top) as f32;
                                    let size_w = intersection.width() as f32;
                                    let size_h = intersection.height() as f32;
                                    
                                    region_data = Some((
                                        cropped,
                                        OcrMapping {
                                            origin: (origin_x, origin_y),
                                            size: (size_w, size_h),
                                            scale,
                                            output_name: output.name.clone(),
                                        },
                                    ));
                                    break;
                                }
                            }
                        }
                        
                        if let Some((cropped_img, mapping)) = region_data {
                            // Run OCR in background with status updates
                            return cosmic::Task::perform(
                                async move {
                                    tokio::task::spawn_blocking(move || {
                                        run_ocr_on_image_with_status(&cropped_img, mapping)
                                    }).await.unwrap_or_else(|_| OcrStatus::Error("OCR task panicked".to_string()))
                                },
                                |status| crate::app::Msg::Screenshot(Msg::OcrStatus(status))
                            );
                        }
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::OcrStatus(status) => {
            match &status {
                OcrStatus::Done(text, overlays) => {
                    log::info!("OCR Result: {} ({} overlays)", text, overlays.len());
                    for overlay in overlays.iter() {
                        log::info!("  Overlay block {}: ({}, {}, {}x{}) on {}", 
                            overlay.block_num, overlay.left, overlay.top, overlay.width, overlay.height, overlay.output_name);
                    }
                    if let Some(args) = app.screenshot_args.as_mut() {
                        args.ocr_status = status.clone();
                        args.ocr_overlays = overlays.clone();
                        // Store text for later copying when user clicks the button
                        if !text.is_empty() && text != "No text detected" {
                            args.ocr_text = Some(text.clone());
                        }
                        log::info!("Stored {} overlays in args", args.ocr_overlays.len());
                    }
                    // Don't auto-copy - user will click "copy text" button
                }
                OcrStatus::Error(err) => {
                    log::error!("OCR Error: {}", err);
                    if let Some(args) = app.screenshot_args.as_mut() {
                        args.ocr_status = status;
                        args.ocr_overlays.clear();
                        args.ocr_text = None;
                    }
                }
                _ => {
                    if let Some(args) = app.screenshot_args.as_mut() {
                        args.ocr_status = status;
                    }
                }
            }
            cosmic::Task::none()
        }
        Msg::OcrStatusClear => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.ocr_status = OcrStatus::Idle;
            }
            cosmic::Task::none()
        }
        Msg::OcrCopyAndClose => {
            // Copy OCR text and close the app
            let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = app
                .outputs
                .iter()
                .map(|o| destroy_layer_surface(o.id))
                .collect();
            
            if let Some(args) = app.screenshot_args.take() {
                let Args { tx, ocr_text, .. } = args;
                
                if let Some(text) = ocr_text {
                    cmds.push(clipboard::write(text));
                }
                
                tokio::spawn(async move {
                    if let Err(err) = tx.send(PortalResponse::Cancelled).await {
                        log::error!("Failed to send screenshot event");
                    }
                });
            }
            cosmic::Task::batch(cmds)
        }
        Msg::QrCopyAndClose => {
            // Copy first QR code content and close the app
            let mut cmds: Vec<cosmic::Task<crate::app::Msg>> = app
                .outputs
                .iter()
                .map(|o| destroy_layer_surface(o.id))
                .collect();
            
            if let Some(args) = app.screenshot_args.take() {
                let Args { tx, qr_codes, .. } = args;
                
                // Copy first QR code content
                if let Some(qr) = qr_codes.first() {
                    cmds.push(clipboard::write(qr.content.clone()));
                }
                
                tokio::spawn(async move {
                    if let Err(err) = tx.send(PortalResponse::Cancelled).await {
                        log::error!("Failed to send screenshot event");
                    }
                });
            }
            cosmic::Task::batch(cmds)
        }
        Msg::RadialMenuOpen(x, y, output_name) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.radial_menu = RadialMenuState {
                    visible: true,
                    center: (x, y),
                    highlighted: Some(RadialMenuOption::Cancel),
                    output_name,
                };
            }
            cosmic::Task::none()
        }
        Msg::RadialMenuUpdate(option) => {
            if let Some(args) = app.screenshot_args.as_mut() {
                args.radial_menu.highlighted = option;
            }
            cosmic::Task::none()
        }
        Msg::RadialMenuSelect => {
            if let Some(args) = app.screenshot_args.as_mut() {
                let selected = args.radial_menu.highlighted;
                let output_name = args.radial_menu.output_name.clone();
                
                // Hide the menu
                args.radial_menu.visible = false;
                
                match selected {
                    Some(RadialMenuOption::Region) => {
                        // Switch to rectangle selection mode
                        args.choice = Choice::Rectangle(Rect::default(), DragState::None);
                    }
                    Some(RadialMenuOption::Window) => {
                        // Switch to window selection mode
                        args.choice = Choice::Window(output_name, None);
                    }
                    Some(RadialMenuOption::Display) => {
                        // Switch to output/display selection mode
                        args.choice = Choice::Output(output_name);
                    }
                    Some(RadialMenuOption::Cancel) | None => {
                        // Do nothing, just close menu
                    }
                }
            }
            cosmic::Task::none()
        }
    }
}

pub fn update_args(app: &mut App, args: Args) -> cosmic::Task<crate::app::Msg> {
    let Args {
        handle,
        app_id,
        parent_window,
        options,
        output_images: images,
        tx,
        choice,
        action,
        location,
        toplevel_images,
        qr_codes: _,
        qr_scanning: _,
        ocr_status: _,
        ocr_overlays: _,
        ocr_text: _,
        radial_menu: _,
    } = &args;

    if app.outputs.len() != images.len() {
        log::error!(
            "Screenshot output count mismatch: {} != {}",
            app.outputs.len(),
            images.len()
        );
        log::warn!("Screenshot outputs: {:?}", app.outputs);
        log::warn!("Screenshot images: {:?}", images.keys().collect::<Vec<_>>());
        return cosmic::Task::none();
    }

    // update output bg sources
    if let Ok(c) = cosmic::cosmic_config::Config::new_state(
        cosmic_bg_config::NAME,
        cosmic_bg_config::state::State::version(),
    ) {
        let bg_state = match cosmic_bg_config::state::State::get_entry(&c) {
            Ok(state) => state,
            Err((err, s)) => {
                log::error!("Failed to get bg config state: {:?}", err);
                s
            }
        };
        for o in &mut app.outputs {
            let source = bg_state.wallpapers.iter().find(|s| s.0 == o.name);
            o.bg_source = Some(source.cloned().map(|s| s.1).unwrap_or_else(|| {
                cosmic_bg_config::Source::Path(
                    "/usr/share/backgrounds/pop/kate-hazen-COSMIC-desktop-wallpaper.png".into(),
                )
            }));
        }
    } else {
        log::error!("Failed to get bg config state");
        for o in &mut app.outputs {
            o.bg_source = Some(cosmic_bg_config::Source::Path(
                "/usr/share/backgrounds/pop/kate-hazen-COSMIC-desktop-wallpaper.png".into(),
            ));
        }
    }
    app.location_options = vec![
        fl!("save-to", "clipboard"),
        fl!("save-to", "pictures"),
        fl!("save-to", "documents"),
    ];

    if app.screenshot_args.replace(args).is_none() {
        let cmds: Vec<_> = app
            .outputs
            .iter()
            .map(
                |OutputState {
                     output, id, name, ..
                 }| {
                    get_layer_surface(SctkLayerSurfaceSettings {
                        id: *id,
                        layer: Layer::Overlay,
                        keyboard_interactivity: KeyboardInteractivity::Exclusive,
                        input_zone: None,
                        anchor: Anchor::all(),
                        output: IcedOutput::Output(output.clone()),
                        namespace: "blazingshot".to_string(),
                        size: Some((None, None)),
                        exclusive_zone: -1,
                        size_limits: Limits::NONE.min_height(1.0).min_width(1.0),
                        ..Default::default()
                    })
                },
            )
            .collect();
        cosmic::Task::batch(cmds)
    } else {
        log::info!("Existing screenshot args updated");
        cosmic::Task::none()
    }
}
