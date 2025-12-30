"""
Voice Playground Backend - Anonymous Voice Message Exchange
IMPORTANT: Includes privacy, consent, and safety mechanisms
"""

from fastapi import FastAPI, Depends, HTTPException, File, UploadFile, Form, BackgroundTasks
from sqlalchemy import Column, Integer, String, DateTime, Boolean, Text, ForeignKey, Float, JSON
from sqlalchemy.orm import Session, relationship
from datetime import datetime, timedelta
from typing import List, Dict, Optional
import uuid
import asyncio
import logging
from enum import Enum

# Add to your existing models.py
from models import Base, User, UserSubscription

logger = logging.getLogger(__name__)

# ============================================
# New Database Models for Voice Playground
# ============================================

class PlaygroundVoiceMessage(Base):
    """Anonymous voice messages for the playground"""
    __tablename__ = "playground_voice_messages"
    
    id = Column(String(50), primary_key=True, default=lambda: str(uuid.uuid4()), index=True)
    sender_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    # Voice data
    voice_url = Column(String(500), nullable=False)
    duration = Column(Integer, nullable=False)  # seconds
    transcript = Column(Text, nullable=True)  # Auto-generated for AI training
    
    # Anonymized sender info (for matching preferences)
    sender_gender = Column(String(20), nullable=False, index=True)
    sender_age_group = Column(String(20), nullable=False)  # 18-24, 25-30, etc.
    sender_location_city = Column(String(100), nullable=True, index=True)
    
    # Playground settings
    target_gender = Column(String(20), nullable=True, index=True)  # Who can see this
    is_active = Column(Boolean, default=True, index=True)
    expires_at = Column(DateTime, nullable=False, index=True)  # Auto-expire after 24h
    
    # Engagement tracking
    play_count = Column(Integer, default=0)
    response_count = Column(Integer, default=0)
    like_count = Column(Integer, default=0)
    
    # Safety and moderation
    is_flagged = Column(Boolean, default=False)
    moderation_status = Column(String(20), default="pending")  # pending, approved, rejected
    ai_safety_score = Column(Float, nullable=True)  # 0-1, higher is safer
    
    # AI training consent (CRITICAL)
    ai_training_consent = Column(Boolean, default=False, nullable=False)
    data_retention_consent = Column(Boolean, default=False, nullable=False)
    
    created_at = Column(DateTime, default=datetime.utcnow, index=True)
    
    # Relationships
    sender = relationship("User", foreign_keys=[sender_id])
    responses = relationship("PlaygroundResponse", back_populates="original_message")

class PlaygroundResponse(Base):
    """Responses to playground voice messages"""
    __tablename__ = "playground_responses"
    
    id = Column(String(50), primary_key=True, default=lambda: str(uuid.uuid4()), index=True)
    original_message_id = Column(String(50), ForeignKey("playground_voice_messages.id"), nullable=False)
    responder_id = Column(Integer, ForeignKey("users.id"), nullable=False, index=True)
    
    # Response data
    voice_url = Column(String(500), nullable=False)
    duration = Column(Integer, nullable=False)
    transcript = Column(Text, nullable=True)
    
    # Privacy settings
    is_anonymous = Column(Boolean, default=True)
    reveal_profile = Column(Boolean, default=False)  # If responder wants to reveal profile
    
    # Status
    is_played = Column(Boolean, default=False)
    played_at = Column(DateTime, nullable=True)
    
    # AI training consent
    ai_training_consent = Column(Boolean, default=False, nullable=False)
    
    created_at = Column(DateTime, default=datetime.utcnow)
    
    # Relationships
    original_message = relationship("PlaygroundVoiceMessage", back_populates="responses")
    responder = relationship("User", foreign_keys=[responder_id])

