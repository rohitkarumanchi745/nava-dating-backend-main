"""
LinUCB contextual bandit (sync) for ranking spots/users.
Stores arm state in bandit_arm_stats table.
"""
from typing import Dict, Tuple, Optional, List
import numpy as np
from sqlalchemy.orm import Session
from models import BanditArmStat


class LinUCBConfig:
    FEATURE_DIM = 16
    ALPHA = 0.6
    LAMBDA_REG = 1.0
    MIN_PULLS_FOR_UCB = 5
    OBSERVATION_DECAY = 0.995


class LinUCB:
    def __init__(self, feature_dim: int = None, alpha: float = None):
        self.cfg = LinUCBConfig()
        self.d = feature_dim or self.cfg.FEATURE_DIM
        self.alpha = alpha or self.cfg.ALPHA

    # ---------- DB helpers ----------
    def _get_state(self, db: Optional[Session], arm_id: str, arm_type: str, user_id: Optional[int]) -> Tuple[np.ndarray, np.ndarray, np.ndarray, int, float]:
        if db is None:
            # Stateless path: return cold-start state
            A = self.cfg.LAMBDA_REG * np.eye(self.d)
            b = np.zeros(self.d)
            theta = np.zeros(self.d)
            return A, b, theta, 0, 0.0
        row = (
            db.query(BanditArmStat)
            .filter(
                BanditArmStat.arm_id == arm_id,
                BanditArmStat.arm_type == arm_type,
                BanditArmStat.user_id == user_id,
            )
            .first()
        )
        if not row:
            A = self.cfg.LAMBDA_REG * np.eye(self.d)
            b = np.zeros(self.d)
            theta = np.zeros(self.d)
            return A, b, theta, 0, 0.0
        A = np.array(row.a_matrix or self.cfg.LAMBDA_REG * np.eye(self.d)).reshape(self.d, self.d)
        b = np.array(row.b_vector or np.zeros(self.d))
        theta = np.array(row.theta_vector or np.zeros(self.d))
        return A, b, theta, row.num_pulls or 0, row.total_reward or 0.0

    def _save_state(self, db: Session, arm_id: str, arm_type: str, user_id: Optional[int], A: np.ndarray, b: np.ndarray, theta: np.ndarray, num_pulls: int, total_reward: float):
        row = (
            db.query(BanditArmStat)
            .filter(
                BanditArmStat.arm_id == arm_id,
                BanditArmStat.arm_type == arm_type,
                BanditArmStat.user_id == user_id,
            )
            .first()
        )
        if not row:
            row = BanditArmStat(
                arm_id=arm_id,
                arm_type=arm_type,
                user_id=user_id,
            )
            db.add(row)
        row.a_matrix = A.flatten().tolist()
        row.b_vector = b.tolist()
        row.theta_vector = theta.tolist()
        row.num_pulls = num_pulls
        row.total_reward = total_reward
        db.commit()

    # ---------- core math ----------
    def _init_state(self):
        A = self.cfg.LAMBDA_REG * np.eye(self.d)
        b = np.zeros(self.d)
        theta = np.zeros(self.d)
        return A, b, theta

    def _compute(self, theta: np.ndarray, A_inv: np.ndarray, x: np.ndarray, alpha: float = None) -> Tuple[float, float]:
        alpha = alpha or self.alpha
        pred = float(np.dot(theta, x))
        bonus = alpha * np.sqrt(float(x @ A_inv @ x))
        return pred, bonus

    def _update(self, A: np.ndarray, b: np.ndarray, x: np.ndarray, r: float) -> Tuple[np.ndarray, np.ndarray, np.ndarray]:
        A = self.cfg.OBSERVATION_DECAY * A + (1 - self.cfg.OBSERVATION_DECAY) * self.cfg.LAMBDA_REG * np.eye(self.d)
        b = self.cfg.OBSERVATION_DECAY * b
        A = A + np.outer(x, x)
        b = b + r * x
        try:
            theta = np.linalg.inv(A) @ b
        except np.linalg.LinAlgError:
            theta = np.linalg.lstsq(A, b, rcond=None)[0]
        return A, b, theta

    # ---------- public API ----------
    def score_candidates(self, db: Optional[Session], candidates: List[Dict], context_features: Dict, user_id: Optional[int] = None, exploration_rate: float = 0.1) -> List[Dict]:
        scored = []
        for cand in candidates:
            x = self._build_context(cand, context_features)
            arm_id = self._arm_id(cand)
            A, b, theta, pulls, _ = self._get_state(db, arm_id, "global", user_id)
            try:
                A_inv = np.linalg.inv(A)
            except np.linalg.LinAlgError:
                A_inv = np.eye(self.d) / self.cfg.LAMBDA_REG
            if pulls < self.cfg.MIN_PULLS_FOR_UCB:
                pred, bonus = self._compute(theta, A_inv, x, alpha=self.alpha * 2)
            else:
                pred, bonus = self._compute(theta, A_inv, x)
            if np.random.rand() < exploration_rate:
                bonus += np.random.exponential(0.3)
            scored.append({**cand, "bandit_predicted": pred, "bandit_bonus": bonus, "bandit_score": pred + bonus, "bandit_arm_id": arm_id, "bandit_pulls": pulls})
        return scored

    def update_reward(self, db: Session, arm_id: str, context: np.ndarray, reward: float, user_id: Optional[int] = None):
        A, b, theta, pulls, total = self._get_state(db, arm_id, "global", user_id)
        A, b, theta = self._update(A, b, context, reward)
        pulls += 1
        total += reward
        self._save_state(db, arm_id, "global", user_id, A, b, theta, pulls, total)

    # ---------- helpers ----------
    def _build_context(self, cand: Dict, ctx: Dict) -> np.ndarray:
        feats = []
        feats.append(cand.get("similarity_score", 0.0))
        feats.append(1.0 if cand.get("creator_verified") else 0.0)
        feats.append(cand.get("quality_score", 0.5))
        feats.append(cand.get("engagement_rate", 0.0))
        feats.append(cand.get("trend_velocity", 0.0))
        feats.append(cand.get("freshness", 0.5))
        feats.append(cand.get("distance_score", 0.5))
        feats.append(ctx.get("hour_of_day", 12) / 24)
        feats.append(ctx.get("day_of_week", 3) / 7)
        feats.append(1.0 if ctx.get("is_weekend") else 0.0)
        x = np.array(feats, dtype=np.float32)
        if len(x) < self.d:
            x = np.pad(x, (0, self.d - len(x)))
        elif len(x) > self.d:
            x = x[: self.d]
        return x

    def _arm_id(self, cand: Dict) -> str:
        # arm by creator
        creator = cand.get("creator_id") or "unknown"
        return f"creator:{creator}"


# Singleton
_bandit = LinUCB()

def get_bandit() -> LinUCB:
    return _bandit
