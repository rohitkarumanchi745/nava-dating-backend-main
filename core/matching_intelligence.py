"""
Core Matching Intelligence Module
Implements Reinforcement Learning and Federated Learning for smart matching
"""

import numpy as np
import json
import pickle
from typing import Dict, List, Tuple, Optional
from datetime import datetime, timedelta
from dataclasses import dataclass
from collections import defaultdict, deque
import logging
import os
import hashlib
import threading
from abc import ABC, abstractmethod
import random

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

# ============================================
# Data Classes
# ============================================

@dataclass
class UserFeatures:
    """User feature representation for ML models"""
    age: float
    gender_encoded: float  # 0: male, 1: female, 0.5: non-binary/other
    activity_score: float  # Based on app usage
    selectivity_score: float  # How selective the user is (like ratio)
    bio_sentiment: float  # Sentiment analysis of bio (-1 to 1)
    photo_attractiveness: float  # Placeholder for photo analysis
    location_cluster: int  # Geographic cluster ID
    
    def to_vector(self) -> np.ndarray:
        """Convert to numpy array for ML processing"""
        return np.array([
            self.age / 100.0,  # Normalize age
            self.gender_encoded,
            self.activity_score,
            self.selectivity_score,
            (self.bio_sentiment + 1) / 2,  # Normalize to 0-1
            self.photo_attractiveness,
            self.location_cluster / 10.0  # Normalize assuming max 10 clusters
        ])

@dataclass
class SwipeAction:
    """Represents a swipe action for RL training"""
    user_id: int
    target_user_id: int
    action: int  # 0: pass, 1: like
    timestamp: datetime
    features: UserFeatures
    target_features: UserFeatures
    reward: float = 0.0  # Will be updated based on match outcome

# ============================================
# Reinforcement Learning Agent
# ============================================

