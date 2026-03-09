//! Photo quality scoring: blur detection, low-light detection, face ratio checks.
//! All functions are pure and operate on raw pixel data — no ONNX models needed.

use image::RgbImage;

/// Result of photo quality analysis.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PhotoQualityResult {
    /// Overall composite quality score (0.0-1.0, higher is better)
    pub composite_score: f32,
    /// Blur score (0.0 = very blurry, 1.0 = sharp)
    pub blur_score: f32,
    /// Low-light score (0.0 = very dark, 1.0 = well-lit)
    pub low_light_score: f32,
    /// Face ratio (face area / image area). 0.0 if no face detected.
    pub face_ratio: f32,
    /// Quality flags for user-facing feedback
    pub flags: Vec<String>,
}

/// Compute blur score using Laplacian variance.
/// Higher variance = sharper image. Normalized to 0.0-1.0.
pub fn blur_score(image: &RgbImage) -> f32 {
    let (w, h) = (image.width() as usize, image.height() as usize);
    if w < 3 || h < 3 {
        return 0.0;
    }

    // Convert to grayscale
    let gray: Vec<f32> = image
        .pixels()
        .map(|p| 0.299 * p.0[0] as f32 + 0.587 * p.0[1] as f32 + 0.114 * p.0[2] as f32)
        .collect();

    // Compute Laplacian (3x3 kernel: [0,-1,0; -1,4,-1; 0,-1,0])
    let mut laplacian_sum = 0.0_f64;
    let mut count = 0u64;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let center = gray[y * w + x] as f64;
            let top = gray[(y - 1) * w + x] as f64;
            let bottom = gray[(y + 1) * w + x] as f64;
            let left = gray[y * w + (x - 1)] as f64;
            let right = gray[y * w + (x + 1)] as f64;
            let lap = 4.0 * center - top - bottom - left - right;
            laplacian_sum += lap * lap;
            count += 1;
        }
    }

    if count == 0 {
        return 0.0;
    }

    let variance = laplacian_sum / count as f64;
    // Normalize: variance > 500 is sharp, < 50 is blurry
    let score = ((variance - 20.0) / 480.0).clamp(0.0, 1.0);
    score as f32
}

/// Compute low-light score from luminance histogram.
/// Returns 0.0 for very dark images, 1.0 for well-lit images.
pub fn low_light_score(image: &RgbImage) -> f32 {
    let total = image.width() as f64 * image.height() as f64;
    if total == 0.0 {
        return 0.0;
    }

    let mut lum_sum = 0.0_f64;
    let mut dark_pixels = 0u64;

    for p in image.pixels() {
        // ITU-R BT.601 luminance
        let lum = 0.299 * p.0[0] as f64 + 0.587 * p.0[1] as f64 + 0.114 * p.0[2] as f64;
        lum_sum += lum;
        if lum < 40.0 {
            dark_pixels += 1;
        }
    }

    let mean_lum = lum_sum / total;
    let dark_ratio = dark_pixels as f64 / total;

    // Score based on mean luminance and dark pixel ratio
    let lum_score = (mean_lum / 128.0).clamp(0.0, 1.0);
    let dark_penalty = (1.0 - dark_ratio * 2.0).clamp(0.0, 1.0);

    (lum_score * 0.6 + dark_penalty * 0.4) as f32
}

/// Compute face ratio given a face bounding box.
/// Returns face_area / image_area. Higher ratio means face fills more of the frame.
pub fn face_ratio_score(
    image_width: u32,
    image_height: u32,
    face_bbox: Option<(u32, u32, u32, u32)>,
) -> f32 {
    let image_area = image_width as f64 * image_height as f64;
    if image_area == 0.0 {
        return 0.0;
    }

    match face_bbox {
        Some((_x, _y, w, h)) => {
            let face_area = w as f64 * h as f64;
            let ratio = face_area / image_area;
            // Optimal: face is 10-40% of frame. Too small = far away, too big = too close.
            if ratio < 0.05 {
                ratio as f32 * 4.0 // Penalize tiny faces
            } else if ratio > 0.6 {
                (1.0 - (ratio - 0.6) * 2.0).max(0.3) as f32 // Penalize extreme close-ups
            } else {
                // Sweet spot: normalize 0.05-0.4 to 0.5-1.0
                (0.5 + (ratio - 0.05) / 0.7).min(1.0) as f32
            }
        }
        None => 0.0,
    }
}

/// Compute composite photo quality score from individual signals.
pub fn composite_quality(
    aesthetic: f32,
    blur: f32,
    low_light: f32,
    face_ratio: f32,
) -> PhotoQualityResult {
    let mut flags = Vec::new();

    if blur < 0.3 {
        flags.push("blurry".to_string());
    }
    if low_light < 0.3 {
        flags.push("too_dark".to_string());
    }
    if face_ratio < 0.05 && face_ratio > 0.0 {
        flags.push("face_too_small".to_string());
    }
    if face_ratio == 0.0 {
        flags.push("no_face_detected".to_string());
    }
    if aesthetic < 0.3 {
        flags.push("low_quality".to_string());
    }

    // Weighted composite: aesthetic 40%, blur 25%, low-light 15%, face 20%
    let composite = aesthetic * 0.40 + blur * 0.25 + low_light * 0.15 + face_ratio * 0.20;

    PhotoQualityResult {
        composite_score: composite.clamp(0.0, 1.0),
        blur_score: blur,
        low_light_score: low_light,
        face_ratio,
        flags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blur_score_returns_valid_range() {
        // Create a simple 10x10 white image
        let img = RgbImage::from_fn(10, 10, |_, _| image::Rgb([255, 255, 255]));
        let score = blur_score(&img);
        assert!(score >= 0.0 && score <= 1.0);
    }

    #[test]
    fn test_low_light_dark_image() {
        let img = RgbImage::from_fn(10, 10, |_, _| image::Rgb([10, 10, 10]));
        let score = low_light_score(&img);
        assert!(score < 0.3, "Dark image should have low light score: {}", score);
    }

    #[test]
    fn test_low_light_bright_image() {
        let img = RgbImage::from_fn(10, 10, |_, _| image::Rgb([200, 200, 200]));
        let score = low_light_score(&img);
        assert!(score > 0.7, "Bright image should have high light score: {}", score);
    }

    #[test]
    fn test_face_ratio_no_face() {
        let score = face_ratio_score(100, 100, None);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_face_ratio_good_size() {
        let score = face_ratio_score(100, 100, Some((20, 20, 30, 30)));
        assert!(score > 0.5, "Good face ratio should score high: {}", score);
    }

    #[test]
    fn test_composite_quality_flags() {
        let result = composite_quality(0.2, 0.1, 0.1, 0.0);
        assert!(result.flags.contains(&"blurry".to_string()));
        assert!(result.flags.contains(&"too_dark".to_string()));
        assert!(result.flags.contains(&"low_quality".to_string()));
        assert!(result.flags.contains(&"no_face_detected".to_string()));
    }
}
