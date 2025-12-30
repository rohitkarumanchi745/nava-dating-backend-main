"""
Face Verification Module for Dating App
Implements real-time selfie verification with liveness detection
"""

import torch
import torch.nn as nn
import torch.nn.functional as F
import torchvision.transforms as transforms
from PIL import Image
import numpy as np
import cv2
import face_recognition
from typing import Dict, List, Tuple, Optional, Union
from dataclasses import dataclass
import logging
from datetime import datetime, timedelta
import hashlib
from collections import deque
import asyncio
import time

# Configure logging
logger = logging.getLogger(__name__)

# ============================================
# Data Classes
# ============================================

@dataclass
class SelfieVerificationResult:
    """Results from selfie verification"""
    is_match: bool
    confidence_score: float
    liveness_score: float  # Anti-spoofing score
    face_match_score: float
    verification_timestamp: datetime
    failure_reasons: List[str]
    rl_reward: float  # Reward for RL system
    federated_update: bool  # Whether to trigger federated learning

# ============================================
# ResNet Block for Face Encoder
# ============================================

class ResNetBlock(nn.Module):
    """Basic ResNet block for face encoding"""
    def __init__(self, in_channels, out_channels, stride=1):
        super().__init__()
        self.conv1 = nn.Conv2d(in_channels, out_channels, kernel_size=3,
                               stride=stride, padding=1, bias=False)
        self.bn1 = nn.BatchNorm2d(out_channels)
        self.conv2 = nn.Conv2d(out_channels, out_channels, kernel_size=3,
                               padding=1, bias=False)
        self.bn2 = nn.BatchNorm2d(out_channels)
        self.relu = nn.ReLU(inplace=True)
        
        # Shortcut connection
        self.shortcut = nn.Sequential()
        if stride != 1 or in_channels != out_channels:
            self.shortcut = nn.Sequential(
                nn.Conv2d(in_channels, out_channels, kernel_size=1,
                         stride=stride, bias=False),
                nn.BatchNorm2d(out_channels)
            )
    
    def forward(self, x):
        identity = x
        out = self.relu(self.bn1(self.conv1(x)))
        out = self.bn2(self.conv2(out))
        out += self.shortcut(identity)
        return self.relu(out)

# ============================================
# Face Encoder Network
# ============================================

class FaceEncoder(nn.Module):
    """Specialized face encoding network for identity verification"""
    
    def __init__(self, backbone='resnet'):
        super().__init__()
        
        # Initial convolution
        self.conv1 = nn.Conv2d(3, 64, kernel_size=7, stride=2, padding=3, bias=False)
        self.bn1 = nn.BatchNorm2d(64)
        self.relu = nn.ReLU(inplace=True)
        self.maxpool = nn.MaxPool2d(kernel_size=3, stride=2, padding=1)
        
        # ResNet layers
        self.layer1 = self._make_layer(64, 64, 3, stride=1)
        self.layer2 = self._make_layer(64, 128, 4, stride=2)
        self.layer3 = self._make_layer(128, 256, 6, stride=2)
        self.layer4 = self._make_layer(256, 512, 3, stride=2)
        
        # Global average pooling
        self.avgpool = nn.AdaptiveAvgPool2d((1, 1))
        
        # Face embedding layers
        self.face_projector = nn.Sequential(
            nn.Linear(512, 256),
            nn.ReLU(),
            nn.Dropout(0.2),
            nn.Linear(256, 128)  # 128-dim face embedding
        )
        
        # Batch normalization for embeddings
        self.embedding_bn = nn.BatchNorm1d(128)
        
        # Liveness detection head
        self.liveness_detector = nn.Sequential(
            nn.Linear(512, 256),
            nn.ReLU(),
            nn.Dropout(0.3),
            nn.Linear(256, 2)  # Real vs Fake
        )
        
        # Quality assessment head
        self.quality_assessor = nn.Sequential(
            nn.Linear(512, 128),
            nn.ReLU(),
            nn.Linear(128, 1)  # Quality score
        )
        
        # Initialize weights
        self._initialize_weights()
    
    def _make_layer(self, in_channels, out_channels, num_blocks, stride):
        layers = []
        layers.append(ResNetBlock(in_channels, out_channels, stride))
        for _ in range(1, num_blocks):
            layers.append(ResNetBlock(out_channels, out_channels, 1))
        return nn.Sequential(*layers)
    
    def _initialize_weights(self):
        for m in self.modules():
            if isinstance(m, nn.Conv2d):
                nn.init.kaiming_normal_(m.weight, mode='fan_out', nonlinearity='relu')
            elif isinstance(m, nn.BatchNorm2d) or isinstance(m, nn.BatchNorm1d):
                nn.init.constant_(m.weight, 1)
                nn.init.constant_(m.bias, 0)
    
    def forward(self, x):
        # Feature extraction
        x = self.conv1(x)
        x = self.bn1(x)
        x = self.relu(x)
        x = self.maxpool(x)
        
        x = self.layer1(x)
        x = self.layer2(x)
        x = self.layer3(x)
        x = self.layer4(x)
        
        x = self.avgpool(x)
        features = x.view(x.size(0), -1)
        
        # Face embedding
        face_embedding = self.face_projector(features)
        if face_embedding.size(0) > 1:  # Only apply batch norm if batch size > 1
            face_embedding = self.embedding_bn(face_embedding)
        face_embedding = F.normalize(face_embedding, p=2, dim=1)  # L2 normalize
        
        # Liveness detection
        liveness = self.liveness_detector(features)
        
        # Quality assessment
        quality = torch.sigmoid(self.quality_assessor(features))
        
        return {
            'embedding': face_embedding,
            'liveness': F.softmax(liveness, dim=1),
            'quality': quality
        }