class ReinforcementLearningAgent:
    """Q-Learning based recommendation agent for profile swiping"""
    
    def __init__(self, state_size: int = 14, action_size: int = 2, learning_rate: float = 0.01):
        self.state_size = state_size  # Combined user + target features
        self.action_size = action_size  # 0: pass, 1: like
        self.learning_rate = learning_rate
        self.epsilon = 0.3  # Exploration rate
        self.epsilon_decay = 0.995
        self.epsilon_min = 0.01
        self.gamma = 0.95  # Discount factor
        
        # Q-table approximation using neural network weights
        self.q_weights = np.random.normal(0, 0.1, (state_size, action_size))
        self.experience_replay = deque(maxlen=10000)
        
        # User-specific models
        self.user_models: Dict[int, np.ndarray] = {}
        self.user_stats: Dict[int, Dict] = defaultdict(lambda: {
            'total_swipes': 0,
            'likes_given': 0,
            'matches_received': 0,
            'response_rate': 0.0
        })
        self.user_models_path = "user_models.pkl"
        self.load_user_models()
    
    def get_state(self, user_features: UserFeatures, target_features: UserFeatures) -> np.ndarray:
        """Combine user and target features into state vector"""
        user_vec = user_features.to_vector()
        target_vec = target_features.to_vector()
        return np.concatenate([user_vec, target_vec])
    
    def get_user_model(self, user_id: int) -> np.ndarray:
        """Get or create user-specific model weights"""
        if user_id not in self.user_models:
            # Initialize with global model + small random variation
            self.user_models[user_id] = self.q_weights + np.random.normal(0, 0.05, self.q_weights.shape)
            self.save_user_models()
        return self.user_models[user_id]
    
    def predict_action(self, user_id: int, user_features: UserFeatures, target_features: UserFeatures) -> int:
        """Predict best action (0: pass, 1: like) using epsilon-greedy policy"""
        state = self.get_state(user_features, target_features)
        user_weights = self.get_user_model(user_id)
        
        if np.random.random() <= self.epsilon:
            return np.random.randint(self.action_size)  # Random action (exploration)
        
        q_values = np.dot(state, user_weights)
        return np.argmax(q_values)  # Best action (exploitation)
    
    def get_recommendation_score(self, user_id: int, user_features: UserFeatures, target_features: UserFeatures) -> float:
        """Get recommendation confidence score (0-1)"""
        state = self.get_state(user_features, target_features)
        user_weights = self.get_user_model(user_id)
        q_values = np.dot(state, user_weights)
        
        # Convert to probability using softmax
        exp_values = np.exp(q_values - np.max(q_values))
        probabilities = exp_values / np.sum(exp_values)
        
        return probabilities[1]  # Probability of like action
    
    def predict_verification_action(self, user_id: int, features: np.ndarray) -> float:
        """Predict verification decision (for selfie verification)"""
        user_weights = self.get_user_model(user_id)
        
        # Pad features if needed
        if len(features) < self.state_size:
            features = np.pad(features, (0, self.state_size - len(features)), 'constant')
        
        q_values = np.dot(features, user_weights)
        
        # Return adjustment factor for verification threshold
        return (q_values[1] - q_values[0]) * 0.1  # Scale to reasonable threshold adjustment
    
    def update_q_values(self, user_id: int, state: np.ndarray, action: int, reward: float, next_state: np.ndarray):
        """Update Q-values using Q-learning algorithm"""
        user_weights = self.get_user_model(user_id)
        
        current_q = np.dot(state, user_weights)[action]
        next_q_max = np.max(np.dot(next_state, user_weights))
        target_q = reward + self.gamma * next_q_max
        
        # Update weights using gradient descent
        error = target_q - current_q
        gradient = error * state
        user_weights[:, action] += self.learning_rate * gradient
        
        # Update global model with weighted average
        alpha = 0.1  # Global model update rate
        self.q_weights = (1 - alpha) * self.q_weights + alpha * user_weights
        self.save_user_models()
    
    def record_swipe(self, swipe_action: SwipeAction):
        """Record swipe action for later training"""
        self.experience_replay.append(swipe_action)
        
        # Update user statistics
        user_id = swipe_action.user_id
        self.user_stats[user_id]['total_swipes'] += 1
        if swipe_action.action == 1:
            self.user_stats[user_id]['likes_given'] += 1
    
    def update_rewards(self, user_id: int, target_user_id: int, match_occurred: bool, response_received: bool = False):
        """Update rewards for past actions based on outcomes"""
        reward = 0.0
        
        if match_occurred:
            reward += 5.0  # High reward for successful match
            self.user_stats[user_id]['matches_received'] += 1
        
        if response_received:
            reward += 2.0  # Additional reward for message response
            
        # Find and update relevant experiences
        for experience in reversed(self.experience_replay):
            if (experience.user_id == user_id and 
                experience.target_user_id == target_user_id and
                experience.action == 1):  # Was a like action
                
                experience.reward = reward
                
                # Perform Q-learning update
                state = self.get_state(experience.features, experience.target_features)
                # Simplified next_state (could be improved)
                next_state = state  
                self.update_q_values(user_id, state, experience.action, reward, next_state)
                break
    
    def calculate_reward(self, match_occurred: bool) -> float:
        """Calculate base reward for an action"""
        if match_occurred:
            return 5.0
        return -0.5
    
    def train_batch(self, batch_size: int = 32):
        """Train on a batch of experiences"""
        if len(self.experience_replay) < batch_size:
            return
            
        batch = np.random.choice(list(self.experience_replay), batch_size, replace=False)
        
        for experience in batch:
            if experience.reward != 0:  # Only train on experiences with rewards
                state = self.get_state(experience.features, experience.target_features)
                next_state = state  # Simplified
                self.update_q_values(experience.user_id, state, experience.action, experience.reward, next_state)
        
        # Decay epsilon
        if self.epsilon > self.epsilon_min:
            self.epsilon *= self.epsilon_decay
    
    def get_user_insights(self, user_id: int) -> Dict:
        """Get insights about user's swiping behavior"""
        stats = self.user_stats[user_id]
        selectivity = 1 - (stats['likes_given'] / max(stats['total_swipes'], 1))
        
        return {
            'total_swipes': stats['total_swipes'],
            'selectivity_score': selectivity,
            'match_rate': stats['matches_received'] / max(stats['likes_given'], 1),
            'epsilon': self.epsilon,
            'model_maturity': min(stats['total_swipes'] / 100, 1.0)
        }

    # ----------------------------
    # Persistence helpers
    # ----------------------------
    def load_user_models(self):
        """Load user-specific models from disk if available"""
        try:
            if os.path.exists(self.user_models_path):
                with open(self.user_models_path, "rb") as f:
                    self.user_models = pickle.load(f)
        except Exception as e:
            logger.error(f"Failed to load user models: {e}")
    
    def save_user_models(self):
        """Persist user-specific models to disk"""
        try:
            with open(self.user_models_path, "wb") as f:
                pickle.dump(self.user_models, f)
        except Exception as e:
            logger.error(f"Failed to save user models: {e}")

