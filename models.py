"""
Consolidated Database Models for Voice Dating App
Combines all features: voice messaging, AI matching, location, payments, student verification
"""

from sqlalchemy import Column, Integer, String, DateTime, Boolean, Text, ForeignKey, Date, Float, Numeric, JSON
from decimal import Decimal
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import relationship, backref
from sqlalchemy import Index
from datetime import datetime, date
import enum
from typing import Optional

Base = declarative_base()

# ============================================
# Core User Management
# ============================================

class User(Base):
    """Core user table with profile information"""
    __tablename__ = "users"
    
    # Basic identification
    id = Column(Integer, primary_key=True, index=True)
    phone_number = Column(String(20), unique=True, index=True, nullable=False)
    email = Column(String(255), unique=True, index=True, nullable=True)
    
    # Profile information
    name = Column(String(100), nullable=True)
    dob = Column(Date, nullable=True)
    gender = Column(String(20), nullable=True)  # male, female, non_binary, other
    bio = Column(Text, nullable=True)
    location_text = Column(String(255), nullable=True)  # e.g., "Hyderabad, Telangana, India"
    interests = Column(JSON, nullable=True)  # ["Foodie","Travel"]
    languages = Column(JSON, nullable=True)  # ["English","Telugu"]
    looking_for = Column(String(50), nullable=True)  # long_term, casual, fun
    profession_category = Column(String(100), nullable=True)
    profession_title = Column(String(100), nullable=True)
    height_cm = Column(Integer, nullable=True)
    # Legacy comma-separated photos string used by older endpoints
    profile_photo_url = Column(Text, nullable=True)
    
    # Profile photos (JSON array of URLs)
    profile_photos = Column(JSON, nullable=True)  # ["url1", "url2", "url3"]
    profile_photo_1 = Column(String(500), nullable=True)  # Backward compatibility
    profile_photo_2 = Column(String(500), nullable=True)
    profile_photo_3 = Column(String(500), nullable=True)
    
    # Voice introduction
    voice_intro_url = Column(String(500), nullable=True)
    voice_intro_duration = Column(Integer, nullable=True)  # seconds
    
    # Account status
    is_active = Column(Boolean, default=True)
    is_verified = Column(Boolean, default=False)
    is_profile_complete = Column(Boolean, default=False)
    is_student_verified = Column(Boolean, default=False)
    verified_at = Column(DateTime, nullable=True)
    last_active = Column(DateTime, default=datetime.utcnow)
    
    # AI/ML features
    attractiveness_score = Column(Float, nullable=True)  # Computed by vision models
    ai_embedding = Column(JSON, nullable=True)  # User's ML embedding
    
    # Student information
    university = Column(String(200), nullable=True)
    student_tier = Column(String(50), nullable=True)  # top_private, top_public, etc.
    
    # Timestamps
    created_at = Column(DateTime, default=datetime.utcnow, index=True)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    preferences = relationship("UserPreferences", back_populates="user", uselist=False)
    location_data = relationship("UserLocation", back_populates="user", uselist=False)
    sent_matches = relationship("Match", foreign_keys="Match.user1_id", back_populates="user1")
    received_matches = relationship("Match", foreign_keys="Match.user2_id", back_populates="user2")
    sent_messages = relationship("Message", foreign_keys="Message.sender_id", back_populates="sender")
    received_messages = relationship("Message", foreign_keys="Message.receiver_id", back_populates="receiver")
    sent_voice_messages = relationship("VoiceMessage", foreign_keys="VoiceMessage.sender_id", back_populates="sender")
    received_voice_messages = relationship("VoiceMessage", foreign_keys="VoiceMessage.receiver_id", back_populates="receiver")
    subscriptions = relationship("UserSubscription", back_populates="user")
    verification_attempts = relationship("StudentVerification", back_populates="user")

