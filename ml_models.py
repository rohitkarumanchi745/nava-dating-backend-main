

import os
import io
import json
import math
import base64
import pickle
import hashlib
import logging
import threading
from abc import ABC, abstractmethod
from collections import defaultdict, deque
from dataclasses import dataclass
from datetime import datetime, timedelta
from typing import Dict, List, Tuple, Optional, Union

import numpy as np
from PIL import Image

import torch
import torch.nn as nn
import torch.nn.functional as F
import torchvision.transforms as transforms

# ------------------------------------------------------------------------------------
# Logging
# ------------------------------------------------------------------------------------
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

# ------------------------------------------------------------------------------------
# Vision: Data classes
# ------------------------------------------------------------------------------------

@dataclass
class ImageAnalysisResult:
    """Results from image analysis models"""
    attractiveness_score: float  # 0-1
    authenticity_score: float    # 0-1 (real vs fake/filtered)
    quality_score: float         # 0-1 (image quality)
    inappropriate_content: bool  # moderation flag
    face_detected: bool          # face detected heuristic
    smile_intensity: float       # 0-1
    style_embedding: np.ndarray  # combined embedding
    confidence_scores: Dict[str, float]

# ------------------------------------------------------------------------------------
# Vision: ResNet (light) for dating app heads
# ------------------------------------------------------------------------------------

class ResNetBlock(nn.Module):
    def __init__(self, in_channels, out_channels, stride=1, downsample=None):
        super().__init__()
        self.conv1 = nn.Conv2d(in_channels, out_channels, 3, stride=stride, padding=1, bias=False)
        self.bn1 = nn.BatchNorm2d(out_channels)
        self.conv2 = nn.Conv2d(out_channels, out_channels, 3, padding=1, bias=False)
        self.bn2 = nn.BatchNorm2d(out_channels)
        self.downsample = downsample
        self.relu = nn.ReLU(inplace=True)

    def forward(self, x):
        identity = x
        out = self.relu(self.bn1(self.conv1(x)))
        out = self.bn2(self.conv2(out))
        if self.downsample is not None:
            identity = self.downsample(x)
        out = self.relu(out + identity)
        return out

class DatingAppResNet(nn.Module):
    """ResNet18-ish backbone with multi-task heads"""
    def __init__(self, num_blocks=[2,2,2,2]):
        super().__init__()
        self.in_channels = 64

        self.conv1 = nn.Conv2d(3, 64, 7, stride=2, padding=3, bias=False)
        self.bn1 = nn.BatchNorm2d(64)
        self.relu = nn.ReLU(inplace=True)
        self.maxpool = nn.MaxPool2d(3, stride=2, padding=1)

        self.layer1 = self._make_layer(64,  num_blocks[0], stride=1)
        self.layer2 = self._make_layer(128, num_blocks[1], stride=2)
        self.layer3 = self._make_layer(256, num_blocks[2], stride=2)
        self.layer4 = self._make_layer(512, num_blocks[3], stride=2)

        self.avgpool = nn.AdaptiveAvgPool2d(1)

        # Heads
        self.attractiveness_head = nn.Linear(512, 1)
        self.authenticity_head   = nn.Linear(512, 2)
        self.quality_head        = nn.Linear(512, 5)
        self.inappropriate_head  = nn.Linear(512, 2)
        self.smile_head          = nn.Linear(512, 1)

        self.style_projector = nn.Sequential(
            nn.Linear(512, 256), nn.ReLU(), nn.Linear(256, 128)
        )

    def _make_layer(self, out_channels, num_blocks, stride):
        downsample = None
        if stride != 1 or self.in_channels != out_channels:
            downsample = nn.Sequential(
                nn.Conv2d(self.in_channels, out_channels, 1, stride=stride, bias=False),
                nn.BatchNorm2d(out_channels)
            )
        layers = [ResNetBlock(self.in_channels, out_channels, stride, downsample)]
        self.in_channels = out_channels
        for _ in range(1, num_blocks):
            layers.append(ResNetBlock(out_channels, out_channels))
        return nn.Sequential(*layers)

    def forward(self, x):
        x = self.relu(self.bn1(self.conv1(x)))
        x = self.maxpool(x)
        x = self.layer1(x); x = self.layer2(x); x = self.layer3(x); x = self.layer4(x)
        x = self.avgpool(x)
        features = torch.flatten(x, 1)

        return {
            "features": features,
            "attractiveness": torch.sigmoid(self.attractiveness_head(features)),
            "authenticity": F.softmax(self.authenticity_head(features), dim=1),
            "quality": F.softmax(self.quality_head(features), dim=1),
            "inappropriate": F.softmax(self.inappropriate_head(features), dim=1),
            "smile": torch.sigmoid(self.smile_head(features)),
            "style_embedding": self.style_projector(features)
        }