class PlaygroundUsageLimit(Base):
    """Track playground usage limits for free/paid users"""
    __tablename__ = "playground_usage_limits"
    
    id = Column(Integer, primary_key=True, index=True)
    user_id = Column(Integer, ForeignKey("users.id"), nullable=False, unique=True)
    
    # Daily limits
    daily_listens = Column(Integer, default=0)
    daily_responses = Column(Integer, default=0)
    daily_posts = Column(Integer, default=0)
    
    # Reset tracking
    last_reset_date = Column(DateTime, default=datetime.utcnow)
    
    # Premium features
    can_respond_to_multiple = Column(Boolean, default=False)
    max_daily_responses = Column(Integer, default=1)  # Based on subscription
    can_see_responder_profiles = Column(Boolean, default=False)
    
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)
    
    # Relationships
    user = relationship("User")

# ============================================
# Playground Service Class
# ============================================

class VoicePlaygroundService:
    """Service for managing voice playground functionality"""
    
    def __init__(self, db: Session):
        self.db = db
    
    def get_user_limits(self, user_id: int) -> PlaygroundUsageLimit:
        """Get or create user's playground limits"""
        limits = self.db.query(PlaygroundUsageLimit).filter(
            PlaygroundUsageLimit.user_id == user_id
        ).first()
        
        if not limits:
            # Determine limits based on subscription
            subscription = self.get_active_subscription(user_id)
            limits = self._create_usage_limits(user_id, subscription)
            self.db.add(limits)
            self.db.commit()
        
        # Reset daily counters if it's a new day
        if limits.last_reset_date.date() < datetime.utcnow().date():
            limits.daily_listens = 0
            limits.daily_responses = 0
            limits.daily_posts = 0
            limits.last_reset_date = datetime.utcnow()
            self.db.commit()
        
        return limits
    
    def get_active_subscription(self, user_id: int) -> Optional[UserSubscription]:
        """Get user's active subscription"""
        return self.db.query(UserSubscription).filter(
            UserSubscription.user_id == user_id,
            UserSubscription.status == "active",
            UserSubscription.end_date > datetime.utcnow()
        ).first()
    
    def _create_usage_limits(self, user_id: int, subscription: Optional[UserSubscription]) -> PlaygroundUsageLimit:
        """Create usage limits based on subscription tier"""
        if not subscription:
            # Free user limits
            return PlaygroundUsageLimit(
                user_id=user_id,
                can_respond_to_multiple=False,
                max_daily_responses=1,
                can_see_responder_profiles=False
            )
        
        # Subscription-based limits
        subscription_limits = {
            "hourly": {"responses": 2, "multiple": True, "profiles": False},
            "daily": {"responses": 3, "multiple": True, "profiles": True},
            "weekly": {"responses": 5, "multiple": True, "profiles": True},
            "monthly": {"responses": 10, "multiple": True, "profiles": True},
            "ultra": {"responses": 20, "multiple": True, "profiles": True}
        }
        
        limits_config = subscription_limits.get(subscription.subscription_type, subscription_limits["hourly"])
        
        return PlaygroundUsageLimit(
            user_id=user_id,
            can_respond_to_multiple=limits_config["multiple"],
            max_daily_responses=limits_config["responses"],
            can_see_responder_profiles=limits_config["profiles"]
        )
    
    def can_user_respond(self, user_id: int) -> Tuple[bool, str]:
        """Check if user can respond to a voice message"""
        limits = self.get_user_limits(user_id)
        
        if limits.daily_responses >= limits.max_daily_responses:
            return False, f"Daily response limit reached ({limits.max_daily_responses})"
        
        return True, "Can respond"
    
    def can_user_post(self, user_id: int) -> Tuple[bool, str]:
        """Check if user can post a voice message"""
        limits = self.get_user_limits(user_id)
        
        # Free users: 1 post per day, Paid users: 3 posts per day
        max_posts = 3 if limits.max_daily_responses > 1 else 1
        
        if limits.daily_posts >= max_posts:
            return False, f"Daily post limit reached ({max_posts})"
        
        return True, "Can post"

# ============================================
# FastAPI Endpoints for Voice Playground
# ============================================

