"""
Computer Vision Models for Dating App
Implements ResNet, EfficientNet, and Vision Transformer for image analysis
"""

import torch
import torch.nn as nn
import torch.nn.functional as F
import torchvision.transforms as transforms
from PIL import Image
import numpy as np
import cv2
from typing import Dict, List, Tuple, Optional, Union
from dataclasses import dataclass
import logging
from datetime import datetime
import hashlib
import io
import base64

# Configure logging
logger = logging.getLogger(__name__)

# ============================================
# Data Classes for Image Analysis
# ============================================

@dataclass
class ImageAnalysisResult:
    """Results from image analysis models"""
    attractiveness_score: float  # 0-1 score
    authenticity_score: float  # 0-1 score (real vs fake/filtered)
    quality_score: float  # 0-1 score (image quality)
    inappropriate_content: bool  # Content moderation flag
    face_detected: bool  # Whether a face was detected
    smile_intensity: float  # 0-1 score
    style_embedding: np.ndarray  # Style feature vector for matching
    confidence_scores: Dict[str, float]  # Confidence for each prediction

# ============================================
# ResNet Block Components
# ============================================

class ResNetBlock(nn.Module):
    """Basic ResNet block"""
    def __init__(self, in_channels, out_channels, stride=1, downsample=None):
        super().__init__()
        self.conv1 = nn.Conv2d(in_channels, out_channels, kernel_size=3, 
                               stride=stride, padding=1, bias=False)
        self.bn1 = nn.BatchNorm2d(out_channels)
        self.conv2 = nn.Conv2d(out_channels, out_channels, kernel_size=3, 
                               padding=1, bias=False)
        self.bn2 = nn.BatchNorm2d(out_channels)
        self.downsample = downsample
        self.relu = nn.ReLU(inplace=True)
    
    def forward(self, x):
        identity = x
        out = self.relu(self.bn1(self.conv1(x)))
        out = self.bn2(self.conv2(out))
        
        if self.downsample is not None:
            identity = self.downsample(x)
        
        out += identity
        return self.relu(out)

# ============================================
# 1. ResNet Implementation for Dating App
# ============================================

class DatingAppResNet(nn.Module):
    """ResNet tailored for dating app image analysis"""
    
    def __init__(self, num_blocks=[2, 2, 2, 2], num_classes=7):
        super().__init__()
        self.in_channels = 64
        self.feature_dim = 512
        
        # Initial convolution
        self.conv1 = nn.Conv2d(3, 64, kernel_size=7, stride=2, padding=3, bias=False)
        self.bn1 = nn.BatchNorm2d(64)
        self.relu = nn.ReLU(inplace=True)
        self.maxpool = nn.MaxPool2d(kernel_size=3, stride=2, padding=1)
        
        # ResNet layers
        self.layer1 = self._make_layer(64, num_blocks[0], stride=1)
        self.layer2 = self._make_layer(128, num_blocks[1], stride=2)
        self.layer3 = self._make_layer(256, num_blocks[2], stride=2)
        self.layer4 = self._make_layer(512, num_blocks[3], stride=2)
        
        # Global average pooling
        self.avgpool = nn.AdaptiveAvgPool2d((1, 1))
        
        # Task-specific heads for dating app
        self.attractiveness_head = nn.Linear(512, 1)
        self.authenticity_head = nn.Linear(512, 2)
        self.quality_head = nn.Linear(512, 5)
        self.inappropriate_head = nn.Linear(512, 2)
        self.smile_head = nn.Linear(512, 1)
        
        # Style embedding for matching
        self.style_projector = nn.Sequential(
            nn.Linear(512, 256),
            nn.ReLU(),
            nn.Linear(256, 128)
        )
    
    def _make_layer(self, out_channels, num_blocks, stride):
        downsample = None
        if stride != 1 or self.in_channels != out_channels:
            downsample = nn.Sequential(
                nn.Conv2d(self.in_channels, out_channels, 1, stride, bias=False),
                nn.BatchNorm2d(out_channels)
            )
        
        layers = []
        layers.append(ResNetBlock(self.in_channels, out_channels, stride, downsample))
        self.in_channels = out_channels
        
        for _ in range(1, num_blocks):
            layers.append(ResNetBlock(out_channels, out_channels))
        
        return nn.Sequential(*layers)
    
    def forward(self, x):
        # Feature extraction
        x = self.relu(self.bn1(self.conv1(x)))
        x = self.maxpool(x)
        x = self.layer1(x)
        x = self.layer2(x)
        x = self.layer3(x)
        x = self.layer4(x)
        x = self.avgpool(x)
        features = torch.flatten(x, 1)
        
        # Multi-task predictions
        outputs = {
            'features': features,
            'attractiveness': torch.sigmoid(self.attractiveness_head(features)),
            'authenticity': F.softmax(self.authenticity_head(features), dim=1),
            'quality': F.softmax(self.quality_head(features), dim=1),
            'inappropriate': F.softmax(self.inappropriate_head(features), dim=1),
            'smile': torch.sigmoid(self.smile_head(features)),
            'style_embedding': self.style_projector(features)
        }
        
        return outputs

