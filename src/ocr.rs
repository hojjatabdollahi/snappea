//! OCR (Optical Character Recognition) module using rusty-tesseract

use image::RgbaImage;
use std::collections::HashMap;

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
pub struct OcrMapping {
    /// Top-left of the cropped OCR region in logical coordinates
    pub origin: (f32, f32),
    /// Size of the cropped OCR region in logical coordinates
    pub size: (f32, f32),
    /// Pixels-per-logical-unit for this output image
    pub scale: f32,
    /// Output name this mapping belongs to
    pub output_name: String,
}

/// Check if OCR models need to be downloaded.
/// For rusty-tesseract, this always returns false as it uses system tesseract.
pub fn models_need_download() -> bool {
    // rusty-tesseract uses system tesseract, no model download needed
    false
}

/// Run OCR on an image and return the status with detected text and overlays.
pub fn run_ocr_on_image_with_status(img: &RgbaImage, mapping: OcrMapping) -> OcrStatus {
    use rusty_tesseract::{Args, Image};

    if mapping.scale <= 0.0 {
        return OcrStatus::Error("Invalid OCR mapping scale".to_string());
    }

    log::info!(
        "Running OCR with rusty-tesseract on {}x{} image...",
        img.width(),
        img.height()
    );

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
        (
            dynamic_img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3),
            4.0_f32,
        )
    } else if min_dimension < 200 {
        // Small selection - upscale 2x
        let new_width = img.width() * 2;
        let new_height = img.height() * 2;
        log::info!("Upscaling small image 2x to {}x{}", new_width, new_height);
        (
            dynamic_img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3),
            2.0_f32,
        )
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
        psm: Some(11), // Fully automatic page segmentation
        oem: Some(3),  // Default OCR Engine Mode
    };

    // Run OCR for text
    let text_result = rusty_tesseract::image_to_string(&tess_img, &args);
    let data_result = rusty_tesseract::image_to_data(&tess_img, &args);

    let mut overlays = Vec::new();
    if let Ok(data_output) = data_result {
        log::info!("Tesseract returned {} data entries", data_output.data.len());

        // Group words by block_num to create block-level overlays
        let mut blocks: std::collections::HashMap<i32, Vec<_>> = std::collections::HashMap::new();
        for d in data_output
            .data
            .into_iter()
            .filter(|d| !d.text.trim().is_empty() && d.conf > 0.0)
        {
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
                a.line_num
                    .cmp(&b.line_num)
                    .then(a.word_num.cmp(&b.word_num))
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

            log::info!(
                "OCR block {}: '{}' at ({}, {}, {}x{})",
                block_num,
                block_text,
                left,
                top,
                width,
                height
            );
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