def add_playground_endpoints(app: FastAPI, get_db, get_current_user_id):
    """Add playground endpoints to your existing FastAPI app"""
    
    @app.post("/playground/post-voice")
    async def post_voice_to_playground(
        target_gender: str = Form(...),  # "male", "female", "any"
        voice_message: UploadFile = File(...),
        ai_training_consent: bool = Form(False),
        data_retention_consent: bool = Form(False),
        user_id: int = Depends(get_current_user_id),
        db: Session = Depends(get_db),
        background_tasks: BackgroundTasks = BackgroundTasks()
    ):
        """Post a voice message to the playground"""
        
        playground_service = VoicePlaygroundService(db)
        
        # Check posting limits
        can_post, message = playground_service.can_user_post(user_id)
        if not can_post:
            raise HTTPException(status_code=429, detail=message)
        
        # Validate consent (REQUIRED for AI training)
        if not ai_training_consent:
            raise HTTPException(
                status_code=400, 
                detail="AI training consent is required to participate in playground"
            )
        
        # Get user info for anonymization
        user = db.query(User).filter(User.id == user_id).first()
        if not user:
            raise HTTPException(status_code=404, detail="User not found")
        
        try:
            # Save voice file
            voice_filename = f"playground_{uuid.uuid4()}.m4a"
            voice_path = f"uploads/playground/{voice_filename}"
            os.makedirs(os.path.dirname(voice_path), exist_ok=True)
            
            with open(voice_path, "wb") as buffer:
                content = await voice_message.read()
                buffer.write(content)
            
            # Get audio duration (implement this based on your audioService)
            duration = get_audio_duration(voice_path)  # You'll need this function
            
            # Create playground message
            playground_msg = PlaygroundVoiceMessage(
                sender_id=user_id,
                voice_url=voice_path,
                duration=duration,
                sender_gender=user.gender,
                sender_age_group=calculate_age_group(user.dob),
                sender_location_city=get_user_city(user_id, db),  # From location service
                target_gender=target_gender if target_gender != "any" else None,
                expires_at=datetime.utcnow() + timedelta(hours=24),
                ai_training_consent=ai_training_consent,
                data_retention_consent=data_retention_consent
            )
            
            db.add(playground_msg)
            
            # Update user's daily post count
            limits = playground_service.get_user_limits(user_id)
            limits.daily_posts += 1
            
            db.commit()
            
            # Queue for AI processing in background
            background_tasks.add_task(process_voice_for_ai, playground_msg.id, voice_path)
            
            return {
                "message_id": playground_msg.id,
                "status": "posted",
                "expires_in_hours": 24,
                "ai_training_enabled": ai_training_consent
            }
            
        except Exception as e:
            logger.error(f"Error posting playground voice: {e}")
            raise HTTPException(status_code=500, detail="Failed to post voice message")
    
    @app.get("/playground/browse")
    async def browse_playground_voices(
        gender_filter: Optional[str] = None,
        limit: int = 20,
        user_id: int = Depends(get_current_user_id),
        db: Session = Depends(get_db)
    ):
        """Browse available playground voice messages"""
        
        # Get user preferences
        user = db.query(User).filter(User.id == user_id).first()
        if not user:
            raise HTTPException(status_code=404, detail="User not found")
        
        # Build query
        query = db.query(PlaygroundVoiceMessage).filter(
            PlaygroundVoiceMessage.is_active == True,
            PlaygroundVoiceMessage.expires_at > datetime.utcnow(),
            PlaygroundVoiceMessage.moderation_status == "approved",
            PlaygroundVoiceMessage.sender_id != user_id  # Don't show own messages
        )
        
        # Apply gender filter based on user's preferences or explicit filter
        if gender_filter:
            query = query.filter(PlaygroundVoiceMessage.sender_gender == gender_filter)
        elif user.gender:
            # Show messages from genders the user might be interested in
            if user.gender == "male":
                query = query.filter(PlaygroundVoiceMessage.sender_gender.in_(["female", "non_binary"]))
            elif user.gender == "female":
                query = query.filter(PlaygroundVoiceMessage.sender_gender.in_(["male", "non_binary"]))
        
        # Also filter by target gender (if message specifies who can see it)
        query = query.filter(
            (PlaygroundVoiceMessage.target_gender == user.gender) |
            (PlaygroundVoiceMessage.target_gender.is_(None))
        )
        
        messages = query.order_by(PlaygroundVoiceMessage.created_at.desc()).limit(limit).all()
        
        # Format response
        voice_messages = []
        for msg in messages:
            voice_messages.append({
                "id": msg.id,
                "voice_url": msg.voice_url,
                "duration": msg.duration,
                "sender_age_group": msg.sender_age_group,
                "sender_gender": msg.sender_gender,
                "sender_location": msg.sender_location_city,
                "play_count": msg.play_count,
                "response_count": msg.response_count,
                "created_hours_ago": int((datetime.utcnow() - msg.created_at).total_seconds() / 3600),
                "expires_in_hours": int((msg.expires_at - datetime.utcnow()).total_seconds() / 3600)
            })
        
        return {
            "voices": voice_messages,
            "total": len(voice_messages),
            "user_daily_responses_left": get_responses_left(user_id, db)
        }
    
    @app.post("/playground/respond/{message_id}")
    async def respond_to_playground_voice(
        message_id: str,
        voice_response: UploadFile = File(...),
        reveal_profile: bool = Form(False),
        ai_training_consent: bool = Form(False),
        user_id: int = Depends(get_current_user_id),
        db: Session = Depends(get_db),
        background_tasks: BackgroundTasks = BackgroundTasks()
    ):
        """Respond to a playground voice message"""
        
        playground_service = VoicePlaygroundService(db)
        
        # Check response limits
        can_respond, message = playground_service.can_user_respond(user_id)
        if not can_respond:
            raise HTTPException(status_code=429, detail=message)
        
        # Get original message
        original_msg = db.query(PlaygroundVoiceMessage).filter(
            PlaygroundVoiceMessage.id == message_id,
            PlaygroundVoiceMessage.is_active == True
        ).first()
        
        if not original_msg:
            raise HTTPException(status_code=404, detail="Voice message not found or expired")
        
        # Don't allow responding to own message
        if original_msg.sender_id == user_id:
            raise HTTPException(status_code=400, detail="Cannot respond to your own message")
        
        try:
            # Save response voice file
            response_filename = f"playground_response_{uuid.uuid4()}.m4a"
            response_path = f"uploads/playground/responses/{response_filename}"
            os.makedirs(os.path.dirname(response_path), exist_ok=True)
            
            with open(response_path, "wb") as buffer:
                content = await voice_response.read()
                buffer.write(content)
            
            duration = get_audio_duration(response_path)
            
            # Create response
            response = PlaygroundResponse(
                original_message_id=message_id,
                responder_id=user_id,
                voice_url=response_path,
                duration=duration,
                reveal_profile=reveal_profile,
                ai_training_consent=ai_training_consent
            )
            
            db.add(response)
            
            # Update counters
            original_msg.response_count += 1
            limits = playground_service.get_user_limits(user_id)
            limits.daily_responses += 1
            
            db.commit()
            
            # Process for AI if consent given
            if ai_training_consent:
                background_tasks.add_task(process_voice_for_ai, response.id, response_path)
            
            return {
                "response_id": response.id,
                "status": "sent",
                "profile_revealed": reveal_profile,
                "ai_training_enabled": ai_training_consent,
                "responses_left_today": limits.max_daily_responses - limits.daily_responses
            }
            
        except Exception as e:
            logger.error(f"Error responding to playground voice: {e}")
            raise HTTPException(status_code=500, detail="Failed to send response")
    
    @app.get("/playground/my-messages")
    async def get_my_playground_messages(
        user_id: int = Depends(get_current_user_id),
        db: Session = Depends(get_db)
    ):
        """Get user's playground messages and responses received"""
        
        # Get user's posted messages
        my_messages = db.query(PlaygroundVoiceMessage).filter(
            PlaygroundVoiceMessage.sender_id == user_id
        ).order_by(PlaygroundVoiceMessage.created_at.desc()).all()
        
        messages_data = []
        for msg in my_messages:
            responses = db.query(PlaygroundResponse).filter(
                PlaygroundResponse.original_message_id == msg.id
            ).all()
            
            messages_data.append({
                "id": msg.id,
                "duration": msg.duration,
                "target_gender": msg.target_gender,
                "play_count": msg.play_count,
                "response_count": len(responses),
                "created_at": msg.created_at,
                "expires_at": msg.expires_at,
                "is_active": msg.is_active,
                "responses": [
                    {
                        "id": r.id,
                        "voice_url": r.voice_url,
                        "duration": r.duration,
                        "responder_revealed": r.reveal_profile,
                        "responder_id": r.responder_id if r.reveal_profile else None,
                        "created_at": r.created_at,
                        "is_played": r.is_played
                    } for r in responses
                ]
            })
        
        return {
            "my_messages": messages_data,
            "total_messages": len(my_messages),
            "total_responses_received": sum(msg.response_count for msg in my_messages)
        }