# ============================================
# 2. EfficientNet Implementation for Dating App
# ============================================

class MBConvBlock(nn.Module):
    """Mobile Inverted Bottleneck Conv block for EfficientNet"""
    
    def __init__(self, in_channels, out_channels, expand_ratio, stride, kernel_size):
        super().__init__()
        self.stride = stride
        self.use_residual = stride == 1 and in_channels == out_channels
        
        hidden_dim = in_channels * expand_ratio
        
        layers = []
        # Expansion phase
        if expand_ratio != 1:
            layers.append(nn.Conv2d(in_channels, hidden_dim, 1, bias=False))
            layers.append(nn.BatchNorm2d(hidden_dim))
            layers.append(nn.SiLU(inplace=True))
        
        # Depthwise convolution
        layers.extend([
            nn.Conv2d(hidden_dim, hidden_dim, kernel_size, stride, 
                     kernel_size//2, groups=hidden_dim, bias=False),
            nn.BatchNorm2d(hidden_dim),
            nn.SiLU(inplace=True)
        ])
        
        # Squeeze and excitation
        self.se = nn.Sequential(
            nn.AdaptiveAvgPool2d(1),
            nn.Conv2d(hidden_dim, hidden_dim // 4, 1),
            nn.SiLU(inplace=True),
            nn.Conv2d(hidden_dim // 4, hidden_dim, 1),
            nn.Sigmoid()
        )
        
        # Output phase
        layers.extend([
            nn.Conv2d(hidden_dim, out_channels, 1, bias=False),
            nn.BatchNorm2d(out_channels)
        ])
        
        self.conv = nn.Sequential(*layers)
    
    def forward(self, x):
        out = self.conv[:3](x) if len(self.conv) > 3 else x
        out = out * self.se(out)
        out = self.conv[3:](out) if len(self.conv) > 3 else self.conv(out)
        
        if self.use_residual:
            out = out + x
        return out

class DatingAppEfficientNet(nn.Module):
    """EfficientNet optimized for dating app use cases"""
    
    def __init__(self, width_mult=1.0, depth_mult=1.0):
        super().__init__()
        self.feature_dim = 1280
        
        # Stem
        self.stem = nn.Sequential(
            nn.Conv2d(3, 32, 3, 2, 1, bias=False),
            nn.BatchNorm2d(32),
            nn.SiLU(inplace=True)
        )
        
        # Building blocks
        self.blocks = nn.ModuleList([
            MBConvBlock(32, 16, 1, 1, 3),
            MBConvBlock(16, 24, 6, 2, 3),
            MBConvBlock(24, 40, 6, 2, 5),
            MBConvBlock(40, 80, 6, 2, 3),
            MBConvBlock(80, 112, 6, 1, 5),
            MBConvBlock(112, 192, 6, 2, 5),
            MBConvBlock(192, 320, 6, 1, 3)
        ])
        
        # Head
        self.head = nn.Sequential(
            nn.Conv2d(320, 1280, 1, bias=False),
            nn.BatchNorm2d(1280),
            nn.SiLU(inplace=True),
            nn.AdaptiveAvgPool2d(1)
        )
        
        # Dating app specific heads
        self.face_quality_head = nn.Sequential(
            nn.Linear(1280, 512),
            nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(512, 3)  # low, medium, high quality
        )
        
        self.age_group_head = nn.Sequential(
            nn.Linear(1280, 256),
            nn.ReLU(),
            nn.Linear(256, 5)  # age groups: 18-24, 25-30, 31-40, 41-50, 50+
        )
        
        self.photo_type_head = nn.Sequential(
            nn.Linear(1280, 256),
            nn.ReLU(),
            nn.Linear(256, 6)  # selfie, group, outdoor, gym, formal, casual
        )
        
        self.compatibility_encoder = nn.Sequential(
            nn.Linear(1280, 512),
            nn.ReLU(),
            nn.Linear(512, 256),
            nn.ReLU(),
            nn.Linear(256, 128)
        )
    
    def forward(self, x):
        x = self.stem(x)
        for block in self.blocks:
            x = block(x)
        x = self.head(x)
        features = torch.flatten(x, 1)
        
        outputs = {
            'features': features,
            'face_quality': F.softmax(self.face_quality_head(features), dim=1),
            'age_group': F.softmax(self.age_group_head(features), dim=1),
            'photo_type': F.softmax(self.photo_type_head(features), dim=1),
            'compatibility_features': self.compatibility_encoder(features)
        }
        
        return outputs

# ============================================
# 3. Vision Transformer Implementation for Dating App
# ============================================

class PatchEmbedding(nn.Module):
    """Convert image to patches for ViT"""
    
    def __init__(self, img_size=224, patch_size=16, in_channels=3, embed_dim=768):
        super().__init__()
        self.img_size = img_size
        self.patch_size = patch_size
        self.num_patches = (img_size // patch_size) ** 2
        
        self.projection = nn.Conv2d(in_channels, embed_dim, 
                                   kernel_size=patch_size, stride=patch_size)
    
    def forward(self, x):
        x = self.projection(x)  # (B, embed_dim, H', W')
        x = x.flatten(2)  # (B, embed_dim, num_patches)
        x = x.transpose(1, 2)  # (B, num_patches, embed_dim)
        return x

class TransformerBlock(nn.Module):
    """Transformer block for ViT"""
    
    def __init__(self, embed_dim, num_heads, mlp_ratio=4.0, dropout=0.1):
        super().__init__()
        self.norm1 = nn.LayerNorm(embed_dim)
        self.attn = nn.MultiheadAttention(embed_dim, num_heads, dropout=dropout)
        self.norm2 = nn.LayerNorm(embed_dim)
        
        mlp_hidden_dim = int(embed_dim * mlp_ratio)
        self.mlp = nn.Sequential(
            nn.Linear(embed_dim, mlp_hidden_dim),
            nn.GELU(),
            nn.Dropout(dropout),
            nn.Linear(mlp_hidden_dim, embed_dim),
            nn.Dropout(dropout)
        )
    
    def forward(self, x):
        # Self-attention
        x_norm = self.norm1(x)
        attn_out, _ = self.attn(x_norm, x_norm, x_norm)
        x = x + attn_out
        
        # MLP
        x = x + self.mlp(self.norm2(x))
        return x

class DatingAppVisionTransformer(nn.Module):
    """Vision Transformer for advanced dating app image understanding"""
    
    def __init__(self, img_size=224, patch_size=16, embed_dim=768, 
                 depth=12, num_heads=12, mlp_ratio=4.0):
        super().__init__()
        self.embed_dim = embed_dim
        self.feature_dim = embed_dim
        
        # Patch embedding
        self.patch_embed = PatchEmbedding(img_size, patch_size, 3, embed_dim)
        num_patches = self.patch_embed.num_patches
        
        # Position embedding
        self.cls_token = nn.Parameter(torch.zeros(1, 1, embed_dim))
        self.pos_embed = nn.Parameter(torch.zeros(1, num_patches + 1, embed_dim))
        
        # Transformer blocks
        self.blocks = nn.ModuleList([
            TransformerBlock(embed_dim, num_heads, mlp_ratio)
            for _ in range(depth)
        ])
        
        self.norm = nn.LayerNorm(embed_dim)
        
        # Dating app specific heads
        self.personality_head = nn.Sequential(
            nn.Linear(embed_dim, 512),
            nn.ReLU(),
            nn.Dropout(0.3),
            nn.Linear(512, 5)  # Big 5 personality traits
        )
        
        self.lifestyle_head = nn.Sequential(
            nn.Linear(embed_dim, 512),
            nn.ReLU(),
            nn.Linear(512, 8)  # lifestyle categories
        )
        
        self.verification_head = nn.Sequential(
            nn.Linear(embed_dim, 256),
            nn.ReLU(),
            nn.Linear(256, 2)  # verified/unverified
        )
        
        self.aesthetic_encoder = nn.Sequential(
            nn.Linear(embed_dim, 512),
            nn.ReLU(),
            nn.Linear(512, 256),
            nn.ReLU(),
            nn.Linear(256, 128)
        )
        
        # Initialize weights
        nn.init.trunc_normal_(self.pos_embed, std=0.02)
        nn.init.trunc_normal_(self.cls_token, std=0.02)
    
    def forward(self, x):
        B = x.shape[0]
        
        # Patch embedding
        x = self.patch_embed(x)
        
        # Add CLS token
        cls_tokens = self.cls_token.expand(B, -1, -1)
        x = torch.cat((cls_tokens, x), dim=1)
        
        # Add position embedding
        x = x + self.pos_embed
        
        # Transformer blocks
        for block in self.blocks:
            x = block(x)
        
        x = self.norm(x)
        
        # Use CLS token for classification
        cls_output = x[:, 0]
        
        outputs = {
            'features': cls_output,
            'personality_traits': F.softmax(self.personality_head(cls_output), dim=1),
            'lifestyle': F.softmax(self.lifestyle_head(cls_output), dim=1),
            'verification_score': F.softmax(self.verification_head(cls_output), dim=1),
            'aesthetic_features': self.aesthetic_encoder(cls_output)
        }
        
        return outputs

# ============================================
# Integrated Vision Analysis System
# ============================================

class DatingAppVisionAnalyzer:
    """Main class that integrates all vision models for comprehensive image analysis"""
    
    def __init__(self, device='cuda' if torch.cuda.is_available() else 'cpu'):
        self.device = torch.device(device)
        
        # Initialize models
        self.resnet = DatingAppResNet().to(self.device)
        self.efficientnet = DatingAppEfficientNet().to(self.device)
        self.vit = DatingAppVisionTransformer().to(self.device)
        
        # Set to evaluation mode
        self.resnet.eval()
        self.efficientnet.eval()
        self.vit.eval()
        
        # Image preprocessing
        self.transform = transforms.Compose([
            transforms.Resize((224, 224)),
            transforms.ToTensor(),
            transforms.Normalize(mean=[0.485, 0.456, 0.406],
                               std=[0.229, 0.224, 0.225])
        ])
        
        # Cache for processed images
        self.cache = {}
        self.cache_size = 1000
    
    def _get_cache_key(self, image_data: Union[str, bytes]) -> str:
        """Generate cache key for image"""
        if isinstance(image_data, str):
            return hashlib.md5(image_data.encode()).hexdigest()
        return hashlib.md5(image_data).hexdigest()
    
    def preprocess_image(self, image_data: Union[str, bytes, Image.Image]) -> torch.Tensor:
        """Preprocess image for model input"""
        if isinstance(image_data, str):
            # Base64 encoded image
            image_bytes = base64.b64decode(image_data)
            image = Image.open(io.BytesIO(image_bytes))
        elif isinstance(image_data, bytes):
            # Raw bytes
            image = Image.open(io.BytesIO(image_data))
        else:
            # PIL Image
            image = image_data
        
        # Convert to RGB if necessary
        if image.mode != 'RGB':
            image = image.convert('RGB')
        
        # Apply transformations
        tensor = self.transform(image).unsqueeze(0).to(self.device)
        return tensor
    
    @torch.no_grad()
    def analyze_image(self, image_data: Union[str, bytes, Image.Image], 
                     use_cache: bool = True) -> ImageAnalysisResult:
        """Comprehensive image analysis using all models"""
        
        # Check cache
        cache_key = self._get_cache_key(str(image_data)[:100]) if use_cache else None
        if cache_key and cache_key in self.cache:
            return self.cache[cache_key]
        
        # Preprocess image
        image_tensor = self.preprocess_image(image_data)
        
        # Get predictions from all models
        resnet_output = self.resnet(image_tensor)
        efficientnet_output = self.efficientnet(image_tensor)
        vit_output = self.vit(image_tensor)
        
        # Combine predictions using weighted ensemble
        attractiveness_score = float(
            0.4 * resnet_output['attractiveness'][0, 0] +
            0.3 * efficientnet_output['face_quality'][0, 2] +  # High quality score
            0.3 * vit_output['verification_score'][0, 0]  # Verified score
        )
        
        authenticity_score = float(
            0.5 * resnet_output['authenticity'][0, 0] +  # Real class
            0.5 * vit_output['verification_score'][0, 0]
        )
        
        quality_score = float(
            torch.argmax(efficientnet_output['face_quality'][0]) / 2.0  # Normalize to 0-1
        )
        
        inappropriate_content = bool(
            resnet_output['inappropriate'][0, 1] > 0.7  # High confidence for inappropriate
        )
        
        smile_intensity = float(resnet_output['smile'][0, 0])
        
        # Combine style embeddings from all models
        style_embedding = np.concatenate([
            resnet_output['style_embedding'][0].cpu().numpy(),
            efficientnet_output['compatibility_features'][0].cpu().numpy(),
            vit_output['aesthetic_features'][0].cpu().numpy()
        ])
        
        # Calculate confidence scores
        confidence_scores = {
            'attractiveness': float(torch.std(torch.tensor([
                resnet_output['attractiveness'][0, 0],
                efficientnet_output['face_quality'][0, 2],
                vit_output['verification_score'][0, 0]
            ]))),
            'overall': 0.85  # Placeholder - could be calculated from model uncertainties
        }
        
        result = ImageAnalysisResult(
            attractiveness_score=min(max(attractiveness_score, 0), 1),
            authenticity_score=min(max(authenticity_score, 0), 1),
            quality_score=min(max(quality_score, 0), 1),
            inappropriate_content=inappropriate_content,
            face_detected=quality_score > 0.3,  # Simple heuristic
            smile_intensity=min(max(smile_intensity, 0), 1),
            style_embedding=style_embedding,
            confidence_scores=confidence_scores
        )
        
        # Update cache
        if cache_key and len(self.cache) < self.cache_size:
            self.cache[cache_key] = result
        
        return result
    
    def calculate_visual_compatibility(self, user1_images: List, user2_images: List) -> float:
        """Calculate visual compatibility between two users based on their photos"""
        user1_embeddings = []
        user2_embeddings = []
        
        # Extract embeddings for all images
        for img in user1_images[:5]:  # Limit to 5 images
            result = self.analyze_image(img)
            user1_embeddings.append(result.style_embedding)
        
        for img in user2_images[:5]:
            result = self.analyze_image(img)
            user2_embeddings.append(result.style_embedding)
        
        if not user1_embeddings or not user2_embeddings:
            return 0.5  # Default compatibility
        
        # Calculate average embeddings
        user1_avg = np.mean(user1_embeddings, axis=0)
        user2_avg = np.mean(user2_embeddings, axis=0)
        
        # Calculate cosine similarity
        similarity = np.dot(user1_avg, user2_avg) / (
            np.linalg.norm(user1_avg) * np.linalg.norm(user2_avg)
        )
        
        # Normalize to 0-1 range
        compatibility = (similarity + 1) / 2
        
        return float(compatibility)
    
    def get_photo_insights(self, image_data: Union[str, bytes, Image.Image]) -> Dict:
        """Get detailed insights about a photo"""
        analysis = self.analyze_image(image_data)
        
        # Process with individual models for detailed insights
        image_tensor = self.preprocess_image(image_data)
        
        with torch.no_grad():
            efficientnet_output = self.efficientnet(image_tensor)
            vit_output = self.vit(image_tensor)
        
        photo_type_idx = torch.argmax(efficientnet_output['photo_type'][0])
        photo_types = ['selfie', 'group', 'outdoor', 'gym', 'formal', 'casual']
        
        age_group_idx = torch.argmax(efficientnet_output['age_group'][0])
        age_groups = ['18-24', '25-30', '31-40', '41-50', '50+']
        
        personality_scores = vit_output['personality_traits'][0].cpu().numpy()
        personality_traits = ['openness', 'conscientiousness', 'extraversion', 
                            'agreeableness', 'neuroticism']
        
        insights = {
            'overall_score': (analysis.attractiveness_score + analysis.quality_score) / 2,
            'photo_type': photo_types[photo_type_idx],
            'estimated_age_group': age_groups[age_group_idx],
            'quality_rating': ['low', 'medium', 'high'][int(analysis.quality_score * 2.99)],
            'authenticity': 'authentic' if analysis.authenticity_score > 0.7 else 'possibly edited',
            'smile_detected': analysis.smile_intensity > 0.5,
            'personality_hints': {
                trait: float(score) 
                for trait, score in zip(personality_traits, personality_scores)
            },
            'recommendations': self._generate_photo_recommendations(analysis)
        }
        
        return insights
    
    def _generate_photo_recommendations(self, analysis: ImageAnalysisResult) -> List[str]:
        """Generate recommendations for improving photos"""
        recommendations = []
        
        if analysis.quality_score < 0.5:
            recommendations.append("Consider using better lighting or a higher quality camera")
        
        if analysis.authenticity_score < 0.6:
            recommendations.append("Natural, unfiltered photos tend to get better matches")
        
        if analysis.smile_intensity < 0.3:
            recommendations.append("Photos with genuine smiles receive 23% more likes")
        
        if not analysis.face_detected:
            recommendations.append("Make sure your face is clearly visible in at least one photo")
        
        return recommendations

# Export main components
__all__ = [
    'ImageAnalysisResult',
    'DatingAppResNet',
    'DatingAppEfficientNet',
    'DatingAppVisionTransformer',
    'DatingAppVisionAnalyzer'
]