class UserPreferences(Base):
    """User matching preferences"""
    __tablename__ = "user_preferences"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, unique=True)
    
    # Age preferences
    min_age = Column(Integer, default=18)
    max_age = Column(Integer, default=50)
    
    # Gender preferences
    preferred_genders = Column(JSON, nullable=True)  # legacy
    intent = Column(String(50), nullable=True)  # e.g. long_term, casual, fun, friends
    languages = Column(JSON, nullable=True)  # preferred partner languages
    
    # Location preferences
    max_distance = Column(Integer, default=50)  # kilometers
    distance_miles = Column(Integer, nullable=True)  # optional override in miles
    preferred_locations = Column(JSON, nullable=True)  # Specific cities/areas
    
    # Other preferences
    only_verified = Column(Boolean, default=False)
    only_students = Column(Boolean, default=False)
    
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    user = relationship("User", back_populates="preferences")

# ============================================
# Interaction Events (for ranking / telemetry)
# ============================================

class InteractionEvent(Base):
    """Lightweight event log for impressions/interactions to fuel ranking."""
    __tablename__ = "interaction_events"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False)  # actor
    target_user_id = Column(Integer, ForeignKey("users.id"), nullable=False)
    event_type = Column(String(50), nullable=False)  # impression, like, pass, message, view_profile, etc.
    event_metadata = Column(JSON, nullable=True)
    slate_id = Column(String(64), nullable=True, index=True)  # helps tie impressions/rewards
    rank = Column(Integer, nullable=True)
    surface = Column(String(50), nullable=True)  # discover/playground/etc.
    reward = Column(Float, nullable=True)  # optional immediate reward
    delay_ms = Column(Integer, nullable=True)  # optional latency until action
    created_at = Column(DateTime, default=datetime.utcnow, index=True)

# ============================================
# Embeddings and user features (stub for bandit/RL)
# ============================================

class SpotEmbedding(Base):
    """Stores fused embedding for a spot; vector column can be pgvector if available."""
    __tablename__ = "spot_embeddings"
    spot_id = Column(Integer, ForeignKey("spots.id"), primary_key=True)
    embedding = Column(JSON, nullable=True)  # placeholder; swap to VECTOR when pgvector installed
    created_at = Column(DateTime, default=datetime.utcnow)

class UserFeatures(Base):
    """Stores cached user embedding and lightweight stats."""
    __tablename__ = "user_features"
    user_id = Column(Integer, ForeignKey("users.id"), primary_key=True)
    embedding = Column(JSON, nullable=True)
    recency_stats = Column(JSON, nullable=True)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)

class BanditArmStat(Base):
    """State for contextual bandit (LinUCB) per arm / user."""
    __tablename__ = "bandit_arm_stats"
    id = Column(Integer, primary_key=True, autoincrement=True)
    arm_id = Column(String(128), nullable=False, index=True)
    arm_type = Column(String(32), nullable=False, index=True, default="global")
    user_id = Column(Integer, nullable=True, index=True)  # optional per-user arm
    a_matrix = Column(JSON, nullable=True)   # flattened list
    b_vector = Column(JSON, nullable=True)   # list
    theta_vector = Column(JSON, nullable=True)  # list
    num_pulls = Column(Integer, default=0)
    total_reward = Column(Float, default=0.0)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)

# ============================================
# Location System
# ============================================

class UserLocation(Base):
    """User location data for matching"""
    __tablename__ = "user_locations"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, unique=True)
    
    # GPS coordinates
    latitude = Column(Float, nullable=True)
    longitude = Column(Float, nullable=True)
    accuracy = Column(Float, nullable=True)  # GPS accuracy in meters
    
    # Location details
    city = Column(String(100), nullable=True, index=True)
    state = Column(String(100), nullable=True, index=True)
    country = Column(String(100), nullable=True, default="US")
    neighborhood = Column(String(100), nullable=True)  # Premium feature
    
    # Privacy settings
    is_fuzzy = Column(Boolean, default=False)  # Whether location is intentionally fuzzed
    show_exact_distance = Column(Boolean, default=False)  # Premium feature
    
    # Metadata
    last_updated = Column(DateTime, default=datetime.utcnow, index=True)
    update_source = Column(String(50), nullable=True)  # manual, auto, etc.
    
    # Relationships
    user = relationship("User", back_populates="location_data")

# ============================================
# Matching System
# ============================================