# ============================================
# AI Training Data Processing
# ============================================

async def process_voice_for_ai(message_id: str, voice_path: str):
    """Process voice message for AI training (background task)"""
    try:
        # Only process if user gave consent
        db = SessionLocal()
        
        # Check if it's a playground message or response
        playground_msg = db.query(PlaygroundVoiceMessage).filter(
            PlaygroundVoiceMessage.id == message_id
        ).first()
        
        playground_response = db.query(PlaygroundResponse).filter(
            PlaygroundResponse.id == message_id
        ).first()
        
        if not playground_msg and not playground_response:
            return
        
        # Check consent
        has_consent = False
        if playground_msg and playground_msg.ai_training_consent:
            has_consent = True
        elif playground_response and playground_response.ai_training_consent:
            has_consent = True
        
        if not has_consent:
            logger.info(f"Skipping AI processing for {message_id} - no consent")
            return
        
        # Extract features for AI training
        voice_features = extract_voice_features(voice_path)  # Implement this
        transcript = transcribe_audio(voice_path)  # Implement this
        
        # Store in AI training dataset
        ai_data = {
            "voice_features": voice_features,
            "transcript": transcript,
            "message_id": message_id,
            "consent_verified": True,
            "processed_at": datetime.utcnow()
        }
        
        # Save to your AI training database/storage
        save_ai_training_data(ai_data)  # Implement this
        
        # Update transcript in database
        if playground_msg:
            playground_msg.transcript = transcript
        elif playground_response:
            playground_response.transcript = transcript
        
        db.commit()
        db.close()
        
        logger.info(f"AI processing completed for message {message_id}")
        
    except Exception as e:
        logger.error(f"Error processing voice for AI: {e}")