# ------------------------------------------------------------------------------------
# Vision: EfficientNet-ish (very simplified)
# ------------------------------------------------------------------------------------

class MBConvBlock(nn.Module):
    def __init__(self, in_channels, out_channels, expand_ratio, stride, kernel_size):
        super().__init__()
        self.stride = stride
        self.use_residual = stride == 1 and in_channels == out_channels
        hidden = in_channels * expand_ratio
        self.expand = nn.Sequential() if expand_ratio == 1 else nn.Sequential(
            nn.Conv2d(in_channels, hidden, 1, bias=False),
            nn.BatchNorm2d(hidden),
            nn.SiLU(inplace=True)
        )
        self.depthwise = nn.Sequential(
            nn.Conv2d(hidden, hidden, kernel_size, stride, kernel_size//2, groups=hidden, bias=False),
            nn.BatchNorm2d(hidden),
            nn.SiLU(inplace=True)
        )
        self.se_avg = nn.AdaptiveAvgPool2d(1)
        self.se_reduce = nn.Conv2d(hidden, max(1, hidden // 4), 1)
        self.se_expand = nn.Conv2d(max(1, hidden // 4), hidden, 1)
        self.project = nn.Sequential(
            nn.Conv2d(hidden, out_channels, 1, bias=False),
            nn.BatchNorm2d(out_channels)
        )

    def forward(self, x):
        identity = x
        x = self.expand(x)
        x = self.depthwise(x)
        # S/E
        w = self.se_avg(x)
        w = torch.sigmoid(self.se_expand(F.silu(self.se_reduce(w))))
        x = x * w
        x = self.project(x)
        if self.use_residual:
            x = x + identity
        return x

class DatingAppEfficientNet(nn.Module):
    def __init__(self):
        super().__init__()
        self.stem = nn.Sequential(
            nn.Conv2d(3, 32, 3, 2, 1, bias=False),
            nn.BatchNorm2d(32),
            nn.SiLU(inplace=True)
        )
        self.blocks = nn.Sequential(
            MBConvBlock(32, 16, 1, 1, 3),
            MBConvBlock(16, 24, 6, 2, 3),
            MBConvBlock(24, 40, 6, 2, 5),
            MBConvBlock(40, 80, 6, 2, 3),
            MBConvBlock(80, 112, 6, 1, 5),
            MBConvBlock(112, 192, 6, 2, 5),
            MBConvBlock(192, 320, 6, 1, 3)
        )
        self.head = nn.Sequential(
            nn.Conv2d(320, 1280, 1, bias=False),
            nn.BatchNorm2d(1280),
            nn.SiLU(inplace=True),
            nn.AdaptiveAvgPool2d(1)
        )
        # Heads
        self.face_quality_head = nn.Sequential(
            nn.Linear(1280, 512), nn.ReLU(), nn.Dropout(0.2), nn.Linear(512, 3)
        )
        self.age_group_head = nn.Sequential(
            nn.Linear(1280, 256), nn.ReLU(), nn.Linear(256, 5)
        )
        self.photo_type_head = nn.Sequential(
            nn.Linear(1280, 256), nn.ReLU(), nn.Linear(256, 6)
        )
        self.compatibility_encoder = nn.Sequential(
            nn.Linear(1280, 512), nn.ReLU(), nn.Linear(512, 256), nn.ReLU(), nn.Linear(256, 128)
        )

    def forward(self, x):
        x = self.stem(x)
        x = self.blocks(x)
        x = self.head(x)
        features = torch.flatten(x, 1)
        return {
            "features": features,
            "face_quality": F.softmax(self.face_quality_head(features), dim=1),
            "age_group": F.softmax(self.age_group_head(features), dim=1),
            "photo_type": F.softmax(self.photo_type_head(features), dim=1),
            "compatibility_features": self.compatibility_encoder(features)
        }

# ------------------------------------------------------------------------------------
# Vision: ViT (compact)
# ------------------------------------------------------------------------------------

class PatchEmbedding(nn.Module):
    def __init__(self, img_size=224, patch_size=16, in_channels=3, embed_dim=768):
        super().__init__()
        self.num_patches = (img_size // patch_size) ** 2
        self.proj = nn.Conv2d(in_channels, embed_dim, kernel_size=patch_size, stride=patch_size)
    def forward(self, x):
        x = self.proj(x)       # (B, E, H', W')
        x = x.flatten(2).transpose(1, 2)  # (B, N, E)
        return x

class TransformerBlock(nn.Module):
    def __init__(self, embed_dim, num_heads, mlp_ratio=4.0, dropout=0.1):
        super().__init__()
        self.norm1 = nn.LayerNorm(embed_dim)
        self.attn = nn.MultiheadAttention(embed_dim, num_heads, dropout=dropout, batch_first=True)
        self.norm2 = nn.LayerNorm(embed_dim)
        self.mlp = nn.Sequential(
            nn.Linear(embed_dim, int(embed_dim * mlp_ratio)),
            nn.GELU(), nn.Dropout(dropout),
            nn.Linear(int(embed_dim * mlp_ratio), embed_dim),
            nn.Dropout(dropout),
        )
    def forward(self, x):
        x = x + self.attn(self.norm1(x), self.norm1(x), self.norm1(x))[0]
        x = x + self.mlp(self.norm2(x))
        return x

class DatingAppVisionTransformer(nn.Module):
    def __init__(self, img_size=224, patch_size=16, embed_dim=384, depth=6, num_heads=6, mlp_ratio=4.0):
        super().__init__()
        self.embed_dim = embed_dim
        self.patch_embed = PatchEmbedding(img_size, patch_size, 3, embed_dim)
        self.cls_token = nn.Parameter(torch.zeros(1, 1, embed_dim))
        self.pos_embed = nn.Parameter(torch.zeros(1, 1 + self.patch_embed.num_patches, embed_dim))
        self.blocks = nn.ModuleList([TransformerBlock(embed_dim, num_heads, mlp_ratio) for _ in range(depth)])
        self.norm = nn.LayerNorm(embed_dim)

        self.personality_head = nn.Sequential(
            nn.Linear(embed_dim, 512), nn.ReLU(), nn.Dropout(0.3), nn.Linear(512, 5)
        )
        self.lifestyle_head = nn.Sequential(
            nn.Linear(embed_dim, 512), nn.ReLU(), nn.Linear(512, 8)
        )
        self.verification_head = nn.Sequential(
            nn.Linear(embed_dim, 256), nn.ReLU(), nn.Linear(256, 2)
        )
        self.aesthetic_encoder = nn.Sequential(
            nn.Linear(embed_dim, 512), nn.ReLU(), nn.Linear(512, 256), nn.ReLU(), nn.Linear(256, 128)
        )

        nn.init.trunc_normal_(self.pos_embed, std=0.02)
        nn.init.trunc_normal_(self.cls_token, std=0.02)

    def forward(self, x):
        B = x.size(0)
        x = self.patch_embed(x)
        cls = self.cls_token.expand(B, -1, -1)
        x = torch.cat([cls, x], dim=1)
        x = x + self.pos_embed
        for blk in self.blocks:
            x = blk(x)
        x = self.norm(x)
        cls_out = x[:, 0, :]
        return {
            "features": cls_out,
            "personality_traits": F.softmax(self.personality_head(cls_out), dim=1),
            "lifestyle": F.softmax(self.lifestyle_head(cls_out), dim=1),
            "verification_score": F.softmax(self.verification_head(cls_out), dim=1),
            "aesthetic_features": self.aesthetic_encoder(cls_out)
        }

# ------------------------------------------------------------------------------------
# Vision: Analyzer wrapper
# ------------------------------------------------------------------------------------

class DatingAppVisionAnalyzer:
    def __init__(self, device='cuda' if torch.cuda.is_available() else 'cpu'):
        self.device = torch.device(device)
        self.resnet = DatingAppResNet().to(self.device).eval()
        self.efficientnet = DatingAppEfficientNet().to(self.device).eval()
        self.vit = DatingAppVisionTransformer().to(self.device).eval()

        self.transform = transforms.Compose([
            transforms.Resize((224, 224)),
            transforms.ToTensor(),
            transforms.Normalize(mean=[0.485, 0.456, 0.406],
                                 std=[0.229, 0.224, 0.225])
        ])

        self.cache: Dict[str, ImageAnalysisResult] = {}
        self.cache_size = 1000

    def _get_cache_key(self, image_data: Union[str, bytes]) -> str:
        if isinstance(image_data, str):
            return hashlib.md5(image_data.encode()).hexdigest()
        return hashlib.md5(image_data).hexdigest()

    def preprocess_image(self, image_data: Union[str, bytes, Image.Image]) -> torch.Tensor:
        if isinstance(image_data, str):
            img = Image.open(io.BytesIO(base64.b64decode(image_data)))
        elif isinstance(image_data, bytes):
            img = Image.open(io.BytesIO(image_data))
        else:
            img = image_data
        if img.mode != "RGB":
            img = img.convert("RGB")
        return self.transform(img).unsqueeze(0).to(self.device)

    @torch.no_grad()
    def analyze_image(self, image_data: Union[str, bytes, Image.Image], use_cache: bool = True) -> ImageAnalysisResult:
        cache_key = self._get_cache_key(str(image_data)[:100]) if use_cache else None
        if cache_key and cache_key in self.cache:
            return self.cache[cache_key]

        t = self.preprocess_image(image_data)

        r = self.resnet(t)
        e = self.efficientnet(t)
        v = self.vit(t)

        attractiveness_score = float(
            0.4 * r["attractiveness"][0, 0]
            + 0.3 * e["face_quality"][0, 2]  # high-quality bucket
            + 0.3 * v["verification_score"][0, 0]
        )
        authenticity_score = float(0.5 * r["authenticity"][0, 0] + 0.5 * v["verification_score"][0, 0])
        quality_idx = int(torch.argmax(e["face_quality"][0]).item())
        quality_score = quality_idx / 2.0  # map {0,1,2} -> {0.0,0.5,1.0}

        inappropriate_content = bool(r["inappropriate"][0, 1] > 0.7)
        smile_intensity = float(r["smile"][0, 0])

        style_embedding = np.concatenate([
            r["style_embedding"][0].detach().cpu().numpy(),
            e["compatibility_features"][0].detach().cpu().numpy(),
            v["aesthetic_features"][0].detach().cpu().numpy(),
        ])

        confidence_scores = {
            "attractiveness": float(torch.std(torch.tensor([
                r["attractiveness"][0, 0],
                e["face_quality"][0, 2],
                v["verification_score"][0, 0]
            ]))),
            "overall": 0.85
        }

        result = ImageAnalysisResult(
            attractiveness_score=float(np.clip(attractiveness_score, 0, 1)),
            authenticity_score=float(np.clip(authenticity_score, 0, 1)),
            quality_score=float(np.clip(quality_score, 0, 1)),
            inappropriate_content=inappropriate_content,
            face_detected=quality_score > 0.3,
            smile_intensity=float(np.clip(smile_intensity, 0, 1)),
            style_embedding=style_embedding,
            confidence_scores=confidence_scores
        )

        if cache_key and len(self.cache) < self.cache_size:
            self.cache[cache_key] = result
        return result

    @torch.no_grad()
    def get_photo_insights(self, image_data: Union[str, bytes, Image.Image]) -> Dict:
        analysis = self.analyze_image(image_data)

        t = self.preprocess_image(image_data)
        e = self.efficientnet(t)
        v = self.vit(t)

        photo_types = ['selfie', 'group', 'outdoor', 'gym', 'formal', 'casual']
        age_groups = ['18-24', '25-30', '31-40', '41-50', '50+']
        traits = ['openness', 'conscientiousness', 'extraversion', 'agreeableness', 'neuroticism']

        photo_type = photo_types[int(torch.argmax(e["photo_type"][0]).item())]
        age_group = age_groups[int(torch.argmax(e["age_group"][0]).item())]
        personality_scores = v["personality_traits"][0].detach().cpu().numpy()

        insights = {
            "overall_score": (analysis.attractiveness_score + analysis.quality_score) / 2.0,
            "photo_type": photo_type,
            "estimated_age_group": age_group,
            "quality_rating": ['low', 'medium', 'high'][min(2, int(analysis.quality_score * 2.99))],
            "authenticity": 'authentic' if analysis.authenticity_score > 0.7 else 'possibly edited',
            "smile_detected": analysis.smile_intensity > 0.5,
            "personality_hints": {tname: float(val) for tname, val in zip(traits, personality_scores)},
            "recommendations": self._generate_photo_recommendations(analysis)
        }
        return insights

    def _generate_photo_recommendations(self, a: ImageAnalysisResult) -> List[str]:
        tips = []
        if a.quality_score < 0.5:
            tips.append("Use better lighting or a sharper camera.")
        if a.authenticity_score < 0.6:
            tips.append("Keep edits minimal—natural photos perform better.")
        if a.smile_intensity < 0.3:
            tips.append("A genuine smile tends to increase likes.")
        if not a.face_detected:
            tips.append("Ensure your face is clearly visible in at least one photo.")
        return tips

    def calculate_visual_compatibility(self, user1_images: List, user2_images: List) -> float:
        u1 = []
        u2 = []
        for img in user1_images[:5]:
            u1.append(self.analyze_image(img).style_embedding)
        for img in user2_images[:5]:
            u2.append(self.analyze_image(img).style_embedding)
        if not u1 or not u2:
            return 0.5
        u1_avg = np.mean(u1, axis=0)
        u2_avg = np.mean(u2, axis=0)
        sim = np.dot(u1_avg, u2_avg) / (np.linalg.norm(u1_avg) * np.linalg.norm(u2_avg) + 1e-8)
        return float((sim + 1) / 2.0)

# ------------------------------------------------------------------------------------
# Core ML: User features, RL agent, federated learning, feature extractor, matching
# ------------------------------------------------------------------------------------

@dataclass
class UserFeatures:
    age: float
    gender_encoded: float  # 0 male, 1 female, 0.5 other
    activity_score: float
    selectivity_score: float
    bio_sentiment: float   # -1..1
    photo_attractiveness: float
    location_cluster: int

    def to_vector(self) -> np.ndarray:
        return np.array([
            self.age / 100.0,
            self.gender_encoded,
            self.activity_score,
            self.selectivity_score,
            (self.bio_sentiment + 1) / 2.0,
            self.photo_attractiveness,
            self.location_cluster / 10.0
        ], dtype=np.float32)

@dataclass
class SwipeAction:
    user_id: int
    target_user_id: int
    action: int  # 0 pass, 1 like
    timestamp: datetime
    features: UserFeatures
    target_features: UserFeatures
    reward: float = 0.0

class ReinforcementLearningAgent:
    def __init__(self, state_size: int = 14, action_size: int = 2, learning_rate: float = 0.01):
        self.state_size = state_size
        self.action_size = action_size
        self.learning_rate = learning_rate
        self.epsilon = 0.3
        self.epsilon_decay = 0.995
        self.epsilon_min = 0.01
        self.gamma = 0.95

        self.q_weights = np.random.normal(0, 0.1, (state_size, action_size))
        self.experience_replay = deque(maxlen=10000)
        self.user_models: Dict[int, np.ndarray] = {}
        self.user_stats: Dict[int, Dict] = defaultdict(lambda: {
            'total_swipes': 0, 'likes_given': 0, 'matches_received': 0, 'response_rate': 0.0
        })

    def get_state(self, u: UserFeatures, t: UserFeatures) -> np.ndarray:
        return np.concatenate([u.to_vector(), t.to_vector()])

    def get_user_model(self, user_id: int) -> np.ndarray:
        if user_id not in self.user_models:
            self.user_models[user_id] = self.q_weights + np.random.normal(0, 0.05, self.q_weights.shape)
        return self.user_models[user_id]

    def predict_action(self, user_id: int, u: UserFeatures, t: UserFeatures) -> int:
        state = self.get_state(u, t)
        W = self.get_user_model(user_id)
        if np.random.random() <= self.epsilon:
            return np.random.randint(self.action_size)
        q = np.dot(state, W)
        return int(np.argmax(q))

    def get_recommendation_score(self, user_id: int, u: UserFeatures, t: UserFeatures) -> float:
        state = self.get_state(u, t)
        W = self.get_user_model(user_id)
        q = np.dot(state, W)
        exp = np.exp(q - np.max(q))
        prob = exp / np.sum(exp)
        return float(prob[1])

    def update_q_values(self, user_id: int, state: np.ndarray, action: int, reward: float, next_state: np.ndarray):
        W = self.get_user_model(user_id)
        current_q = np.dot(state, W)[action]
        next_q_max = np.max(np.dot(next_state, W))
        target = reward + self.gamma * next_q_max
        error = target - current_q
        grad = error * state
        W[:, action] += self.learning_rate * grad
        alpha = 0.1
        self.q_weights = (1 - alpha) * self.q_weights + alpha * W

    def record_swipe(self, s: SwipeAction):
        self.experience_replay.append(s)
        st = self.user_stats[s.user_id]
        st['total_swipes'] += 1
        if s.action == 1:
            st['likes_given'] += 1

    def update_rewards(self, user_id: int, target_user_id: int, match: bool, response_received: bool = False):
        reward = 0.0
        if match:
            reward += 5.0
            self.user_stats[user_id]['matches_received'] += 1
        if response_received:
            reward += 2.0
        for exp in reversed(self.experience_replay):
            if exp.user_id == user_id and exp.target_user_id == target_user_id and exp.action == 1:
                exp.reward = reward
                state = self.get_state(exp.features, exp.target_features)
                self.update_q_values(user_id, state, exp.action, reward, state)
                break

    def train_batch(self, batch_size: int = 32):
        if len(self.experience_replay) < batch_size:
            # Still decay epsilon slowly
            if self.epsilon > self.epsilon_min:
                self.epsilon *= self.epsilon_decay
            return
        batch = np.random.choice(list(self.experience_replay), batch_size, replace=False)
        for exp in batch:
            if exp.reward != 0:
                state = self.get_state(exp.features, exp.target_features)
                self.update_q_values(exp.user_id, state, exp.action, exp.reward, state)
        if self.epsilon > self.epsilon_min:
            self.epsilon *= self.epsilon_decay

    def get_user_insights(self, user_id: int) -> Dict:
        st = self.user_stats[user_id]
        selectivity = 1 - (st['likes_given'] / max(st['total_swipes'], 1))
        return {
            'total_swipes': st['total_swipes'],
            'selectivity_score': selectivity,
            'match_rate': st['matches_received'] / max(st['likes_given'], 1),
            'epsilon': self.epsilon,
            'model_maturity': min(st['total_swipes'] / 100, 1.0)
        }

class FederatedLearningManager:
    def __init__(self, global_model_path: str = "global_model.pkl"):
        self.global_model_path = global_model_path
        self.client_updates: Dict[str, Dict] = {}
        self.aggregation_threshold = 10
        self.privacy_budget = 1.0
        self.lock = threading.Lock()
        self.global_weights = self.load_global_model()
        self.last_aggregation_time = 'Never'

    def load_global_model(self) -> np.ndarray:
        try:
            if os.path.exists(self.global_model_path):
                with open(self.global_model_path, 'rb') as f:
                    return pickle.load(f)
        except Exception as e:
            logger.error(f"Error loading global model: {e}")
        return np.random.normal(0, 0.1, (14, 2))

    def save_global_model(self):
        try:
            with open(self.global_model_path, 'wb') as f:
                pickle.dump(self.global_weights, f)
        except Exception as e:
            logger.error(f"Error saving global model: {e}")

    def generate_client_id(self, user_id: int, session_info: str) -> str:
        client_data = f"{user_id}_{session_info}_{datetime.now().date()}"
        return hashlib.sha256(client_data.encode()).hexdigest()[:16]

    def add_noise_for_privacy(self, weights: np.ndarray, noise_scale: float = 0.1) -> np.ndarray:
        if self.privacy_budget <= 0:
            return weights
        noise = np.random.laplace(0, noise_scale, weights.shape)
        self.privacy_budget = max(0.0, self.privacy_budget - 0.1)
        return weights + noise

    def submit_client_update(self, user_id: int, local_weights: np.ndarray,
                             training_samples: int, session_info: str = "default") -> bool:
        client_id = self.generate_client_id(user_id, session_info)
        with self.lock:
            noisy = self.add_noise_for_privacy(local_weights)
            self.client_updates[client_id] = {
                'weights': noisy,
                'samples': training_samples,
                'timestamp': datetime.now(),
                'user_id_hash': hashlib.sha256(str(user_id).encode()).hexdigest()[:8]
            }
            logger.info(f"Client {client_id} update received ({training_samples} samples)")
            if len(self.client_updates) >= self.aggregation_threshold:
                self.federated_averaging()
                return True
        return False

    def federated_averaging(self):
        if not self.client_updates:
            return
        logger.info(f"Federated averaging over {len(self.client_updates)} clients")
        total_samples = sum(u['samples'] for u in self.client_updates.values())
        weighted = np.zeros_like(self.global_weights)
        for cid, upd in self.client_updates.items():
            w = upd['samples'] / max(total_samples, 1)
            weighted += w * upd['weights']
        lr = 0.1
        self.global_weights = (1 - lr) * self.global_weights + lr * weighted
        self.save_global_model()
        self.client_updates.clear()
        self.last_aggregation_time = datetime.now().isoformat(timespec='seconds')
        logger.info("Global model updated.")

    def get_global_model(self) -> np.ndarray:
        return self.global_weights.copy()

    def get_federated_stats(self) -> Dict:
        return {
            'active_clients': len(self.client_updates),
            'aggregation_threshold': self.aggregation_threshold,
            'privacy_budget_remaining': self.privacy_budget,
            'last_aggregation': self.last_aggregation_time,
            'global_model_version': hashlib.md5(self.global_weights.tobytes()).hexdigest()[:8]
        }

class UserFeatureExtractor:
    def __init__(self):
        self.cache: Dict[int, UserFeatures] = {}
        self.cache_timeout = timedelta(minutes=30)
        self.last_cache_clear = datetime.now()
        self.extraction_count = 0

    def extract_user_features(self, user_id: int, force_refresh: bool = False) -> Optional[UserFeatures]:
        if datetime.now() - self.last_cache_clear > self.cache_timeout:
            self.cache.clear()
            self.last_cache_clear = datetime.now()
        if user_id in self.cache and not force_refresh:
            return self.cache[user_id]
        try:
            features = self._extract_features_from_db(user_id)
            if features:
                self.cache[user_id] = features
                self.extraction_count += 1
            return features
        except Exception as e:
            logger.error(f"Feature extraction error for user {user_id}: {e}")
            return None

    def _extract_features_from_db(self, user_id: int) -> Optional[UserFeatures]:
        # TODO: replace with your DB
        import random
        return UserFeatures(
            age=random.uniform(18, 45),
            gender_encoded=random.choice([0.0, 1.0, 0.5]),
            activity_score=random.uniform(0, 1),
            selectivity_score=random.uniform(0, 1),
            bio_sentiment=random.uniform(-0.5, 0.5),
            photo_attractiveness=random.uniform(0.3, 0.9),
            location_cluster=random.randint(0, 5)
        )

    def get_stats(self) -> Dict:
        return {
            "cached_users": len(self.cache),
            "total_extractions": self.extraction_count,
            "cache_hit_rate": 1 - (self.extraction_count / max(len(self.cache) * 2, 1))
        }

# ------------------------------------------------------------------------------------
# Integration: Enhance feature extractor to use vision analyzer for photo score
# ------------------------------------------------------------------------------------

class EnhancedUserFeatureExtractor:
    def __init__(self, original_extractor: UserFeatureExtractor, vision_analyzer: DatingAppVisionAnalyzer):
        self.original_extractor = original_extractor
        self.vision_analyzer = vision_analyzer

    def extract_user_features(self, user_id: int, force_refresh: bool = False,
                              user_photos: Optional[List] = None) -> Optional[UserFeatures]:
        features = self.original_extractor.extract_user_features(user_id, force_refresh)
        if features and user_photos:
            photo_scores = []
            for photo in user_photos[:3]:
                try:
                    analysis = self.vision_analyzer.analyze_image(photo)
                    photo_scores.append(analysis.attractiveness_score)
                except Exception as e:
                    logger.error(f"Vision analysis failed for user {user_id}: {e}")
            if photo_scores:
                features.photo_attractiveness = float(np.mean(photo_scores))
        return features

# ------------------------------------------------------------------------------------
# Matching Intelligence: main orchestrator
# ------------------------------------------------------------------------------------

class MatchingIntelligence:
    def __init__(self):
        self.rl_agent = ReinforcementLearningAgent()
        self.federated_manager = FederatedLearningManager()
        self.feature_extractor = UserFeatureExtractor()

        # Load any global model
        self.rl_agent.q_weights = self.federated_manager.get_global_model()

        # Vision (will be attached by integrate_vision_models)
        self.vision_analyzer: Optional[DatingAppVisionAnalyzer] = None

        self.last_training_time = datetime.now()
        self.training_interval = timedelta(hours=1)

    def get_smart_recommendations(self, user_id: int, potential_matches: List[Dict],
                                  limit: int = 10) -> List[Dict]:
        user_features = self.feature_extractor.extract_user_features(user_id)
        if not user_features:
            return potential_matches[:limit]

        scored = []
        for match in potential_matches:
            target_features = self.feature_extractor.extract_user_features(match['id'])
            if target_features is None:
                continue
            score = self.rl_agent.get_recommendation_score(user_id, user_features, target_features)
            match['ai_score'] = score
            scored.append((score, match))

        scored.sort(key=lambda x: x[0], reverse=True)
        return [m for _, m in scored[:limit]]

    def record_swipe_action(self, user_id: int, target_user_id: int, action: str) -> Dict:
        u = self.feature_extractor.extract_user_features(user_id)
        t = self.feature_extractor.extract_user_features(target_user_id)
        if not u or not t:
            return {"status": "error", "message": "Could not extract features"}

        swipe = SwipeAction(
            user_id=user_id,
            target_user_id=target_user_id,
            action=1 if action.lower() == 'like' else 0,
            timestamp=datetime.now(),
            features=u,
            target_features=t
        )
        self.rl_agent.record_swipe(swipe)

        if datetime.now() - self.last_training_time > self.training_interval:
            self.trigger_learning_update(user_id)
            self.last_training_time = datetime.now()

        return {
            "status": "success",
            "message": "Swipe recorded",
            "user_insights": self.rl_agent.get_user_insights(user_id)
        }

    def update_match_outcome(self, user_id: int, target_user_id: int,
                             match_occurred: bool, response_received: bool = False):
        self.rl_agent.update_rewards(user_id, target_user_id, match_occurred, response_received)
        if match_occurred:
            self.trigger_learning_update(user_id)

    def trigger_learning_update(self, user_id: int):
        self.rl_agent.train_batch()
        user_weights = self.rl_agent.get_user_model(user_id)
        samples = self.rl_agent.user_stats[user_id]['total_swipes']
        if samples > 10:
            session_info = f"session_{datetime.now().hour}"
            updated = self.federated_manager.submit_client_update(user_id, user_weights, samples, session_info)
            if updated:
                self.rl_agent.q_weights = self.federated_manager.get_global_model()
                logger.info(f"Local RL synced to new global model (user {user_id})")

    def get_system_stats(self) -> Dict:
        return {
            "reinforcement_learning": {
                "total_experiences": len(self.rl_agent.experience_replay),
                "epsilon": self.rl_agent.epsilon,
                "active_user_models": len(self.rl_agent.user_models)
            },
            "federated_learning": self.federated_manager.get_federated_stats(),
            "feature_extraction": self.feature_extractor.get_stats()
        }

# ------------------------------------------------------------------------------------
# Integration glue: attach vision analyzer + visual recommendations
# ------------------------------------------------------------------------------------

def integrate_vision_models(matching_intelligence_instance: MatchingIntelligence) -> MatchingIntelligence:
    va = DatingAppVisionAnalyzer()
    # swap in enhanced extractor that can use photos when provided
    enhanced_extractor = EnhancedUserFeatureExtractor(matching_intelligence_instance.feature_extractor, va)
    matching_intelligence_instance.feature_extractor = enhanced_extractor  # type: ignore
    matching_intelligence_instance.vision_analyzer = va

    def get_visual_recommendations(self: MatchingIntelligence, user_id: int, user_photos: List,
                                   potential_matches: List[Dict]) -> List[Dict]:
        if self.vision_analyzer is None:
            return potential_matches
        out = []
        for m in potential_matches:
            if 'photos' not in m or not m['photos']:
                continue
            compat = self.vision_analyzer.calculate_visual_compatibility(user_photos, m['photos'])
            rl = m.get('ai_score', 0.5)
            combined = 0.6 * rl + 0.4 * compat
            m['visual_compatibility'] = compat
            m['combined_score'] = combined
            out.append((combined, m))
        out.sort(key=lambda x: x[0], reverse=True)
        return [m for _, m in out]

    # bind
    matching_intelligence_instance.get_visual_recommendations = get_visual_recommendations.__get__(
        matching_intelligence_instance, matching_intelligence_instance.__class__
    )
    logger.info("Vision models integrated into MatchingIntelligence.")
    return matching_intelligence_instance

# ------------------------------------------------------------------------------------
# Ready-to-use global instance
# ------------------------------------------------------------------------------------

matching_intelligence = integrate_vision_models(MatchingIntelligence())

# ------------------------------------------------------------------------------------
# Optional: quick smoke test (comment out in prod)
# ------------------------------------------------------------------------------------
if __name__ == "__main__":
    # Example: analyze one image if available
    try:
        path = "user_photo.jpg"
        if os.path.exists(path):
            with open(path, "rb") as f:
                photo = f.read()
            analysis = matching_intelligence.vision_analyzer.analyze_image(photo)  # type: ignore
            print("Photo analysis:", analysis)

            insights = matching_intelligence.vision_analyzer.get_photo_insights(photo)  # type: ignore
            print("Photo insights:", json.dumps(insights, indent=2))
        else:
            print("Put a test image at user_photo.jpg to smoke-test vision.")
    except Exception as e:
        logger.exception(e)

    # Example: smart recs
    candidates = [{"id": i, "photos": []} for i in range(2, 12)]
    recs = matching_intelligence.get_smart_recommendations(user_id=1, potential_matches=candidates, limit=5)
    print("AI recs (no photos):", [r["id"] for r in recs])