class Match(Base):
    """User matching/swiping data"""
    __tablename__ = "matches"
    
    id = Column(String(50), primary_key=True, index=True)  # UUID
    user1_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    user2_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    # Match status
    user1_liked = Column(Boolean, nullable=True)  # null = no swipe yet
    user2_liked = Column(Boolean, nullable=True)
    is_mutual_match = Column(Boolean, default=False, index=True)
    
    # AI/ML data
    ai_compatibility_score = Column(Float, nullable=True)
    visual_compatibility_score = Column(Float, nullable=True)
    match_reason = Column(String(100), nullable=True)  # location, ai, manual, etc.
    
    # Communication tracking
    messages_count = Column(Integer, default=0)
    voice_messages_count = Column(Integer, default=0)
    last_message_at = Column(DateTime, nullable=True, index=True)
    can_send_text = Column(Boolean, default=False)  # Unlocked after voice messages
    
    # Status
    status = Column(String(20), default="active")  # active, blocked, reported, expired
    blocked_by_user_id = Column(Integer, ForeignKey("users.id"), nullable=True)
    
    created_at = Column(DateTime, default=datetime.utcnow, index=True)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    user1 = relationship("User", foreign_keys=[user1_id], back_populates="sent_matches")
    user2 = relationship("User", foreign_keys=[user2_id], back_populates="received_matches")
    messages = relationship("Message", back_populates="match")
    voice_messages = relationship("VoiceMessage", back_populates="match")

# ============================================
# Messaging System
# ============================================

class Message(Base):
    """Text messages between matched users"""
    __tablename__ = "messages"
    
    id = Column(Integer, primary_key=True, index=True)
    match_id = Column(String(50), ForeignKey("matches.id"), nullable=False, index=True)
    sender_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    receiver_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    # Message content
    content = Column(Text, nullable=False)
    message_type = Column(String(20), default="text")  # text, image, gif, etc.
    
    # Status
    is_read = Column(Boolean, default=False, index=True)
    read_at = Column(DateTime, nullable=True)
    is_deleted = Column(Boolean, default=False)
    
    # Moderation
    is_flagged = Column(Boolean, default=False)
    moderation_status = Column(String(20), nullable=True)  # approved, rejected, pending
    
    created_at = Column(DateTime, default=datetime.utcnow, index=True)
    
    # Relationships
    match = relationship("Match", back_populates="messages")
    sender = relationship("User", foreign_keys=[sender_id], back_populates="sent_messages")
    receiver = relationship("User", foreign_keys=[receiver_id], back_populates="received_messages")

class VoiceMessage(Base):
    """Voice messages between matched users"""
    __tablename__ = "voice_messages"
    
    id = Column(Integer, primary_key=True, index=True)
    match_id = Column(String(50), ForeignKey("matches.id"), nullable=False, index=True)
    sender_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    receiver_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    # Voice data
    voice_url = Column(String(500), nullable=False)
    duration = Column(Integer, nullable=False)  # seconds
    file_size = Column(Integer, nullable=True)  # bytes
    transcription = Column(Text, nullable=True)  # Auto-generated transcription
    
    # Status
    is_played = Column(Boolean, default=False, index=True)
    played_at = Column(DateTime, nullable=True)
    play_count = Column(Integer, default=0)
    
    # AI analysis
    sentiment_score = Column(Float, nullable=True)  # -1 to 1
    audio_quality_score = Column(Float, nullable=True)  # 0 to 1
    
    # Moderation
    is_flagged = Column(Boolean, default=False)
    moderation_status = Column(String(20), nullable=True)
    
    created_at = Column(DateTime, default=datetime.utcnow, index=True)
    
    # Relationships
    match = relationship("Match", back_populates="voice_messages")
    sender = relationship("User", foreign_keys=[sender_id], back_populates="sent_voice_messages")
    receiver = relationship("User", foreign_keys=[receiver_id], back_populates="received_voice_messages")

# ============================================
# Payment and Subscription System
# ============================================

