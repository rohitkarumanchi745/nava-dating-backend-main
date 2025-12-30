"""
Recommendation-system models aligned to the pgvector schema in db/schema_reco.sql.
These do NOT replace existing models; they live side-by-side so we can migrate
code incrementally. Uses a separate BaseReco to avoid collisions with legacy tables.
"""
from datetime import datetime
from uuid import uuid4
from sqlalchemy import (
    Column, String, Boolean, DateTime, Integer, Float, Text, ForeignKey, JSON,
)
from sqlalchemy.dialects.postgresql import UUID, ARRAY
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy import Enum as PgEnum
from sqlalchemy.orm import relationship

# Fallback Vector type: stores as array if pgvector dialect is not installed.
try:
    from pgvector.sqlalchemy import Vector  # type: ignore
except Exception:  # pragma: no cover
    class Vector(ARRAY):
        def __init__(self, dim):
            super().__init__(Float, dimensions=1)
            self.dim = dim

BaseReco = declarative_base()

# Enums
GENDER = PgEnum('male', 'female', 'non_binary', 'other', 'prefer_not_to_say', name='gender_type')
INTENT = PgEnum('casual', 'dating', 'friendship', 'networking', 'open', name='intent_type')
INTERACTION = PgEnum(
    'impression', 'view', 'like', 'save', 'share', 'comment', 'skip', 'hide',
    'not_interested', 'request_profile', 'match', 'message', name='interaction_type'
)
SURFACE = PgEnum('discover', 'playground', 'search', 'profile', 'notification', name='surface_type')
CONTENT = PgEnum('image', 'video', 'reel', 'story', name='content_type')


class UserReco(BaseReco):
    __tablename__ = 'users'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    username = Column(String(50), unique=True, nullable=False)
    email = Column(String(255), unique=True, nullable=False)
    hashed_password = Column(String(255), nullable=False)
    display_name = Column(String(100))
    bio = Column(Text)
    avatar_url = Column(String(500))
    birth_date = Column(DateTime)
    gender = Column(GENDER)
    is_verified = Column(Boolean, default=False)
    is_active = Column(Boolean, default=True)
    is_premium = Column(Boolean, default=False)
    latitude = Column(Float)
    longitude = Column(Float)
    city = Column(String(100))
    country = Column(String(100))
    follower_count = Column(Integer, default=0)
    following_count = Column(Integer, default=0)
    total_likes_received = Column(Integer, default=0)
    engagement_rate = Column(Float, default=0.0)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    last_active_at = Column(DateTime)


class UserPreferencesReco(BaseReco):
    __tablename__ = 'user_preferences'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey('users.id', ondelete='CASCADE'), unique=True, nullable=False)
    min_age = Column(Integer, default=18)
    max_age = Column(Integer, default=99)
    preferred_genders = Column(ARRAY(String))
    preferred_intents = Column(ARRAY(String))
    preferred_languages = Column(ARRAY(String))
    verified_only = Column(Boolean, default=False)
    max_distance_km = Column(Integer, default=100)
    show_nsfw = Column(Boolean, default=False)
    push_enabled = Column(Boolean, default=True)
    email_enabled = Column(Boolean, default=True)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class UserFeaturesReco(BaseReco):
    __tablename__ = 'user_features'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey('users.id', ondelete='CASCADE'), unique=True, nullable=False)
    embedding_vec = Column(Vector(512))
    short_term_embedding = Column(Vector(512))
    long_term_embedding = Column(Vector(512))
    embedding_json = Column(JSON)
    avg_session_duration_sec = Column(Float, default=0)
    avg_interactions_per_session = Column(Float, default=0)
    like_rate = Column(Float, default=0)
    save_rate = Column(Float, default=0)
    share_rate = Column(Float, default=0)
    skip_rate = Column(Float, default=0)
    category_affinities = Column(JSON)
    creator_affinities = Column(JSON)
    active_hours = Column(JSON)
    active_days = Column(JSON)
    last_interaction_at = Column(DateTime)
    embedding_updated_at = Column(DateTime)
    features_updated_at = Column(DateTime, default=datetime.utcnow)


class SpotReco(BaseReco):
    __tablename__ = 'spots'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    creator_id = Column(UUID(as_uuid=True), ForeignKey('users.id', ondelete='CASCADE'), nullable=False)
    content_type = Column(CONTENT, default='image', nullable=False)
    caption = Column(Text)
    hashtags = Column(ARRAY(String))
    media_url = Column(String(500), nullable=False)
    thumbnail_url = Column(String(500))
    duration_sec = Column(Float)
    audio_id = Column(UUID(as_uuid=True))
    latitude = Column(Float)
    longitude = Column(Float)
    location_name = Column(String(200))
    view_count = Column(Integer, default=0)
    like_count = Column(Integer, default=0)
    save_count = Column(Integer, default=0)
    share_count = Column(Integer, default=0)
    comment_count = Column(Integer, default=0)
    engagement_rate = Column(Float, default=0)
    like_rate = Column(Float, default=0)
    quality_score = Column(Float, default=0.5)
    is_trending = Column(Boolean, default=False)
    trend_velocity = Column(Float, default=0)
    is_active = Column(Boolean, default=True)
    is_nsfw = Column(Boolean, default=False)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class SpotEmbeddingReco(BaseReco):
    __tablename__ = 'spot_embeddings'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    spot_id = Column(UUID(as_uuid=True), ForeignKey('spots.id', ondelete='CASCADE'), unique=True, nullable=False)
    visual_embedding = Column(Vector(512))
    text_embedding = Column(Vector(384))
    audio_embedding = Column(Vector(256))
    fused_embedding = Column(Vector(512))
    embedding_json = Column(JSON)
    visual_model_version = Column(String(50))
    text_model_version = Column(String(50))
    audio_model_version = Column(String(50))
    visual_quality_score = Column(Float)
    text_sentiment_score = Column(Float)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class InteractionEventReco(BaseReco):
    __tablename__ = 'interaction_events'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey('users.id', ondelete='CASCADE'), nullable=False)
    spot_id = Column(UUID(as_uuid=True), ForeignKey('spots.id', ondelete='CASCADE'), nullable=False)
    interaction_type = Column(INTERACTION, nullable=False)
    surface = Column(SURFACE, nullable=False)
    slate_id = Column(UUID(as_uuid=True))
    position_in_slate = Column(Integer)
    time_spent_ms = Column(Integer)
    scroll_depth = Column(Float)
    reward = Column(Float, default=0)
    context_json = Column(JSON, default={})
    created_at = Column(DateTime, default=datetime.utcnow)