# ============================================
# Motion-Based Liveness Detection
# ============================================

class MotionBasedLivenessDetector:
    """Detect liveness based on motion analysis"""
    
    def __init__(self):
        self.motion_threshold = 0.02
        self.required_frames = 5
        self.optical_flow_params = dict(
            pyr_scale=0.5,
            levels=3,
            winsize=15,
            iterations=3,
            poly_n=5,
            poly_sigma=1.2,
            flags=0
        )
    
    async def check_liveness(self, video_frames: Union[np.ndarray, List[np.ndarray]]) -> float:
        """
        Check liveness using motion detection
        In production, this would analyze multiple frames from video
        """
        
        # For single image, return moderate score
        if isinstance(video_frames, np.ndarray):
            return 0.6
        
        if len(video_frames) < self.required_frames:
            return 0.3
        
        # Calculate motion between frames
        motion_scores = []
        for i in range(1, len(video_frames)):
            prev_frame = cv2.cvtColor(video_frames[i-1], cv2.COLOR_RGB2GRAY)
            curr_frame = cv2.cvtColor(video_frames[i], cv2.COLOR_RGB2GRAY)
            
            # Calculate optical flow
            flow = cv2.calcOpticalFlowFarneback(
                prev_frame, curr_frame, None, **self.optical_flow_params
            )
            
            # Calculate motion magnitude
            magnitude = np.sqrt(flow[..., 0]**2 + flow[..., 1]**2)
            motion_score = np.mean(magnitude)
            motion_scores.append(motion_score)
        
        avg_motion = np.mean(motion_scores)
        std_motion = np.std(motion_scores)
        
        # Natural motion should be moderate and consistent
        if self.motion_threshold < avg_motion < 0.5 and std_motion < 0.2:
            return 0.9
        elif avg_motion < self.motion_threshold:
            return 0.3  # Too static (possible photo)
        elif avg_motion > 0.5:
            return 0.4  # Too much motion (possible video playback)
        else:
            return 0.6  # Uncertain

# ============================================
# Texture-Based Spoof Detection
# ============================================

