//! Media optimization: responsive image variants, format transcoding,
//! smallest rendition serving based on client capabilities.

use image::{DynamicImage, imageops::FilterType};
use serde::{Deserialize, Serialize};

/// Pre-defined image rendition sizes.
#[derive(Debug, Clone, Copy)]
pub struct RenditionSize {
    pub name: &'static str,
    pub max_dimension: u32,
}

/// Standard rendition sizes for profile photos.
pub const RENDITIONS: &[RenditionSize] = &[
    RenditionSize { name: "thumbnail", max_dimension: 150 },
    RenditionSize { name: "card", max_dimension: 400 },
    RenditionSize { name: "full", max_dimension: 1080 },
];

/// Set of generated renditions for a single photo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenditionSet {
    pub original_key: String,
    pub renditions: Vec<Rendition>,
}

/// A single rendition variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rendition {
    pub name: String,
    pub format: String,     // "jpeg" or "webp"
    pub width: u32,
    pub height: u32,
    pub size_bytes: usize,
    pub key: String,        // S3 key or local path
}

/// Client capabilities extracted from request headers.
#[derive(Debug, Clone)]
pub struct ClientCapabilities {
    pub accepts_webp: bool,
    pub accepts_avif: bool,
    pub device_pixel_ratio: f32,
    pub viewport_width: Option<u32>,
    pub network_class: Option<String>,
}

impl ClientCapabilities {
    /// Extract from HTTP headers.
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let accept = headers.get("Accept")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let dpr = headers.get("DPR")
            .or_else(|| headers.get("X-DPR"))
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0);

        let viewport = headers.get("Viewport-Width")
            .or_else(|| headers.get("X-Viewport-Width"))
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok());

        let network = headers.get("X-Network-Class")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        Self {
            accepts_webp: accept.contains("image/webp"),
            accepts_avif: accept.contains("image/avif"),
            device_pixel_ratio: dpr,
            viewport_width: viewport,
            network_class: network,
        }
    }

    /// Select the best rendition for this client.
    pub fn select_rendition<'a>(&self, renditions: &'a [Rendition]) -> Option<&'a Rendition> {
        let target_width = self.viewport_width
            .map(|vw| (vw as f32 * self.device_pixel_ratio) as u32)
            .unwrap_or(400);

        // Adjust for network class
        let adjusted_width = match self.network_class.as_deref() {
            Some("2g") | Some("3g") => target_width.min(150), // Thumbnail only
            Some("4g") => target_width.min(400),              // Card size
            _ => target_width,                                 // Full quality
        };

        // Prefer WebP if supported
        let preferred_format = if self.accepts_webp { "webp" } else { "jpeg" };

        // Find the smallest rendition that's >= adjusted_width in the preferred format
        let mut best: Option<&Rendition> = None;
        for r in renditions {
            if r.format != preferred_format {
                continue;
            }
            if r.width >= adjusted_width {
                match best {
                    Some(current) if r.width < current.width => best = Some(r),
                    None => best = Some(r),
                    _ => {}
                }
            }
        }

        // Fallback: any format, any size
        best.or_else(|| {
            renditions.iter()
                .filter(|r| r.width >= adjusted_width)
                .min_by_key(|r| r.width)
                .or_else(|| renditions.iter().max_by_key(|r| r.width))
        })
    }
}

/// Media optimizer service.
pub struct MediaOptimizer;

