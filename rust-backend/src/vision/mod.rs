use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::{imageops::FilterType, DynamicImage, ImageError, RgbImage};
use serde::Serialize;
use tract_onnx::prelude::*;
use tract_onnx::prelude::tract_ndarray::Array4;
use tract_onnx::prelude::{Tensor, TVec, TValue, TypedModel};

use crate::config::Config;

const NSFW_SIZE: u32 = 224;
const NIMA_SIZE: u32 = 224;
const FER_SIZE: u32 = 64;
const ARCFACE_SIZE: u32 = 112;
const LIVENESS_SIZE: u32 = 80;

#[derive(Debug)]
pub struct VisionError {
    message: String,
}

impl VisionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for VisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for VisionError {}

impl From<TractError> for VisionError {
    fn from(err: TractError) -> Self {
        Self::new(format!("Model error: {err}"))
    }
}

impl From<ImageError> for VisionError {
    fn from(err: ImageError) -> Self {
        Self::new(format!("Image decode error: {err}"))
    }
}

pub struct VisionAnalyzer {
    nsfw: OnnxModel,
    fer: OnnxModel,
    nima: OnnxModel,
    arcface: OnnxModel,
    liveness: OnnxModel,
}

#[derive(Serialize)]
pub struct VisionAnalysis {
    pub attractiveness_score: f32,
    pub authenticity_score: f32,
    pub quality_score: f32,
    pub inappropriate_content: bool,
    pub face_detected: bool,
    pub smile_intensity: f32,
    pub style_embedding: Vec<f32>,
    pub confidence_scores: HashMap<String, f32>,
}

impl VisionAnalyzer {
    pub fn load(config: &Config) -> Result<Self, VisionError> {
        let base = PathBuf::from(&config.vision_model_dir);
        let nsfw_path = model_path(&base, &config.vision_nsfw_model);
        let fer_path = model_path(&base, &config.vision_fer_model);
        let nima_path = model_path(&base, &config.vision_nima_model);
        let arcface_path = model_path(&base, &config.vision_arcface_model);
        let liveness_path = model_path(&base, &config.vision_liveness_model);

        Ok(Self {
            nsfw: OnnxModel::new(&nsfw_path, [1, 3, NSFW_SIZE as usize, NSFW_SIZE as usize])?,
            fer: OnnxModel::new(&fer_path, [1, 1, FER_SIZE as usize, FER_SIZE as usize])?,
            nima: OnnxModel::new(&nima_path, [1, 3, NIMA_SIZE as usize, NIMA_SIZE as usize])?,
            arcface: OnnxModel::new(&arcface_path, [1, 3, ARCFACE_SIZE as usize, ARCFACE_SIZE as usize])?,
            liveness: OnnxModel::new(&liveness_path, [1, 3, LIVENESS_SIZE as usize, LIVENESS_SIZE as usize])?,
        })
    }

    #[allow(dead_code)]
    pub fn analyze_bytes(&self, bytes: &[u8]) -> Result<VisionAnalysis, VisionError> {
        let image = image::load_from_memory(bytes)?;
        self.analyze_image(&image)
    }

    pub fn analyze_image(&self, image: &DynamicImage) -> Result<VisionAnalysis, VisionError> {
        let rgb = image.to_rgb8();
        let center = center_crop(&rgb);

        let nsfw_score = self.run_nsfw(&rgb)?;
        let inappropriate_content = nsfw_score >= 0.7;

        let aesthetic_score = self.run_nima(&rgb)?;
        let quality_score = aesthetic_score;
        let attractiveness_score = aesthetic_score;

        let smile_intensity = self.run_fer(&center)?;
        let face_detected = smile_intensity >= 0.2;

        let authenticity_score = self.run_liveness(&center)?;
        let style_embedding = self.run_arcface(&center)?;

        let mut confidence_scores = HashMap::new();
        confidence_scores.insert("nsfw".to_string(), nsfw_score);
        confidence_scores.insert("aesthetic".to_string(), aesthetic_score);
        confidence_scores.insert("smile".to_string(), smile_intensity);
        confidence_scores.insert("authenticity".to_string(), authenticity_score);

        Ok(VisionAnalysis {
            attractiveness_score,
            authenticity_score,
            quality_score,
            inappropriate_content,
            face_detected,
            smile_intensity,
            style_embedding,
            confidence_scores,
        })
    }

    fn run_nsfw(&self, image: &RgbImage) -> Result<f32, VisionError> {
        let resized = resize_rgb(image, NSFW_SIZE, NSFW_SIZE);
        let input = chw_bgr_mean_sub(&resized, [104.0, 117.0, 123.0]);
        let output = self.nsfw.run(input)?;
        let scores = flatten_output(&output, 2);
        let nsfw_score = scores.get(1).copied().unwrap_or(0.0);
        Ok(nsfw_score.clamp(0.0, 1.0))
    }

    fn run_nima(&self, image: &RgbImage) -> Result<f32, VisionError> {
        let resized = resize_rgb(image, NIMA_SIZE, NIMA_SIZE);
        let input = chw_mean_std(&resized, [0.485, 0.456, 0.406], [0.229, 0.224, 0.225]);
        let output = self.nima.run(input)?;
        let scores = flatten_output(&output, 10);
        let mut mos = 0.0;
        for (idx, prob) in scores.iter().enumerate() {
            mos += (idx as f32 + 1.0) * prob;
        }
        let normalized = ((mos - 1.0) / 9.0).clamp(0.0, 1.0);
        Ok(normalized)
    }