class UserSubscription(Base):
    """User subscription/pass data"""
    __tablename__ = "user_subscriptions"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    # Subscription details
    subscription_type = Column(String(50), nullable=False)  # free, hourly, daily, weekly, monthly, ultra
    status = Column(String(20), default="active")  # active, expired, cancelled, refunded
    
    # Pricing
    original_price = Column(Numeric(10, 2), nullable=False)
    amount_paid = Column(Numeric(10, 2), nullable=False)
    discount_applied = Column(Numeric(10, 2), nullable=True)
    discount_type = Column(String(50), nullable=True)  # student, promo_code
    
    # Dates
    start_date = Column(DateTime, nullable=False, default=datetime.utcnow)
    end_date = Column(DateTime, nullable=True)
    
    # Payment info
    payment_id = Column(String(100), nullable=True)
    payment_method = Column(String(50), nullable=True)
    stripe_subscription_id = Column(String(100), nullable=True)
    
    # Features unlocked
    enhanced_radius = Column(Float, default=0)  # miles
    can_see_city_names = Column(Boolean, default=False)
    can_see_neighborhoods = Column(Boolean, default=False)
    unlimited_swipes = Column(Boolean, default=False)
    priority_visibility = Column(Boolean, default=False)
    
    created_at = Column(DateTime, default=datetime.utcnow, index=True)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    user = relationship("User", back_populates="subscriptions")


# ============================================
# Spots (Short Videos / Audio)
# ============================================

class Spot(Base):
    """30s spots uploaded by users (video/audio)"""
    __tablename__ = "spots"

    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    title = Column(String(200), nullable=True)
    original_url = Column(String(500), nullable=False)
    poster_url = Column(String(500), nullable=True)
    mime_type = Column(String(100), nullable=False)
    duration_sec = Column(Integer, nullable=True)
    renditions = Column(JSON, nullable=True)  # [{"quality":"2160p","url":"...","mime":"video/mp4"}]
    tags = Column(JSON, nullable=True)  # ["Food","Travel"]
    city = Column(String(100), nullable=True)
    is_global = Column(Boolean, default=True)
    expires_at = Column(DateTime, nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow, index=True)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)

    user = relationship("User", backref=backref("spots", lazy="dynamic"))


# ============================================
# Spot Conversations (pre-match chat on spots)
# ============================================

class SpotMessage(Base):
    """Lightweight 30s spot messages exchanged before matching"""
    __tablename__ = "spot_messages"

    id = Column(Integer, primary_key=True, index=True)
    spot_id = Column(Integer, ForeignKey("spots.id"), nullable=False, index=True)
    sender_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    text = Column(Text, nullable=False)
    created_at = Column(DateTime, default=datetime.utcnow, index=True)

    spot = relationship("Spot", backref=backref("spot_messages", lazy="dynamic"))
    sender = relationship("User", backref=backref("spot_messages", lazy="dynamic"))


class SpotPairStatus(Base):
    """Track message counts and profile request/acceptance per spot pair"""
    __tablename__ = "spot_pair_status"

    id = Column(Integer, primary_key=True, index=True)
    spot_id = Column(Integer, ForeignKey("spots.id"), nullable=False, index=True)
    user_a = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)  # lower id
    user_b = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)  # higher id
    a_count = Column(Integer, default=0)
    b_count = Column(Integer, default=0)
    eligible_for_match = Column(Boolean, default=False)
    request_status = Column(String(20), default="none")  # none, requested, received, accepted
    match_id = Column(String(50), nullable=True)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)

    spot = relationship("Spot", backref=backref("pair_statuses", lazy="dynamic"))

# ============================================
# Student Verification System
# ============================================

class StudentVerification(Base):
    """Student verification records"""
    __tablename__ = "student_verifications"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    # University info
    university_name = Column(String(200), nullable=False)
    university_type = Column(String(20), nullable=True)  # public, private
    email = Column(String(255), nullable=False)
    student_id = Column(String(100), nullable=True)
    
    # Verification status
    status = Column(String(20), default="pending")  # pending, approved, rejected
    verification_method = Column(String(50), nullable=False)  # email, id_card, manual
    discount_tier = Column(String(50), nullable=True)  # top_private, top_public, etc.
    
    # Dates
    submitted_at = Column(DateTime, default=datetime.utcnow)
    verified_at = Column(DateTime, nullable=True)
    expires_at = Column(DateTime, nullable=True)
    
    # Verification data
    verification_code = Column(String(50), nullable=True)
    verification_token = Column(String(100), nullable=True)
    
    # Relationships
    user = relationship("User", back_populates="verification_attempts")