impl MediaOptimizer {
    /// Generate all rendition variants for a photo.
    /// Returns raw bytes for each rendition (caller handles upload to S3).
    pub fn generate_renditions(
        image: &DynamicImage,
        base_key: &str,
    ) -> Vec<RenditionOutput> {
        let mut outputs = Vec::new();

        for size in RENDITIONS {
            // Resize maintaining aspect ratio
            let resized = image.resize(
                size.max_dimension,
                size.max_dimension,
                FilterType::Lanczos3,
            );

            // JPEG rendition
            let jpeg_bytes = encode_jpeg(&resized, 85);
            let jpeg_key = format!("{}_{}.jpg", base_key, size.name);
            outputs.push(RenditionOutput {
                rendition: Rendition {
                    name: size.name.to_string(),
                    format: "jpeg".to_string(),
                    width: resized.width(),
                    height: resized.height(),
                    size_bytes: jpeg_bytes.len(),
                    key: jpeg_key,
                },
                bytes: jpeg_bytes,
            });

            // WebP rendition (typically 25-30% smaller)
            let webp_bytes = encode_webp(&resized, 80);
            let webp_key = format!("{}_{}.webp", base_key, size.name);
            outputs.push(RenditionOutput {
                rendition: Rendition {
                    name: size.name.to_string(),
                    format: "webp".to_string(),
                    width: resized.width(),
                    height: resized.height(),
                    size_bytes: webp_bytes.len(),
                    key: webp_key,
                },
                bytes: webp_bytes,
            });
        }

        outputs
    }

    /// Compute bandwidth savings from serving optimal rendition.
    pub fn bandwidth_savings(original_bytes: usize, rendition_bytes: usize) -> f64 {
        if original_bytes == 0 {
            return 0.0;
        }
        1.0 - (rendition_bytes as f64 / original_bytes as f64)
    }
}

/// Output of rendition generation (rendition metadata + bytes).
pub struct RenditionOutput {
    pub rendition: Rendition,
    pub bytes: Vec<u8>,
}

/// Encode a DynamicImage as JPEG.
fn encode_jpeg(image: &DynamicImage, quality: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
    if let Err(e) = image.write_with_encoder(encoder) {
        tracing::warn!("JPEG encoding failed: {e}");
        return Vec::new();
    }
    buf
}

/// Encode a DynamicImage as WebP.
fn encode_webp(image: &DynamicImage, quality: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut cursor);
    // Use lossless encoder as a fallback-safe approach
    let _ = quality; // quality param reserved for future lossy support
    if let Err(e) = image.write_with_encoder(encoder) {
        tracing::warn!("WebP encoding failed: {e}");
        // Fallback to JPEG if WebP fails
        return encode_jpeg(image, quality);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    #[test]
    fn test_generate_renditions() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 200, |_, _| {
            image::Rgb([128, 128, 128])
        }));
        let outputs = MediaOptimizer::generate_renditions(&img, "test/photo_1");
        // 3 sizes * 2 formats = 6 renditions
        assert_eq!(outputs.len(), 6);
        // All should have non-empty bytes
        for o in &outputs {
            assert!(!o.bytes.is_empty(), "Rendition {} {} is empty", o.rendition.name, o.rendition.format);
        }
    }

    #[test]
    fn test_client_capabilities_webp() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("Accept", "image/webp,image/png,image/jpeg".parse().unwrap());
        let caps = ClientCapabilities::from_headers(&headers);
        assert!(caps.accepts_webp);
        assert!(!caps.accepts_avif);
    }

    #[test]
    fn test_select_rendition_2g() {
        let renditions = vec![
            Rendition { name: "thumbnail".into(), format: "jpeg".into(), width: 150, height: 150, size_bytes: 5000, key: "t.jpg".into() },
            Rendition { name: "card".into(), format: "jpeg".into(), width: 400, height: 400, size_bytes: 20000, key: "c.jpg".into() },
            Rendition { name: "full".into(), format: "jpeg".into(), width: 1080, height: 1080, size_bytes: 100000, key: "f.jpg".into() },
        ];

        let caps = ClientCapabilities {
            accepts_webp: false,
            accepts_avif: false,
            device_pixel_ratio: 1.0,
            viewport_width: Some(400),
            network_class: Some("2g".to_string()),
        };

        let selected = caps.select_rendition(&renditions).unwrap();
        assert_eq!(selected.name, "thumbnail", "2G should get thumbnail");
    }

    #[test]
    fn test_bandwidth_savings() {
        let savings = MediaOptimizer::bandwidth_savings(100000, 25000);
        assert!((savings - 0.75).abs() < 0.01);
    }
}