class TextureBasedSpoofDetector:
    """Detect spoofing attempts using texture analysis"""
    
    def __init__(self):
        self.lbp_radius = 1
        self.lbp_points = 8
        
    def analyze_texture(self, face_image: np.ndarray) -> float:
        """Analyze face texture to detect printed photos or screens"""
        
        # Convert to grayscale
        if len(face_image.shape) == 3:
            gray = cv2.cvtColor(face_image, cv2.COLOR_RGB2GRAY)
        else:
            gray = face_image
        
        # Calculate Local Binary Pattern (LBP) features
        lbp_features = self._compute_lbp(gray)
        
        # Calculate frequency domain features (detect screen patterns)
        freq_features = self._compute_frequency_features(gray)
        
        # Calculate color distribution features
        color_features = self._compute_color_features(face_image) if len(face_image.shape) == 3 else 0.5
        
        # Combine features for spoofing detection
        texture_score = (lbp_features * 0.4 + freq_features * 0.4 + color_features * 0.2)
        
        return texture_score
    
    def _compute_lbp(self, image: np.ndarray) -> float:
        """Compute Local Binary Pattern features"""
        
        h, w = image.shape
        lbp = np.zeros_like(image)
        
        # Compute LBP for each pixel
        for i in range(1, h-1):
            for j in range(1, w-1):
                center = image[i, j]
                code = 0
                
                # 8-neighborhood
                neighbors = [
                    image[i-1, j-1], image[i-1, j], image[i-1, j+1],
                    image[i, j+1], image[i+1, j+1], image[i+1, j],
                    image[i+1, j-1], image[i, j-1]
                ]
                
                for k, neighbor in enumerate(neighbors):
                    if neighbor > center:
                        code |= 1 << k
                
                lbp[i, j] = code
        
        # Calculate histogram
        hist, _ = np.histogram(lbp, bins=256, range=(0, 256))
        hist = hist.astype("float")
        hist /= (hist.sum() + 1e-6)
        
        # Calculate entropy (real faces have more complex texture)
        entropy = -np.sum(hist * np.log2(hist + 1e-6))
        
        # Normalize entropy to 0-1 range
        return min(entropy / 8.0, 1.0)
    
    def _compute_frequency_features(self, image: np.ndarray) -> float:
        """Compute frequency domain features to detect screen/print patterns"""
        
        # Apply FFT
        f_transform = np.fft.fft2(image)
        f_shift = np.fft.fftshift(f_transform)
        magnitude_spectrum = np.log(np.abs(f_shift) + 1)
        
        # Look for regular patterns (screens/prints have regular pixel patterns)
        h, w = magnitude_spectrum.shape
        center_h, center_w = h // 2, w // 2
        
        # Sample high-frequency components
        margin = min(20, h//4, w//4)
        high_freq_region = magnitude_spectrum[
            center_h-margin:center_h+margin,
            center_w-margin:center_w+margin
        ]
        
        # Real faces have smoother frequency distribution
        variance = np.var(high_freq_region)
        mean_val = np.mean(high_freq_region)
        
        # Calculate score (lower variance relative to mean = more likely real)
        if mean_val > 0:
            score = 1.0 / (1.0 + variance / mean_val)
        else:
            score = 0.5
        
        return np.clip(score, 0, 1)
    
    def _compute_color_features(self, image: np.ndarray) -> float:
        """Compute color distribution features"""
        
        if len(image.shape) != 3:
            return 0.5
        
        # Convert to HSV for better color analysis
        hsv = cv2.cvtColor(image, cv2.COLOR_RGB2HSV)
        
        # Calculate histograms for each channel
        hist_h = cv2.calcHist([hsv], [0], None, [180], [0, 180])
        hist_s = cv2.calcHist([hsv], [1], None, [256], [0, 256])
        hist_v = cv2.calcHist([hsv], [2], None, [256], [0, 256])
        
        # Normalize histograms
        hist_h = hist_h.flatten() / (hist_h.sum() + 1e-6)
        hist_s = hist_s.flatten() / (hist_s.sum() + 1e-6)
        hist_v = hist_v.flatten() / (hist_v.sum() + 1e-6)
        
        # Calculate entropy for each channel
        entropy_h = -np.sum(hist_h * np.log2(hist_h + 1e-6))
        entropy_s = -np.sum(hist_s * np.log2(hist_s + 1e-6))
        entropy_v = -np.sum(hist_v * np.log2(hist_v + 1e-6))
        
        # Real faces have more varied color distribution
        total_entropy = (entropy_h + entropy_s + entropy_v) / 3
        
        # Normalize to 0-1 range
        return min(total_entropy / 7.0, 1.0)

# ============================================
# Real-Time Selfie Verification System
# ============================================

class RealTimeSelfieVerification:
    """Real-time selfie verification system with RL and Federated Learning"""
    
    def __init__(self, vision_analyzer, rl_agent, federated_manager):
        self.vision_analyzer = vision_analyzer
        self.rl_agent = rl_agent
        self.federated_manager = federated_manager
        
        # Initialize face encoder
        self.device = torch.device('cuda' if torch.cuda.is_available() else 'cpu')
        self.face_encoder = FaceEncoder().to(self.device)
        self.face_encoder.eval()
        
        # Face detection cascade
        try:
            self.face_cascade = cv2.CascadeClassifier(
                cv2.data.haarcascades + 'haarcascade_frontalface_default.xml'
            )
        except:
            logger.warning("OpenCV face cascade not found, using alternative detection")
            self.face_cascade = None
        
        # Verification thresholds
        self.match_threshold = 0.7
        self.liveness_threshold = 0.8
        self.quality_threshold = 0.5
        
        # RL components for verification
        self.verification_history = deque(maxlen=1000)
        self.user_embeddings_cache = {}
        self.cache_ttl = 3600  # 1 hour
        
        # Federated learning components
        self.local_model_updates = {}
        self.verification_count = 0
        
        # Anti-spoofing features
        self.motion_detector = MotionBasedLivenessDetector()
        self.texture_analyzer = TextureBasedSpoofDetector()
        
        # Performance tracking
        self.processing_times = deque(maxlen=100)
    
    def extract_face_region(self, image: np.ndarray) -> Optional[np.ndarray]:
        """Extract face region from image"""
        
        # Try using face_recognition first (more accurate)
        try:
            face_locations = face_recognition.face_locations(image)
            if face_locations:
                top, right, bottom, left = face_locations[0]
                
                # Add padding
                padding = int((bottom - top) * 0.2)
                top = max(0, top - padding)
                left = max(0, left - padding)
                bottom = min(image.shape[0], bottom + padding)
                right = min(image.shape[1], right + padding)
                
                face_region = image[top:bottom, left:right]
                face_region = cv2.resize(face_region, (224, 224))
                return face_region
        except:
            pass
        
        # Fallback to OpenCV cascade
        if self.face_cascade is not None:
            gray = cv2.cvtColor(image, cv2.COLOR_RGB2GRAY)
            faces = self.face_cascade.detectMultiScale(gray, 1.1, 4)
            
            if len(faces) > 0:
                # Get the largest face
                x, y, w, h = max(faces, key=lambda f: f[2] * f[3])
                
                # Add padding
                padding = int(w * 0.2)
                x = max(0, x - padding)
                y = max(0, y - padding)
                w = min(image.shape[1] - x, w + 2 * padding)
                h = min(image.shape[0] - y, h + 2 * padding)
                
                face_region = image[y:y+h, x:x+w]
                face_region = cv2.resize(face_region, (224, 224))
                return face_region
        
        return None
    
    def compute_face_embedding(self, face_image: np.ndarray) -> Dict:
        """Compute face embedding using the face encoder"""
        
        # Prepare image for model
        transform = transforms.Compose([
            transforms.ToPILImage(),
            transforms.Resize((224, 224)),
            transforms.ToTensor(),
            transforms.Normalize(mean=[0.485, 0.456, 0.406],
                               std=[0.229, 0.224, 0.225])
        ])
        
        face_tensor = transform(face_image).unsqueeze(0).to(self.device)
        
        with torch.no_grad():
            output = self.face_encoder(face_tensor)
        
        return {
            'embedding': output['embedding'].cpu().numpy()[0],
            'liveness_score': output['liveness'][0, 0].item(),  # Real class probability
            'quality_score': output['quality'][0, 0].item()
        }
    
    async def verify_realtime_selfie(self, user_id: int, selfie_image: np.ndarray, 
                                    stored_photos: List[np.ndarray]) -> SelfieVerificationResult:
        """
        Verify real-time selfie against stored photos
        This is where RL and Federated Learning come into play
        """
        
        verification_start = time.time()
        failure_reasons = []
        
        # Step 1: Extract face from selfie
        selfie_face = self.extract_face_region(selfie_image)
        if selfie_face is None:
            failure_reasons.append("No face detected in selfie")
            return SelfieVerificationResult(
                is_match=False,
                confidence_score=0.0,
                liveness_score=0.0,
                face_match_score=0.0,
                verification_timestamp=datetime.now(),
                failure_reasons=failure_reasons,
                rl_reward=-1.0,  # Negative reward for RL
                federated_update=False
            )
        
        # Step 2: Compute selfie embedding and liveness
        selfie_data = self.compute_face_embedding(selfie_face)
        
        # Step 3: Check liveness (anti-spoofing)
        liveness_score = selfie_data['liveness_score']
        
        # Additional liveness checks
        motion_liveness = await self.motion_detector.check_liveness(selfie_image)
        texture_liveness = self.texture_analyzer.analyze_texture(selfie_face)
        
        combined_liveness = (liveness_score * 0.5 + 
                            motion_liveness * 0.3 + 
                            texture_liveness * 0.2)
        
        if combined_liveness < self.liveness_threshold:
            failure_reasons.append("Failed liveness detection - possible spoofing attempt")
        
        # Step 4: Check image quality
        if selfie_data['quality_score'] < self.quality_threshold:
            failure_reasons.append("Selfie quality too low")
        
        # Step 5: Compare with stored photos
        stored_embeddings = []
        for photo in stored_photos[:5]:  # Limit to 5 photos
            face = self.extract_face_region(photo)
            if face is not None:
                photo_data = self.compute_face_embedding(face)
                stored_embeddings.append(photo_data['embedding'])
        
        if not stored_embeddings:
            failure_reasons.append("No valid faces in stored photos")
            return SelfieVerificationResult(
                is_match=False,
                confidence_score=0.0,
                liveness_score=combined_liveness,
                face_match_score=0.0,
                verification_timestamp=datetime.now(),
                failure_reasons=failure_reasons,
                rl_reward=-0.5,
                federated_update=False
            )
        
        # Step 6: Calculate similarity scores
        selfie_embedding = selfie_data['embedding']
        similarities = []
        
        for stored_embedding in stored_embeddings:
            # Cosine similarity
            similarity = np.dot(selfie_embedding, stored_embedding) / (
                np.linalg.norm(selfie_embedding) * np.linalg.norm(stored_embedding) + 1e-8
            )
            similarities.append(similarity)
        
        face_match_score = np.max(similarities)
        avg_match_score = np.mean(similarities)
        
        # Step 7: RL-based decision making
        rl_features = np.array([
            face_match_score,
            avg_match_score,
            combined_liveness,
            selfie_data['quality_score'],
            len(stored_embeddings) / 5.0,  # Normalized count
            self.get_user_verification_history(user_id)
        ])
        
        # Get RL agent's recommendation
        rl_decision = self.rl_agent.predict_verification_action(user_id, rl_features)
        
        # Dynamic threshold based on RL learning
        adjusted_threshold = self.match_threshold - rl_decision
        
        is_match = (face_match_score >= adjusted_threshold and 
                   combined_liveness >= self.liveness_threshold and
                   len(failure_reasons) == 0)
        
        # Step 8: Calculate confidence score
        confidence_score = (face_match_score * 0.5 + 
                          avg_match_score * 0.3 + 
                          combined_liveness * 0.2)
        
        # Step 9: Calculate RL reward
        rl_reward = self.calculate_rl_reward(is_match, confidence_score, user_id)
        
        # Step 10: Update RL agent with this experience
        self.update_rl_agent(user_id, rl_features, is_match, rl_reward)
        
        # Step 11: Prepare for federated learning
        should_update_federated = self.should_trigger_federated_update(user_id)
        
        if should_update_federated:
            await self.trigger_federated_learning_update(user_id, selfie_embedding, is_match)
        
        # Track processing time
        processing_time = time.time() - verification_start
        self.processing_times.append(processing_time)
        
        # Store verification result
        result = SelfieVerificationResult(
            is_match=is_match,
            confidence_score=confidence_score,
            liveness_score=combined_liveness,
            face_match_score=face_match_score,
            verification_timestamp=datetime.now(),
            failure_reasons=failure_reasons,
            rl_reward=rl_reward,
            federated_update=should_update_federated
        )
        
        # Track verification history
        self.verification_history.append({
            'user_id': user_id,
            'result': result,
            'timestamp': datetime.now(),
            'processing_time': processing_time
        })
        
        return result
    
    def calculate_rl_reward(self, is_match: bool, confidence: float, user_id: int) -> float:
        """
        Calculate reward for RL agent based on verification outcome
        """
        
        # Get user's historical verification success rate
        user_history = self.get_user_verification_stats(user_id)
        
        if is_match:
            # Positive reward for successful verification
            base_reward = 1.0
            consistency_bonus = user_history['success_rate'] * 0.5
            confidence_bonus = confidence * 0.3
            reward = base_reward + consistency_bonus + confidence_bonus
        else:
            # Negative reward for failed verification
            base_penalty = -0.5
            if user_history['total_attempts'] < 5:
                # Be lenient with new users
                penalty_reduction = 0.2
            else:
                penalty_reduction = 0
            reward = base_penalty + penalty_reduction
        
        return reward
    
    def update_rl_agent(self, user_id: int, features: np.ndarray, 
                       action: bool, reward: float):
        """Update RL agent with verification experience"""
        
        # Create state representation
        state = np.concatenate([
            features,
            [1.0 if action else 0.0]  # Include the action taken
        ])
        
        # Pad state if needed
        if len(state) < 14:  # Expected state size for RL agent
            state = np.pad(state, (0, 14 - len(state)), 'constant')
        
        # Update user-specific model
        if user_id not in self.rl_agent.user_models:
            self.rl_agent.user_models[user_id] = self.rl_agent.q_weights.copy()
        
        # Perform Q-learning update
        self.rl_agent.update_q_values(
            user_id=user_id,
            state=state,
            action=1 if action else 0,
            reward=reward,
            next_state=state  # Simplified for verification
        )
        
        # Trigger batch training periodically
        if len(self.verification_history) % 100 == 0:
            self.rl_agent.train_batch(batch_size=32)
    
    async def trigger_federated_learning_update(self, user_id: int, 
                                               embedding: np.ndarray, 
                                               verification_result: bool):
        """Trigger federated learning update"""
        
        # Prepare local model update
        local_update = {
            'embedding_adjustments': embedding * (1.0 if verification_result else -0.1),
            'verification_count': 1,
            'success': verification_result
        }
        
        # Add differential privacy noise
        noise = np.random.laplace(0, 0.1, embedding.shape)
        local_update['embedding_adjustments'] += noise
        
        # Generate anonymous client ID
        client_id = self.federated_manager.generate_client_id(
            user_id, 
            f"verification_{datetime.now().hour}"
        )
        
        # Submit to federated learning manager
        success = self.federated_manager.submit_client_update(
            user_id=user_id,
            local_weights=self.rl_agent.get_user_model(user_id),
            training_samples=len([h for h in self.verification_history 
                                 if h['user_id'] == user_id]),
            session_info=f"selfie_verification_{datetime.now().date()}"
        )
        
        if success:
            logger.info(f"Federated learning update triggered for user {user_id}")
            
            # Update local model with new global model
            self.rl_agent.q_weights = self.federated_manager.get_global_model()
    
    def should_trigger_federated_update(self, user_id: int) -> bool:
        """Determine if federated learning update should be triggered"""
        
        # Update every 10 verifications per user
        user_verifications = [v for v in self.verification_history 
                            if v['user_id'] == user_id]
        
        return len(user_verifications) % 10 == 0
    
    def get_user_verification_history(self, user_id: int) -> float:
        """Get user's verification history score"""
        user_verifications = [v for v in self.verification_history 
                            if v['user_id'] == user_id]
        
        if not user_verifications:
            return 0.5  # Default score for new users
        
        recent_verifications = user_verifications[-10:]  # Last 10 attempts
        success_rate = sum(1 for v in recent_verifications 
                         if v['result'].is_match) / len(recent_verifications)
        
        return success_rate
    
    def get_user_verification_stats(self, user_id: int) -> Dict:
        """Get detailed verification statistics for a user"""
        user_verifications = [v for v in self.verification_history 
                            if v['user_id'] == user_id]
        
        if not user_verifications:
            return {
                'total_attempts': 0,
                'success_rate': 0.0,
                'avg_confidence': 0.0,
                'avg_liveness': 0.0,
                'avg_processing_time': 0.0
            }
        
        successful = [v for v in user_verifications if v['result'].is_match]
        
        return {
            'total_attempts': len(user_verifications),
            'success_rate': len(successful) / len(user_verifications),
            'avg_confidence': np.mean([v['result'].confidence_score 
                                      for v in user_verifications]),
            'avg_liveness': np.mean([v['result'].liveness_score 
                                    for v in user_verifications]),
            'avg_processing_time': np.mean([v['processing_time'] 
                                          for v in user_verifications]),
            'last_attempt': user_verifications[-1]['timestamp'],
            'rl_model_version': hashlib.md5(
                self.rl_agent.get_user_model(user_id).tobytes()
            ).hexdigest()[:8] if user_id in self.rl_agent.user_models else 'default'
        }
    
    def get_performance_metrics(self) -> Dict:
        """Get system performance metrics"""
        
        if not self.processing_times:
            return {
                'avg_processing_time_ms': 0,
                'total_verifications': 0,
                'cache_size': 0
            }
        
        return {
            'avg_processing_time_ms': np.mean(self.processing_times) * 1000,
            'min_processing_time_ms': np.min(self.processing_times) * 1000,
            'max_processing_time_ms': np.max(self.processing_times) * 1000,
            'total_verifications': len(self.verification_history),
            'cache_size': len(self.user_embeddings_cache),
            'success_rate': sum(1 for v in self.verification_history 
                              if v['result'].is_match) / max(len(self.verification_history), 1)
        }

# Export main components
__all__ = [
    'SelfieVerificationResult',
    'FaceEncoder',
    'MotionBasedLivenessDetector',
    'TextureBasedSpoofDetector',
    'RealTimeSelfieVerification'
]