# ============================================
# Federated Learning Manager
# ============================================

class FederatedLearningManager:
    """Manages federated learning across users while preserving privacy"""
    
    def __init__(self, global_model_path: str = "global_model.pkl"):
        self.global_model_path = global_model_path
        self.client_updates: Dict[str, Dict] = {}
        self.aggregation_threshold = int(os.getenv("FL_AGGREGATION_THRESHOLD", "3"))  # Lower for dev/small cohorts
        self.privacy_budget = 1.0  # Differential privacy budget
        self.lock = threading.Lock()
        self.aggregation_count = 0
        
        # Load or initialize global model
        self.global_weights = self.load_global_model()
    
    def load_global_model(self) -> np.ndarray:
        """Load global model weights from disk"""
        try:
            if os.path.exists(self.global_model_path):
                with open(self.global_model_path, 'rb') as f:
                    return pickle.load(f)
        except Exception as e:
            logger.error(f"Error loading global model: {e}")
        
        # Return default initialized weights
        return np.random.normal(0, 0.1, (14, 2))
    
    def save_global_model(self):
        """Save global model weights to disk"""
        try:
            with open(self.global_model_path, 'wb') as f:
                pickle.dump(self.global_weights, f)
        except Exception as e:
            logger.error(f"Error saving global model: {e}")
    
    def generate_client_id(self, user_id: int, session_info: str) -> str:
        """Generate anonymous client ID for privacy"""
        # Use hashing to create anonymous but consistent client ID
        client_data = f"{user_id}_{session_info}_{datetime.now().date()}"
        return hashlib.sha256(client_data.encode()).hexdigest()[:16]
    
    def add_noise_for_privacy(self, weights: np.ndarray, noise_scale: float = 0.1) -> np.ndarray:
        """Add differential privacy noise to model weights"""
        if self.privacy_budget <= 0:
            return weights
            
        noise = np.random.laplace(0, noise_scale, weights.shape)
        self.privacy_budget -= 0.1  # Consume privacy budget
        
        return weights + noise
    
    def submit_client_update(self, user_id: int, local_weights: np.ndarray, 
                           training_samples: int, session_info: str = "default") -> bool:
        """Submit local model update from a client"""
        client_id = self.generate_client_id(user_id, session_info)
        
        with self.lock:
            # Add privacy noise
            noisy_weights = self.add_noise_for_privacy(local_weights)
            
            # Store client update
            self.client_updates[client_id] = {
                'weights': noisy_weights,
                'samples': training_samples,
                'timestamp': datetime.now(),
                'user_id_hash': hashlib.sha256(str(user_id).encode()).hexdigest()[:8]  # Anonymous user reference
            }
            
            logger.info(f"Received update from client {client_id} with {training_samples} samples")
            
            # Check if we can perform aggregation
            if len(self.client_updates) >= self.aggregation_threshold:
                self.federated_averaging()
                return True
                
        return False
    
    def federated_averaging(self):
        """Perform federated averaging of client models"""
        if len(self.client_updates) == 0:
            return
            
        logger.info(f"Starting federated averaging with {len(self.client_updates)} clients")
        
        # Weight updates by number of training samples
        total_samples = sum(update['samples'] for update in self.client_updates.values())
        weighted_weights = np.zeros_like(self.global_weights)
        
        for client_id, update in self.client_updates.items():
            weight = update['samples'] / total_samples
            weighted_weights += weight * update['weights']
        
        # Update global model
        learning_rate = 0.1  # Global learning rate
        self.global_weights = (1 - learning_rate) * self.global_weights + learning_rate * weighted_weights
        
        # Save updated global model
        self.save_global_model()
        
        # Track aggregation count
        self.aggregation_count += 1
        
        # Clear client updates
        self.client_updates.clear()
        
        logger.info("Federated averaging completed and global model updated")
    
    def get_global_model(self) -> np.ndarray:
        """Get current global model weights"""
        return self.global_weights.copy()
    
    def get_federated_stats(self) -> Dict:
        """Get statistics about federated learning process"""
        return {
            'active_clients': len(self.client_updates),
            'aggregation_threshold': self.aggregation_threshold,
            'privacy_budget_remaining': self.privacy_budget,
            'aggregation_cycles': self.aggregation_count,
            'last_aggregation': getattr(self, 'last_aggregation_time', 'Never'),
            'global_model_version': hashlib.md5(self.global_weights.tobytes()).hexdigest()[:8]
        }