class ImpressionReco(BaseReco):
    __tablename__ = 'impressions'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    user_id = Column(UUID(as_uuid=True), nullable=False)
    spot_id = Column(UUID(as_uuid=True), nullable=False)
    slate_id = Column(UUID(as_uuid=True), nullable=False)
    surface = Column(SURFACE, nullable=False)
    position = Column(Integer, nullable=False)
    predicted_score = Column(Float)
    exploration_bonus = Column(Float, default=0)
    final_score = Column(Float)
    model_version = Column(String(50))
    created_at = Column(DateTime, default=datetime.utcnow)


class BanditArmStatReco(BaseReco):
    __tablename__ = 'bandit_arm_stats'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    arm_id = Column(String(100), nullable=False)
    arm_type = Column(String(50), nullable=False)
    user_id = Column(UUID(as_uuid=True), ForeignKey('users.id', ondelete='CASCADE'))
    a_matrix = Column(ARRAY(Float), nullable=False)
    b_vector = Column(ARRAY(Float), nullable=False)
    theta_vector = Column(ARRAY(Float))
    num_pulls = Column(Integer, default=0)
    total_reward = Column(Float, default=0)
    avg_reward = Column(Float, default=0)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class CreatorFeaturesReco(BaseReco):
    __tablename__ = 'creator_features'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    user_id = Column(UUID(as_uuid=True), ForeignKey('users.id', ondelete='CASCADE'), unique=True, nullable=False)
    avg_engagement_rate = Column(Float, default=0)
    content_quality_score = Column(Float, default=0.5)
    consistency_score = Column(Float, default=0.5)
    follower_growth_rate = Column(Float, default=0)
    engagement_trend = Column(Float, default=0)
    primary_categories = Column(ARRAY(String))
    posting_frequency_per_week = Column(Float, default=0)
    avg_content_length_sec = Column(Float)
    verified_score = Column(Float, default=0)
    account_age_days = Column(Integer, default=0)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class TrendingSpotReco(BaseReco):
    __tablename__ = 'trending_spots'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    spot_id = Column(UUID(as_uuid=True), ForeignKey('spots.id', ondelete='CASCADE'), nullable=False)
    trend_score = Column(Float, nullable=False)
    velocity = Column(Float, nullable=False)
    category = Column(String(50))
    window_start = Column(DateTime, nullable=False)
    window_end = Column(DateTime, nullable=False)
    created_at = Column(DateTime, default=datetime.utcnow)

# ---------------------------------------------------------------------------
# Minimal stubs for Match/Message/SpotMessage to keep app startup working.
# These are intentionally slim and use UUIDs; legacy functionality is not ported.
# ---------------------------------------------------------------------------


class MatchReco(BaseReco):
    __tablename__ = 'matches'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    user1_id = Column(UUID(as_uuid=True), nullable=False)
    user2_id = Column(UUID(as_uuid=True), nullable=False)
    user1_liked = Column(Boolean)
    user2_liked = Column(Boolean)
    is_mutual_match = Column(Boolean, default=False)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class MessageReco(BaseReco):
    __tablename__ = 'messages'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    match_id = Column(UUID(as_uuid=True), nullable=False)
    sender_id = Column(UUID(as_uuid=True), nullable=False)
    receiver_id = Column(UUID(as_uuid=True), nullable=False)
    content = Column(Text, nullable=False)
    message_type = Column(String(20), default="text")
    is_read = Column(Boolean, default=False)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)


class SpotMessageReco(BaseReco):
    __tablename__ = 'spot_messages'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    spot_id = Column(UUID(as_uuid=True), nullable=False)
    sender_id = Column(UUID(as_uuid=True), nullable=False)
    text = Column(Text, nullable=False)
    created_at = Column(DateTime, default=datetime.utcnow)


class SpotPairStatusReco(BaseReco):
    __tablename__ = 'spot_pair_status'
    id = Column(UUID(as_uuid=True), primary_key=True, default=uuid4)
    spot_id = Column(UUID(as_uuid=True), nullable=False)
    user_a = Column(UUID(as_uuid=True), nullable=False)
    user_b = Column(UUID(as_uuid=True), nullable=False)
    a_count = Column(Integer, default=0)
    b_count = Column(Integer, default=0)
    request_status = Column(String(20), default="none")
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)

# Helper to create all reco tables

def create_all_reco(engine):
    BaseReco.metadata.create_all(bind=engine)