# ============================================
# Utility Functions (implement these)
# ============================================

def get_audio_duration(file_path: str) -> int:
    """Get audio duration in seconds"""
    # Implement using mutagen or similar
    pass

def calculate_age_group(dob) -> str:
    """Calculate age group from date of birth"""
    if not dob:
        return "unknown"
    
    age = (datetime.now().date() - dob).days // 365
    if age < 25:
        return "18-24"
    elif age < 31:
        return "25-30"
    elif age < 41:
        return "31-40"
    elif age < 51:
        return "41-50"
    else:
        return "50+"

def get_user_city(user_id: int, db: Session) -> Optional[str]:
    """Get user's city from location data"""
    # Implement based on your location service
    pass

def get_responses_left(user_id: int, db: Session) -> int:
    """Get how many responses user has left today"""
    service = VoicePlaygroundService(db)
    limits = service.get_user_limits(user_id)
    return max(0, limits.max_daily_responses - limits.daily_responses)

def extract_voice_features(voice_path: str) -> Dict:
    """Extract features from voice for AI training"""
    # Implement voice feature extraction
    pass

def transcribe_audio(voice_path: str) -> str:
    """Transcribe audio to text"""
    # Implement using speech-to-text service
    pass

def save_ai_training_data(data: Dict):
    """Save data for AI model training"""
    # Implement storage for training data
    pass