# ============================================
# System Tracking and Analytics
# ============================================

class UserActivity(Base):
    """Track user activity for analytics and AI"""
    __tablename__ = "user_activities"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    activity_type = Column(String(50), nullable=False, index=True)  # login, swipe, message, etc.
    activity_data = Column(JSON, nullable=True)  # Additional activity-specific data
    
    # Context
    ip_address = Column(String(45), nullable=True)
    user_agent = Column(String(500), nullable=True)
    device_info = Column(JSON, nullable=True)
    
    created_at = Column(DateTime, default=datetime.utcnow, index=True)

class AIModelData(Base):
    """Store AI/ML model training data and results"""
    __tablename__ = "ai_model_data"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=True, index=True)
    
    model_type = Column(String(50), nullable=False)  # rl_agent, vision_analysis, etc.
    model_version = Column(String(20), nullable=True)
    
    # Training data
    input_features = Column(JSON, nullable=True)
    output_predictions = Column(JSON, nullable=True)
    feedback_score = Column(Float, nullable=True)
    
    # Metadata
    confidence_score = Column(Float, nullable=True)
    processing_time_ms = Column(Integer, nullable=True)
    
    created_at = Column(DateTime, default=datetime.utcnow, index=True)

# ============================================
# Content Moderation
# ============================================

class ContentModeration(Base):
    """Content moderation queue and results"""
    __tablename__ = "content_moderation"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    content_type = Column(String(50), nullable=False)  # profile_photo, bio, voice_message, text_message
    content_id = Column(String(100), nullable=False)  # Reference to the actual content
    content_url = Column(String(500), nullable=True)
    
    # Moderation status
    status = Column(String(20), default="pending")  # pending, approved, rejected, appealed
    auto_flagged = Column(Boolean, default=False)
    manual_review = Column(Boolean, default=False)
    
    # Moderation scores
    inappropriate_score = Column(Float, nullable=True)  # 0-1 from AI models
    toxicity_score = Column(Float, nullable=True)
    spam_score = Column(Float, nullable=True)
    
    # Actions taken
    action_taken = Column(String(50), nullable=True)  # none, blur, remove, ban_user
    moderator_notes = Column(Text, nullable=True)
    
    created_at = Column(DateTime, default=datetime.utcnow, index=True)
    reviewed_at = Column(DateTime, nullable=True)

# ============================================
# Database Indexes for Performance
# ============================================

# User indexes
Index('idx_users_active_profile', User.is_active, User.is_profile_complete)
Index('idx_users_gender_age', User.gender, User.dob)
Index('idx_users_location_active', User.is_active, User.last_active)

# Location indexes
Index('idx_locations_city_state', UserLocation.city, UserLocation.state)
Index('idx_locations_coordinates', UserLocation.latitude, UserLocation.longitude)

# Match indexes
Index('idx_matches_mutual', Match.is_mutual_match, Match.created_at)
Index('idx_matches_user1_status', Match.user1_id, Match.status)
Index('idx_matches_user2_status', Match.user2_id, Match.status)

# Message indexes
Index('idx_messages_match_time', Message.match_id, Message.created_at)
Index('idx_messages_unread', Message.receiver_id, Message.is_read)
Index('idx_voice_messages_match_time', VoiceMessage.match_id, VoiceMessage.created_at)
Index('idx_spot_messages_spot_time', SpotMessage.spot_id, SpotMessage.created_at)
Index('idx_spot_pair_status_spot_users', SpotPairStatus.spot_id, SpotPairStatus.user_a, SpotPairStatus.user_b)

# Subscription indexes
Index('idx_subscriptions_active', UserSubscription.user_id, UserSubscription.status, UserSubscription.end_date)

# Activity indexes
Index('idx_activities_user_type_time', UserActivity.user_id, UserActivity.activity_type, UserActivity.created_at)

# Moderation indexes
Index('idx_moderation_pending', ContentModeration.status, ContentModeration.created_at)
