//! QR code detection module using rqrr

use image::RgbaImage;

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
pub fn is_duplicate_qr(existing: &[DetectedQrCode], new: &DetectedQrCode) -> bool {
    const POSITION_THRESHOLD: f32 = 50.0; // pixels
    existing.iter().any(|e| {
        e.content == new.content 
            && e.output_name == new.output_name
            && (e.center_x - new.center_x).abs() < POSITION_THRESHOLD
            && (e.center_y - new.center_y).abs() < POSITION_THRESHOLD
    })
}