    fn run_fer(&self, image: &RgbImage) -> Result<f32, VisionError> {
        let resized = resize_rgb(image, FER_SIZE, FER_SIZE);
        let input = chw_grayscale(&resized);
        let output = self.fer.run(input)?;
        let scores = flatten_output(&output, 8);
        let happiness = scores.get(1).copied().unwrap_or(0.0);
        Ok(happiness.clamp(0.0, 1.0))
    }

    fn run_arcface(&self, image: &RgbImage) -> Result<Vec<f32>, VisionError> {
        let resized = resize_rgb(image, ARCFACE_SIZE, ARCFACE_SIZE);
        let input = chw_normalized(&resized);
        let output = self.arcface.run(input)?;
        let embedding = flatten_all(&output);
        Ok(l2_normalize(embedding))
    }

    fn run_liveness(&self, image: &RgbImage) -> Result<f32, VisionError> {
        let resized = resize_rgb(image, LIVENESS_SIZE, LIVENESS_SIZE);
        let input = chw_normalized(&resized);
        let output = self.liveness.run(input)?;
        let scores = flatten_output(&output, 2);
        let real_score = scores.get(1).copied().unwrap_or(0.0);
        Ok(real_score.clamp(0.0, 1.0))
    }
}

type OnnxRunnable = SimplePlan<TypedFact, Box<dyn TypedOp>, TypedModel>;

struct OnnxModel {
    model: OnnxRunnable,
    input_shape: [usize; 4],
}

impl OnnxModel {
    fn new(path: &Path, input_shape: [usize; 4]) -> Result<Self, VisionError> {
        if !path.exists() {
            return Err(VisionError::new(format!(
                "Model file not found: {}",
                path.display()
            )));
        }

        let model = tract_onnx::onnx()
            .model_for_path(path)?
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(
                    input_shape[0],
                    input_shape[1],
                    input_shape[2],
                    input_shape[3]
                )),
            )?
            .into_optimized()?
            .into_runnable()?;

        Ok(Self { model, input_shape })
    }

    fn run(&self, input: Vec<f32>) -> Result<TVec<TValue>, VisionError> {
        let expected = self.input_shape.iter().product::<usize>();
        if input.len() != expected {
            return Err(VisionError::new(format!(
                "Input has {} values, expected {}",
                input.len(),
                expected
            )));
        }
        let array = Array4::from_shape_vec(
            (
                self.input_shape[0],
                self.input_shape[1],
                self.input_shape[2],
                self.input_shape[3],
            ),
            input,
        )
        .map_err(|err| VisionError::new(format!("Input reshape failed: {err}")))?;
        let tensor: Tensor = array.into();
        let outputs = self.model.run(tvec!(tensor.into()))?;
        Ok(outputs)
    }
}

fn model_path(base: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn center_crop(image: &RgbImage) -> RgbImage {
    let width = image.width();
    let height = image.height();
    let size = width.min(height);
    let left = (width - size) / 2;
    let top = (height - size) / 2;
    image::imageops::crop_imm(image, left, top, size, size).to_image()
}

fn resize_rgb(image: &RgbImage, width: u32, height: u32) -> RgbImage {
    image::imageops::resize(image, width, height, FilterType::CatmullRom)
}

fn chw_mean_std(image: &RgbImage, mean: [f32; 3], std: [f32; 3]) -> Vec<f32> {
    let mut output = Vec::with_capacity((image.width() * image.height() * 3) as usize);
    for c in 0..3 {
        for pixel in image.pixels() {
            let value = pixel.0[c] as f32 / 255.0;
            output.push((value - mean[c]) / std[c]);
        }
    }
    output
}

fn chw_normalized(image: &RgbImage) -> Vec<f32> {
    let mut output = Vec::with_capacity((image.width() * image.height() * 3) as usize);
    for c in 0..3 {
        for pixel in image.pixels() {
            let value = pixel.0[c] as f32 / 255.0;
            output.push((value - 0.5) / 0.5);
        }
    }
    output
}

fn chw_grayscale(image: &RgbImage) -> Vec<f32> {
    let mut output = Vec::with_capacity((image.width() * image.height()) as usize);
    for pixel in image.pixels() {
        let r = pixel.0[0] as f32 / 255.0;
        let g = pixel.0[1] as f32 / 255.0;
        let b = pixel.0[2] as f32 / 255.0;
        let gray = 0.2989 * r + 0.5870 * g + 0.1140 * b;
        output.push(gray);
    }
    output
}

fn chw_bgr_mean_sub(image: &RgbImage, mean: [f32; 3]) -> Vec<f32> {
    let mut output = Vec::with_capacity((image.width() * image.height() * 3) as usize);
    for c in 0..3 {
        let bgr_index = 2 - c;
        for pixel in image.pixels() {
            let value = pixel.0[bgr_index] as f32;
            output.push(value - mean[c]);
        }
    }
    output
}

fn flatten_output(outputs: &[TValue], expected: usize) -> Vec<f32> {
    outputs
        .first()
        .and_then(|tensor| tensor.to_array_view::<f32>().ok())
        .map(|view| view.iter().cloned().collect::<Vec<f32>>())
        .filter(|vals| vals.len() >= expected)
        .unwrap_or_else(|| vec![0.0; expected])
}

fn flatten_all(outputs: &[TValue]) -> Vec<f32> {
    outputs
        .first()
        .and_then(|tensor| tensor.to_array_view::<f32>().ok())
        .map(|view| view.iter().cloned().collect::<Vec<f32>>())
        .unwrap_or_else(|| vec![0.0; 128])
}

fn l2_normalize(mut vec: Vec<f32>) -> Vec<f32> {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vec {
            *v /= norm;
        }
    }
    vec
}