# ============================================
# User Feature Extraction
# ============================================

class UserFeatureExtractor:
    """Extracts features from user data for ML models"""
    
    def __init__(self):
        self.cache: Dict[int, UserFeatures] = {}
        self.cache_timeout = timedelta(minutes=30)
        self.last_cache_clear = datetime.now()
        self.extraction_count = 0
    
    def extract_user_features(self, user_id: int, force_refresh: bool = False) -> Optional[UserFeatures]:
        """Extract features for a user (with caching)"""
        # Clear cache periodically
        if datetime.now() - self.last_cache_clear > self.cache_timeout:
            self.cache.clear()
            self.last_cache_clear = datetime.now()
        
        # Return cached features if available
        if user_id in self.cache and not force_refresh:
            return self.cache[user_id]
        
        try:
            # This would integrate with your database
            # For now, returning mock features
            features = self._extract_features_from_db(user_id)
            if features:
                self.cache[user_id] = features
                self.extraction_count += 1
            return features
        except Exception as e:
            logger.error(f"Error extracting features for user {user_id}: {e}")
            return None
    
    def _extract_features_from_db(self, user_id: int) -> Optional[UserFeatures]:
        """Extract features from database (to be integrated with your DB)"""
        # This is a placeholder - you'll need to integrate with your actual database
        # For demonstration, returning mock features
        
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
        """Get feature extraction statistics"""
        return {
            "cached_users": len(self.cache),
            "total_extractions": self.extraction_count,
            "cache_hit_rate": 1 - (self.extraction_count / max(len(self.cache) * 2, 1))
        }

# ============================================
# Main Matching Intelligence System
# ============================================

class MatchingIntelligence:
    """Main class that combines RL agent and federated learning for smart matching"""
    
    def __init__(self):
        self.rl_agent = ReinforcementLearningAgent()
        self.federated_manager = FederatedLearningManager()
        self.feature_extractor = UserFeatureExtractor()
        
        # Load global model into RL agent
        self.rl_agent.q_weights = self.federated_manager.get_global_model()
        
        # Periodic tasks
        self.last_training_time = datetime.now()
        self.training_interval = timedelta(hours=1)
        
        # Initialize vision analyzer as None (will be set by main app)
        self.vision_analyzer = None
    
    def get_smart_recommendations(self, user_id: int, potential_matches: List[Dict], 
                                limit: int = 10) -> List[Dict]:
        """Get AI-powered profile recommendations"""
        user_features = self.feature_extractor.extract_user_features(user_id)
        if not user_features:
            return potential_matches[:limit]  # Fallback to original ordering
        
        scored_matches = []
        
        for match in potential_matches:
            target_features = self.feature_extractor.extract_user_features(match['id'])
            
            ai_score = 0.0
            if target_features:
                # Get AI recommendation score
                ai_score = self.rl_agent.get_recommendation_score(user_id, user_features, target_features)
            match['ai_score'] = ai_score
            
            # Heuristic boosts to align with frontend needs
            # Prefer verified, profile complete, and photo-rich users
            photos = match.get('photos') or []
            has_photos = len(photos) > 0
            verified_boost = 0.1 if match.get('is_verified') else 0.0
            photo_boost = min(len(photos), 3) * 0.05  # up to +0.15
            bio_boost = 0.05 if match.get('bio') else 0.0
            profile_complete_boost = 0.05 if match.get('age') and has_photos else 0.0
            
            heuristic_score = verified_boost + photo_boost + bio_boost + profile_complete_boost
            
            # Blend AI and heuristics; clamp to [0,1]
            blended = min(1.0, 0.65 * ai_score + 0.35 * heuristic_score)
            match['ai_score'] = blended
            scored_matches.append((blended, match))
        
        # Sort by blended score (highest first)
        scored_matches.sort(key=lambda x: x[0], reverse=True)
        
        # Diversity injection: keep top 70%, fill remainder with random picks to avoid echo chambers
        top_count = max(1, int(limit * 0.7))
        core = scored_matches[:top_count]
        remainder = scored_matches[top_count:]
        inject = []
        if remainder:
            inject = random.sample(remainder, k=min(len(remainder), max(0, limit - top_count)))
        
        blended_list = core + inject
        random.shuffle(blended_list)
        
        # Return top recommendations
        return [match for _, match in blended_list[:limit]]
    
    def record_swipe_action(self, user_id: int, target_user_id: int, action: str) -> Dict:
        """Record a swipe action for learning"""
        user_features = self.feature_extractor.extract_user_features(user_id)
        target_features = self.feature_extractor.extract_user_features(target_user_id)
        
        if not user_features or not target_features:
            return {"status": "error", "message": "Could not extract features"}
        
        swipe_action = SwipeAction(
            user_id=user_id,
            target_user_id=target_user_id,
            action=1 if action.lower() == 'like' else 0,
            timestamp=datetime.now(),
            features=user_features,
            target_features=target_features
        )
        
        self.rl_agent.record_swipe(swipe_action)
        
        # Periodic training
        if datetime.now() - self.last_training_time > self.training_interval:
            self.trigger_learning_update(user_id)
            self.last_training_time = datetime.now()
        
        return {
            "status": "success", 
            "message": "Swipe recorded for learning",
            "user_insights": self.rl_agent.get_user_insights(user_id)
        }
    
    def update_match_outcome(self, user_id: int, target_user_id: int, 
                           match_occurred: bool, response_received: bool = False):
        """Update learning models based on match outcomes"""
        self.rl_agent.update_rewards(user_id, target_user_id, match_occurred, response_received)
        
        # Trigger federated learning update for significant events
        if match_occurred:
            self.trigger_learning_update(user_id)
    
    def trigger_learning_update(self, user_id: int):
        """Trigger federated learning update"""
        # Train RL agent
        self.rl_agent.train_batch()
        
        # Submit update to federated learning
        user_weights = self.rl_agent.get_user_model(user_id)
        training_samples = self.rl_agent.user_stats[user_id]['total_swipes']
        
        if training_samples > 10:  # Only submit if user has enough training data
            session_info = f"session_{datetime.now().hour}"
            updated = self.federated_manager.submit_client_update(
                user_id, user_weights, training_samples, session_info
            )
            
            if updated:
                # Update local model with new global model
                self.rl_agent.q_weights = self.federated_manager.get_global_model()
                logger.info(f"Updated global model for user {user_id}")
    
    def get_system_stats(self) -> Dict:
        """Get comprehensive system statistics"""
        return {
            "reinforcement_learning": {
                "total_experiences": len(self.rl_agent.experience_replay),
                "epsilon": self.rl_agent.epsilon,
                "active_user_models": len(self.rl_agent.user_models)
            },
            "federated_learning": self.federated_manager.get_federated_stats(),
            "feature_extraction": self.feature_extractor.get_stats()
        }

# Global instance
matching_intelligence = MatchingIntelligence()

# Export main components
__all__ = [
    'UserFeatures',
    'SwipeAction',
    'ReinforcementLearningAgent',
    'FederatedLearningManager',
    'UserFeatureExtractor',
    'MatchingIntelligence',
    'matching_intelligence'
]
