"""
AI-Powered Dating App with Computer Vision, Location Features, and Student Discounts
Version 3.0.0
"""

import os
import shutil
from datetime import datetime, timedelta
from typing import Optional, List, Dict, Any
import uuid
import json
from types import SimpleNamespace
import requests
from fastapi import FastAPI, UploadFile, File, Form, Depends, HTTPException, Body, Query, BackgroundTasks, WebSocket, WebSocketDisconnect, Request
from strawberry.fastapi import GraphQLRouter
from graphql_schema import schema as graphql_schema
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from fastapi.security import OAuth2PasswordBearer
from jose import jwt, JWTError
from pydantic import BaseModel, field_validator
from enum import Enum
import logging
import asyncio
import numpy as np
import cv2
import redis
from db_raw import pg_execute, pg_pool, Json, pg_fetchone_namedtuple, pg_fetchall_namedtuple
from decimal import Decimal

# Import existing ML components
from ml_models import matching_intelligence, UserFeatures, UserFeatureExtractor
# Import NEW components
from core.matching_intelligence import MatchingIntelligence
from vision.models import DatingAppVisionAnalyzer, ImageAnalysisResult
from vision.face_verification import RealTimeSelfieVerification, SelfieVerificationResult
from location.pass_manager import EnhancedLocationPassManager, PassType, LocationPass
from location.location_matcher import NationwideLocationMatcher, HeatmapGenerator, PathOptimizer
from location.student_discounts import (
    StudentVerificationSystem,
    integrate_student_discounts,
    StudentTier,
    STUDENT_PASS_CONFIGS
)
from event_bus import emit_event

# --- Config ---
SECRET_KEY = os.getenv("SECRET_KEY", "your-secret-key-change-in-production")
ALGORITHM = "HS256"
ACCESS_TOKEN_EXPIRE_MINUTES = 60 * 24 * 7  # 7 days

DATABASE_URL = os.getenv("DATABASE_URL", "postgresql://nava:nava@localhost:5432/nava?sslmode=disable")
REDIS_URL = os.getenv("REDIS_URL", "redis://localhost:6379/0")
STRIPE_API_KEY = os.getenv("STRIPE_API_KEY", "sk_test_your_key")

ALLOWED_MEDIA_TYPES = {
    "video/mp4",
    "video/quicktime",
    "video/x-matroska",
    "video/webm",
    "video/ogg",
    "video/avi",
    "audio/mpeg",
    "audio/aac",
    "audio/wav",
    "audio/ogg",
    "audio/webm",
    "audio/flac",
}

# We assume the Postgres schema already exists; migrations should be run externally.
# The app now relies solely on raw psycopg access via db_raw.pg_execute/pg_pool.

# Setup Redis
try:
    redis_client = redis.from_url(REDIS_URL)
    redis_client.ping()
except:
    redis_client = None
    print("⚠️ Redis not available - using in-memory cache")

# Setup logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

print("🤖 AI-Powered Dating App Starting...")
print("📸 Computer Vision Models Loading...")
print("📍 Location Services Initializing...")
print("🎓 Student Discount System Ready...")

# --- Initialize Enhanced Components ---

# Vision Components
vision_analyzer = DatingAppVisionAnalyzer()

# Location Components
pass_manager = EnhancedLocationPassManager(redis_client=redis_client, stripe_api_key=STRIPE_API_KEY)
pass_manager = integrate_student_discounts(pass_manager)  # Add student discounts
location_matcher = NationwideLocationMatcher(pass_manager)
heatmap_generator = HeatmapGenerator(location_matcher)
path_optimizer = PathOptimizer(location_matcher)

# Selfie Verification
verification_system = RealTimeSelfieVerification(
    vision_analyzer=vision_analyzer,
    rl_agent=matching_intelligence.rl_agent,
    federated_manager=matching_intelligence.federated_manager
)

# Enhanced matching intelligence with vision
matching_intelligence.vision_analyzer = vision_analyzer

print("✅ All systems loaded successfully!")

# In-memory realtime registries (chat + call signaling)
chat_rooms: Dict[str, List[WebSocket]] = {}
call_rooms: Dict[str, List[WebSocket]] = {}
call_sessions: Dict[str, Dict[str, Any]] = {}

# --- Pydantic Models ---
class Gender(str, Enum):
    MALE = "male"
    FEMALE = "female"
    NON_BINARY = "non_binary"
    OTHER = "other"

class UserProfile(BaseModel):
    id: int
    name: str
    age: int
    gender: str
    bio: Optional[str] = None
    photos: List[str] = []
    distance: Optional[float] = None
    ai_score: Optional[float] = None
    city: Optional[str] = None  # NEW
    verification_status: Optional[str] = None  # NEW
    student_verified: Optional[bool] = None  # NEW

class LocationUpdateRequest(BaseModel):
    latitude: float
    longitude: float
    accuracy: float = 10.0

class PurchasePassRequest(BaseModel):
    pass_type: str  # 'hourly', 'daily', 'weekly', 'monthly', 'ultra'
    payment_method: str
    promo_code: Optional[str] = None

class StudentVerificationRequest(BaseModel):
    email: str
    student_id: Optional[str] = None

class SelfieVerificationRequest(BaseModel):
    selfie_image: str  # Base64 encoded image

class LikeRequest(BaseModel):
    target_user_id: int

# --- App setup ---
app = FastAPI(
    title="AI-Powered Dating App API",
    version="3.0.0",
    description="Dating app with AI/ML, Computer Vision, Location Features, and Student Discounts"
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],  # In production, specify exact origins
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Serve uploaded images
if not os.path.exists("uploads"):
    os.makedirs("uploads")
app.mount("/uploads", StaticFiles(directory="uploads"), name="uploads")

oauth2_scheme = OAuth2PasswordBearer(tokenUrl="token")

# --- Helper Functions (keep existing) ---
def create_access_token(data: dict, expires_delta: timedelta = None):
    to_encode = data.copy()
    expire = datetime.utcnow() + (expires_delta or timedelta(minutes=15))
    to_encode.update({"exp": expire})
    return jwt.encode(to_encode, SECRET_KEY, algorithm=ALGORITHM)

def get_current_user_id(token: str = Depends(oauth2_scheme)):
    try:
        payload = jwt.decode(token, SECRET_KEY, algorithms=[ALGORITHM])
        user_id: int = int(payload.get("sub"))
        return user_id
    except JWTError:
        raise HTTPException(status_code=401, detail="Invalid token")

def calculate_age(birth_date):
    today = datetime.now().date()
    return today.year - birth_date.year - ((today.month, today.day) < (birth_date.month, birth_date.day))

def _attr(obj: Any, key: str, default=None):
    if isinstance(obj, dict):
        return obj.get(key, default)
    return getattr(obj, key, default)


def compute_profile_completion(user: Any) -> int:
    """Compute a simple profile completion percentage based on filled fields and photos."""
    if _attr(user, "is_profile_complete", False):
        return 100
    checks = [
        bool(_attr(user, "name")),
        bool(_attr(user, "dob")),
        bool(_attr(user, "gender")),
        bool(_attr(user, "bio")),
        bool(_attr(user, "profile_photo_1") or _attr(user, "profile_photo_2") or _attr(user, "profile_photo_3")),
    ]
    filled = sum(1 for c in checks if c)
    total = len(checks) or 1
    return min(100, int(round((filled / total) * 100)))

def get_user_photos(user: Any) -> List[str]:
    """Normalize photo storage across legacy/new fields."""
    photos_json = _attr(user, "profile_photos")
    if photos_json:
        photos = [p for p in photos_json if p]
        if photos:
            return photos
    photo_csv = _attr(user, "profile_photo_url")
    if photo_csv:
        return [p for p in photo_csv.split(",") if p]
    return [p for p in [_attr(user, "profile_photo_1"), _attr(user, "profile_photo_2"), _attr(user, "profile_photo_3")] if p]


def fetch_user_by_id(user_id: int):
    return pg_fetchone_namedtuple("SELECT * FROM users WHERE id = %s", [user_id])


def fetch_user_by_phone(phone_number: str):
    return pg_fetchone_namedtuple("SELECT * FROM users WHERE phone_number = %s", [phone_number])


def fetch_user_preferences(user_id: int):
    return pg_fetchone_namedtuple("SELECT * FROM user_preferences WHERE user_id = %s", [user_id])


def fetch_user_location(user_id: int):
    return pg_fetchone_namedtuple("SELECT * FROM user_locations WHERE user_id = %s", [user_id])


def fetch_user_subscriptions(user_id: int):
    try:
        return pg_fetchall_namedtuple(
            """
            SELECT id, subscription_type, pass_type, start_date, end_date, status, is_active
            FROM user_subscriptions
            WHERE user_id = %s
            ORDER BY start_date DESC
            """,
            [user_id],
        ) or []
    except Exception as exc:
        logger.error(f"Subscription fetch failed with pass_type; falling back without column: {exc}")
        rows = pg_fetchall_namedtuple(
            """
            SELECT id, subscription_type, start_date, end_date, status
            FROM user_subscriptions
            WHERE user_id = %s
            ORDER BY start_date DESC
            """,
            [user_id],
        ) or []
        # Normalize to objects with pass_type/is_active to keep serialization safe
        return [
            SimpleNamespace(
                id=_attr(r, "id"),
                subscription_type=_attr(r, "subscription_type"),
                pass_type=None,
                start_date=_attr(r, "start_date"),
                end_date=_attr(r, "end_date"),
                status=_attr(r, "status"),
                is_active=None,
            )
            for r in rows
        ]


def fetch_user_spots(user_id: int, limit: int = 10):
    return pg_fetchall_namedtuple(
        """
        SELECT id, title, poster_url, renditions, expires_at, created_at, is_global, city, tags
        FROM spots
        WHERE user_id = %s
        ORDER BY created_at DESC
        LIMIT %s
        """,
        [user_id, limit],
    ) or []


def fetch_users_by_ids(user_ids: List[int]):
    if not user_ids:
        return []
    placeholders = ", ".join(["%s"] * len(user_ids))
    return pg_fetchall_namedtuple(
        f"SELECT * FROM users WHERE id IN ({placeholders})",
        user_ids,
    )

def serialize_user_profile(user: Any) -> Dict[str, Any]:
    """Return a consistent profile payload for the app."""
    preferences = fetch_user_preferences(_attr(user, "id"))
    location = fetch_user_location(_attr(user, "id"))
    spots = fetch_user_spots(_attr(user, "id"), limit=10)
    subscriptions = fetch_user_subscriptions(_attr(user, "id"))

    return {
        "id": _attr(user, "id"),
        "phone_number": _attr(user, "phone_number"),
        "email": _attr(user, "email"),
        "name": _attr(user, "name"),
        "dob": _attr(user, "dob").isoformat() if _attr(user, "dob") else None,
        "gender": _attr(user, "gender"),
        "bio": _attr(user, "bio"),
        "location": _attr(user, "location_text"),
        "interests": _attr(user, "interests") or [],
        "languages": _attr(user, "languages") or [],
        "looking_for": _attr(user, "looking_for"),
        "profession_category": _attr(user, "profession_category"),
        "profession_title": _attr(user, "profession_title"),
        "height_cm": _attr(user, "height_cm"),
        "photos": get_user_photos(user),
        "is_profile_complete": _attr(user, "is_profile_complete"),
        "profile_completion": compute_profile_completion(user),
        "attractiveness_score": _attr(user, "attractiveness_score"),
        "is_verified": _attr(user, "is_verified"),
        "is_student_verified": _attr(user, "is_student_verified"),
        "preferences": preferences and {
            "min_age": _attr(preferences, "min_age"),
            "max_age": _attr(preferences, "max_age"),
            "preferred_genders": _attr(preferences, "preferred_genders"),
            "max_distance_km": _attr(preferences, "max_distance"),
            "only_verified": _attr(preferences, "only_verified"),
            "only_students": _attr(preferences, "only_students"),
            "preferred_locations": _attr(preferences, "preferred_locations"),
        },
        "location_data": location and {
            "latitude": _attr(location, "latitude"),
            "longitude": _attr(location, "longitude"),
            "city": _attr(location, "city"),
            "state": _attr(location, "state"),
            "country": _attr(location, "country"),
            "neighborhood": _attr(location, "neighborhood"),
            "is_fuzzy": _attr(location, "is_fuzzy"),
            "show_exact_distance": _attr(location, "show_exact_distance"),
            "last_updated": _attr(location, "last_updated").isoformat() if _attr(location, "last_updated") else None,
        },
        "location": _attr(user, "location_text"),
        "subscriptions": [
            {
                "id": _attr(s, "id"),
                "subscription_type": _attr(s, "subscription_type"),
                "pass_type": _attr(s, "pass_type"),
                "start_date": _attr(s, "start_date").isoformat() if _attr(s, "start_date") else None,
                "end_date": _attr(s, "end_date").isoformat() if _attr(s, "end_date") else None,
                "is_active": _attr(s, "is_active"),
            }
            for s in subscriptions
        ],
        "spots": [
            {
                "id": _attr(sp, "id"),
                "title": _attr(sp, "title"),
                "poster_url": _attr(sp, "poster_url"),
                "renditions": _attr(sp, "renditions"),
                "expires_at": _attr(sp, "expires_at").isoformat() if _attr(sp, "expires_at") else None,
                "created_at": _attr(sp, "created_at").isoformat() if _attr(sp, "created_at") else None,
                "is_global": _attr(sp, "is_global"),
                "city": _attr(sp, "city"),
                "tags": _attr(sp, "tags"),
            }
            for sp in spots
        ],
    }

def fetch_match_by_id(match_id: str):
    return pg_fetchone_namedtuple(
        """
        SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, created_at, updated_at
        FROM matches
        WHERE id = %s
        """,
        [match_id],
    )


def fetch_match_between(user_a: int, user_b: int):
    return pg_fetchone_namedtuple(
        """
        SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, created_at, updated_at
        FROM matches
        WHERE (user1_id = %s AND user2_id = %s) OR (user1_id = %s AND user2_id = %s)
        LIMIT 1
        """,
        [user_a, user_b, user_b, user_a],
    )


def create_match_record(user_a: int, user_b: int):
    new_id = str(uuid.uuid4())
    pg_execute(
        """
        INSERT INTO matches (id, user1_id, user2_id, created_at, updated_at)
        VALUES (%s, %s, %s, NOW(), NOW())
        """,
        [new_id, user_a, user_b],
    )
    return fetch_match_by_id(new_id)


def get_or_create_match(user_id: int, target_user_id: int) -> Any:
    match = fetch_match_between(user_id, target_user_id)
    if match:
        return match
    return create_match_record(user_id, target_user_id)

# WebSocket chat helpers
def validate_match_membership(match_id: str, user_id: int) -> bool:
    row = pg_execute(
        "SELECT 1 FROM matches WHERE id = %s AND (user1_id = %s OR user2_id = %s)",
        [match_id, user_id, user_id],
        fetchone=True,
    )
    return bool(row)


def insert_message(match_id: str, sender_id: int, receiver_id: int, content: str):
    row = pg_execute(
        """
        INSERT INTO messages (match_id, sender_id, receiver_id, content, message_type, is_flagged, created_at)
        VALUES (%s, %s, %s, %s, %s, %s, NOW())
        RETURNING id, created_at
        """,
        [match_id, sender_id, receiver_id, content, "text", False],
        fetchone=True,
    )
    if row:
        return {"id": row[0], "created_at": row[1]}
    raise HTTPException(status_code=500, detail="Failed to insert message")

def get_db():
    # SQLAlchemy removed; dependency retained for compatibility
    yield None

# GraphQL context per request
def graphql_context(request: Request):
    return {"request": request}


def decode_token(token: str) -> Optional[int]:
    try:
        payload = jwt.decode(token, SECRET_KEY, algorithms=[ALGORITHM])
        return int(payload.get("sub"))
    except Exception:
        return None


def generate_call_token(user_id: int, match_id: str, call_id: str, ttl_seconds: int = 900) -> str:
    """Short-lived token that scopes a user to a specific call + match."""
    expires = datetime.utcnow() + timedelta(seconds=ttl_seconds)
    payload = {
        "sub": str(user_id),
        "match_id": match_id,
        "call_id": call_id,
        "scope": "call",
        "exp": expires,
    }
    return jwt.encode(payload, SECRET_KEY, algorithm=ALGORITHM)


def decode_call_token(token: str) -> Optional[Dict[str, Any]]:
    try:
        payload = jwt.decode(token, SECRET_KEY, algorithms=[ALGORITHM])
        if payload.get("scope") != "call":
            return None
        return payload
    except Exception:
        return None


def is_premium_user(user: Any) -> bool:
    # Basic heuristic: verified or student_verified counts as premium; extend to subscriptions table if present
    if _attr(user, "is_verified", False) or _attr(user, "is_student_verified", False):
        return True
    # Check active subscription via raw SQL
    subs = fetch_user_subscriptions(_attr(user, "id"))
    for sub in subs:
        if _attr(sub, "status") == "active" or _attr(sub, "is_active"):
            return True
    return False

# GraphQL endpoint
graphql_app = GraphQLRouter(graphql_schema, context_getter=graphql_context)
app.include_router(graphql_app, prefix="/graphql")

# --- Enhanced Feature Extraction with Vision ---
def extract_user_features_from_db(user_id: int) -> Optional[UserFeatures]:
    """Extract user features for ML models from database with vision analysis"""
    try:
        user = fetch_user_by_id(user_id)
        if not user or not user.dob:
            return None
        
        # Calculate basic features (keep existing)
        age = calculate_age(user.dob)
        gender_map = {"male": 0.0, "female": 1.0, "non_binary": 0.5, "other": 0.5}
        gender_encoded = gender_map.get(user.gender, 0.5)
        
        # Calculate activity score
        recent_matches_row = pg_execute(
            """
            SELECT COUNT(*) FROM matches
            WHERE (user1_id = %s OR user2_id = %s)
              AND created_at > NOW() - INTERVAL '30 days'
            """,
            [user_id, user_id],
            fetchone=True,
        )
        recent_matches = recent_matches_row[0] if recent_matches_row else 0
        activity_score = min(recent_matches / 50.0, 1.0)
        
        # Calculate selectivity
        total_swipes_row = pg_execute(
            "SELECT COUNT(*) FROM matches WHERE user1_id = %s",
            [user_id],
            fetchone=True,
        )
        likes_given_row = pg_execute(
            "SELECT COUNT(*) FROM matches WHERE user1_id = %s AND user1_liked = TRUE",
            [user_id],
            fetchone=True,
        )
        total_swipes = total_swipes_row[0] if total_swipes_row else 0
        likes_given = likes_given_row[0] if likes_given_row else 0
        selectivity_score = 1 - (likes_given / max(total_swipes, 1))
        
        # Bio sentiment (enhanced with actual analysis if needed)
        bio_sentiment = 0.0
        if user.bio:
            positive_words = ['love', 'happy', 'fun', 'adventure', 'smile', 'joy']
            negative_words = ['hate', 'sad', 'boring', 'tired', 'angry']
            bio_lower = user.bio.lower()
            pos_count = sum(1 for word in positive_words if word in bio_lower)
            neg_count = sum(1 for word in negative_words if word in bio_lower)
            bio_sentiment = (pos_count - neg_count) / max(len(user.bio.split()), 1)
        
        # NEW: Photo attractiveness using vision analyzer
        photo_attractiveness = 0.5  # Default
        if user.profile_photo_url:
            photos = user.profile_photo_url.split(',')
            if photos:
                try:
                    # Analyze first photo with vision model
                    with open(photos[0], 'rb') as f:
                        photo_data = f.read()
                    analysis = vision_analyzer.analyze_image(photo_data)
                    photo_attractiveness = analysis.attractiveness_score
                except Exception as e:
                    logger.error(f"Error analyzing photo: {e}")
        
        # Location cluster
        location_cluster = hash(str(user_id)) % 10
        
        return UserFeatures(
            age=float(age),
            gender_encoded=gender_encoded,
            activity_score=activity_score,
            selectivity_score=selectivity_score,
            bio_sentiment=bio_sentiment,
            photo_attractiveness=photo_attractiveness,
            location_cluster=location_cluster
        )
    except Exception as e:
        logger.error(f"Error extracting features for user {user_id}: {e}")
        return None

# Update the feature extractor
class EnhancedDatabaseFeatureExtractor(UserFeatureExtractor):
    def __init__(self):
        super().__init__()
    
    def _extract_features_from_db(self, user_id: int) -> Optional[UserFeatures]:
        return extract_user_features_from_db(user_id)

# Replace the feature extractor
matching_intelligence.feature_extractor = EnhancedDatabaseFeatureExtractor()

# --- Keep existing authentication endpoints ---
@app.post("/send-otp")
def send_otp(phone_number: str):
    """Send OTP to phone number (currently mocked)"""
    if not pg_pool:
        raise HTTPException(status_code=500, detail="Postgres pool not initialized")
    try:
        row = pg_execute(
            "SELECT id FROM users WHERE phone_number = %s",
            [phone_number],
            fetchone=True,
        )
        if not row:
            pg_execute(
                "INSERT INTO users (phone_number, created_at, updated_at) VALUES (%s, NOW(), NOW())",
                [phone_number],
            )
        
        logger.info(f"OTP request for {phone_number}")
        return {"message": "OTP sent successfully", "otp": "1234"}
    except Exception as e:
        logger.error(f"Error sending OTP: {e}")
        raise HTTPException(status_code=500, detail="Failed to send OTP")

@app.post("/verify-otp")
def verify_otp(phone_number: str, otp: str):
    """Verify OTP and return access token"""
    if otp != "1234":
        raise HTTPException(status_code=400, detail="Invalid OTP")
    row = pg_execute(
        "SELECT id, is_profile_complete FROM users WHERE phone_number = %s",
        [phone_number],
        fetchone=True,
    )
    if not row:
        raise HTTPException(status_code=404, detail="User not found")
    user_id, is_complete = row
    token = create_access_token(
        {"sub": str(user_id)}, 
        expires_delta=timedelta(minutes=ACCESS_TOKEN_EXPIRE_MINUTES)
    )
    response = {
        "access_token": token,
        "token_type": "bearer",
        "user_id": user_id,
        "is_profile_complete": bool(is_complete)
    }
    # Emit user registration/verification event
    emit_event(
        "user.registered",
        "user.registered",
        {
            "user_id": user_id,
            "phone_number": phone_number,
            "is_profile_complete": response["is_profile_complete"],
            "verified_at": datetime.utcnow().isoformat()
        }
    )

    return response

# --- Enhanced Profile with Photo Analysis ---
@app.post("/update-profile")
async def update_profile(
    name: str = Form(...),
    dob: str = Form(...),
    gender: str = Form(...),
    profile_photo_1: UploadFile = File(...),
    profile_photo_2: UploadFile = File(...),
    profile_photo_3: UploadFile = File(...),
    user_id: int = Depends(get_current_user_id),
    background_tasks: BackgroundTasks = BackgroundTasks()
):
    """Update user profile with photo analysis"""
    try:
        # Validate date and gender (keep existing)
        dob_obj = datetime.strptime(dob, "%Y-%m-%d").date()
        age = calculate_age(dob_obj)
        if age < 18:
            raise HTTPException(status_code=400, detail="Must be at least 18 years old")

        if gender.lower() not in [g.value for g in Gender]:
            raise HTTPException(status_code=400, detail="Invalid gender")

        upload_dir = "uploads"
        os.makedirs(upload_dir, exist_ok=True)

        # Save and analyze photos
        filenames = []
        photo_analyses = []
        
        for idx, photo in enumerate([profile_photo_1, profile_photo_2, profile_photo_3], start=1):
            if not photo.content_type.startswith('image/'):
                raise HTTPException(status_code=400, detail=f"Photo {idx} must be an image")
            
            filename = f"{user_id}_photo_{idx}_{int(datetime.now().timestamp())}.jpg"
            file_path = os.path.join(upload_dir, filename)
            
            # Save photo
            with open(file_path, "wb") as buffer:
                content = await photo.read()
                buffer.write(content)
            
            filenames.append(file_path)
            
            # Analyze photo with vision model
            try:
                analysis = vision_analyzer.analyze_image(content)
                photo_analyses.append(analysis)
                
                # Check for inappropriate content
                if analysis.inappropriate_content:
                    os.remove(file_path)
                    raise HTTPException(status_code=400, detail=f"Photo {idx} contains inappropriate content")
            except Exception as e:
                logger.error(f"Error analyzing photo {idx}: {e}")

        # Update user (prefer raw Postgres)
        pg_execute(
            """
            UPDATE users
            SET name = %s,
                dob = %s,
                gender = %s,
                profile_photo_url = %s,
                profile_photo_1 = %s,
                profile_photo_2 = %s,
                profile_photo_3 = %s,
                profile_photos = %s,
                is_profile_complete = TRUE,
                updated_at = NOW()
            WHERE id = %s
            """,
            [
                name,
                dob_obj,
                gender.lower(),
                ",".join(filenames),
                filenames[0],
                filenames[1],
                filenames[2],
                Json(filenames),
                user_id,
            ],
        )
        
        # Clear ML feature cache
        matching_intelligence.feature_extractor.extract_user_features(user_id, force_refresh=True)
        
        # Get photo insights
        insights = []
        for analysis in photo_analyses:
            insights.append({
                "quality": analysis.quality_score,
                "smile_detected": analysis.smile_intensity > 0.5,
                "authenticity": analysis.authenticity_score
            })
        
        logger.info(f"Profile updated for user {user_id} with photo analysis")

        emit_event(
            "user.profile.updated",
            "user.profile.updated",
            {
                "user_id": user_id,
                "name": user.name,
                "gender": user.gender,
                "profile_complete": user.is_profile_complete,
                "attractiveness_score": getattr(user, "attractiveness_score", None),
                "photos": filenames,
                "updated_at": datetime.utcnow().isoformat()
            }
        )
        
        return {
            "message": "Profile updated successfully",
            "photos": filenames,
            "photo_insights": insights
        }
        
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error updating profile: {e}")
        raise HTTPException(status_code=500, detail="Failed to update profile")

# --- NEW: Selfie Verification Endpoint ---
@app.post("/verify/selfie")
async def verify_selfie(
    selfie: UploadFile = File(...),
    user_id: int = Depends(get_current_user_id),
):
    """Verify user identity with real-time selfie"""
    try:
        user = fetch_user_by_id(user_id)
        if not user or not user.profile_photo_url:
            raise HTTPException(status_code=400, detail="Complete your profile first")
        
        # Read selfie
        selfie_data = await selfie.read()
        selfie_array = np.frombuffer(selfie_data, np.uint8)
        selfie_image = cv2.imdecode(selfie_array, cv2.IMREAD_COLOR)
        
        # Get user's stored photos
        stored_photos = []
        for photo_path in user.profile_photo_url.split(','):
            if os.path.exists(photo_path):
                img = cv2.imread(photo_path)
                if img is not None:
                    stored_photos.append(cv2.cvtColor(img, cv2.COLOR_BGR2RGB))
        
        if not stored_photos:
            raise HTTPException(status_code=400, detail="No valid photos found")
        
        # Perform verification
        result = await verification_system.verify_realtime_selfie(
            user_id=user_id,
            selfie_image=cv2.cvtColor(selfie_image, cv2.COLOR_BGR2RGB),
            stored_photos=stored_photos
        )
        
        # Update user verification status
        if result.is_match:
            user.is_verified = True
            user.verified_at = datetime.now()
            db.commit()
        
        return {
            "verified": result.is_match,
            "confidence": result.confidence_score,
            "liveness_score": result.liveness_score,
            "face_match_score": result.face_match_score,
            "failure_reasons": result.failure_reasons if not result.is_match else None
        }
        
    except Exception as e:
        logger.error(f"Selfie verification error: {e}")
        raise HTTPException(status_code=500, detail="Verification failed")

# --- NEW: Location Endpoints ---
@app.post("/location/update")
async def update_location(
    request: LocationUpdateRequest,
    user_id: int = Depends(get_current_user_id),
):
    """Update user's location"""
    try:
        # Update location in matcher
        location_matcher.update_user_location(
            user_id=user_id,
            latitude=request.latitude,
            longitude=request.longitude,
            accuracy=request.accuracy
        )
        
        # Check pass status
        has_pass = pass_manager.has_active_pass(user_id)
        search_radius = pass_manager.get_enhanced_radius(user_id)
        
        # Get location info
        location = location_matcher.user_locations.get(user_id)
        display_location = None
        if location and location.city:
            parts = [location.city, location.state, location.country]
            display_location = ", ".join([p for p in parts if p])
            # Persist a friendly string to the user record for downstream auto-fill
            try:
                pg_execute(
                    "UPDATE users SET location_text = %s, updated_at = NOW() WHERE id = %s",
                    [display_location, user_id],
                )
            except Exception as exc:
                logger.error(f"Could not persist location_text: {exc}")
        
        # Persist raw coords and metadata to user_locations for real-time reads
        try:
            pg_execute(
                """
                INSERT INTO user_locations (user_id, latitude, longitude, accuracy, city, state, country, neighborhood, is_fuzzy, show_exact_distance, last_updated, update_source)
                VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, NOW(), %s)
                ON CONFLICT (user_id)
                DO UPDATE SET
                  latitude = EXCLUDED.latitude,
                  longitude = EXCLUDED.longitude,
                  accuracy = EXCLUDED.accuracy,
                  city = EXCLUDED.city,
                  state = EXCLUDED.state,
                  country = EXCLUDED.country,
                  neighborhood = EXCLUDED.neighborhood,
                  is_fuzzy = EXCLUDED.is_fuzzy,
                  show_exact_distance = EXCLUDED.show_exact_distance,
                  last_updated = NOW(),
                  update_source = EXCLUDED.update_source
                """,
                [
                    user_id,
                    request.latitude,
                    request.longitude,
                    request.accuracy,
                    location.city if location else None,
                    location.state if location else None,
                    location.country if location else None,
                    getattr(location, "neighborhood", None) if location else None,
                    False,
                    False,
                    "auto",
                ],
            )
        except Exception as exc:
            logger.error(f"Could not upsert user_locations: {exc}")
        
        response = {
            "success": True,
            "has_active_pass": has_pass,
            "search_radius": search_radius,
            "location_updated": datetime.now().isoformat(),
            "display_location": display_location,
        }
        
        # Add city info for premium users (kept for backward compatibility)
        if location and pass_manager.can_see_city_names(user_id):
            response["your_location"] = {
                "city": location.city,
                "state": location.state
            }
        
        return response
        
    except Exception as e:
        logger.error(f"Location update error: {e}")
        raise HTTPException(status_code=500, detail="Failed to update location")

@app.post("/location/purchase-pass")
async def purchase_location_pass(
    request: PurchasePassRequest,
    user_id: int = Depends(get_current_user_id),
):
    """Purchase a location pass with student discount if applicable"""
    try:
        pass_type = PassType[request.pass_type.upper()]
    except KeyError:
        raise HTTPException(status_code=400, detail="Invalid pass type")
    
    # Process purchase (student discounts applied automatically)
    result = await pass_manager.purchase_pass(
        user_id=user_id,
        pass_type=pass_type,
        payment_method=request.payment_method,
        promo_code=request.promo_code
    )
    
    if not result['success']:
        raise HTTPException(status_code=400, detail=result['error'])
    
    return result

@app.get("/location/nearby")
async def get_nearby_matches(
    limit: int = Query(20, le=100),
    user_id: int = Depends(get_current_user_id),
):
    """Get nearby matches based on location and pass status"""
    try:
        # Get matches from location matcher
        matches = await location_matcher.find_nationwide_matches(
            user_id=user_id,
            max_results=limit
        )
        
        # Enhance with user data
        enhanced_matches = []
        for match in matches:
            user = fetch_user_by_id(match['user_id'])
            if user:
                age = calculate_age(user.dob) if user.dob else None
                photos = user.profile_photo_url.split(',')[:1] if user.profile_photo_url else []
                
                enhanced_match = {
                    "id": user.id,
                    "name": user.name,
                    "age": age,
                    "photos": photos,
                    "distance": match['distance_display'],
                    "location": match['location_display']
                }
                
                # Add exact distance for premium users
                if 'exact_distance' in match:
                    enhanced_match['exact_distance'] = match['exact_distance']
                
                # Add city for premium users
                if 'city' in match:
                    enhanced_match['city'] = match['city']
                    enhanced_match['state'] = match.get('state')
                
                enhanced_matches.append(enhanced_match)
        
        # Get pass info
        has_pass = pass_manager.has_active_pass(user_id)
        
        return {
            "matches": enhanced_matches,
            "has_premium": has_pass,
            "total_found": len(enhanced_matches)
        }
        
    except Exception as e:
        logger.error(f"Error getting nearby matches: {e}")
        raise HTTPException(status_code=500, detail="Failed to get matches")

# --- NEW: Student Verification Endpoints ---
@app.post("/student/verify")
async def verify_student_status(
    request: StudentVerificationRequest,
    user_id: int = Depends(get_current_user_id),
):
    """Verify student status for discounts"""
    result = await pass_manager.student_verification.verify_student_email(
        user_id=user_id,
        email=request.email,
        student_id=request.student_id
    )
    
    if not result['success']:
        raise HTTPException(status_code=400, detail=result['error'])
    
    # Update user record
    pg_execute(
        "UPDATE users SET is_student_verified = TRUE, university = %s, updated_at = NOW() WHERE id = %s",
        [result['verification']['university'], user_id],
    )
    
    return result

@app.get("/student/status")
async def get_student_status(
    user_id: int = Depends(get_current_user_id)
):
    """Get student verification status and available discounts"""
    verification = pass_manager.student_verification.get_student_status(user_id)
    
    if not verification:
        return {
            "verified": False,
            "message": "Not verified as student"
        }
    
    tier = StudentTier(verification.discount_tier)
    config = STUDENT_PASS_CONFIGS[tier]
    
    return {
        "verified": True,
        "university": verification.university_name,
        "tier": verification.discount_tier,
        "expiry_date": verification.expiry_date.isoformat(),
        "discount_percentage": config['discount_percentage'],
        "special_prices": config['pass_prices'],
        "special_features": config['special_features']
    }

# --- Enhanced Discovery with Location and Vision ---
@app.get("/discover")
async def discover_users(
    limit: int = Query(10, le=50),
    use_ai: bool = Query(True),
    use_location: bool = Query(True),
    user_id: int = Depends(get_current_user_id),
):
    """Get AI-powered potential matches with location and vision analysis"""
    current_user = fetch_user_by_id(user_id)
    if not current_user:
        raise HTTPException(status_code=404, detail="User not found")
    
    # Get location-based matches if enabled
    users = []
    location_user_ids: List[int] = []
    location_matches: List[dict] = []
    if use_location:
        location_matches = await location_matcher.find_nationwide_matches(
            user_id=user_id,
            max_results=limit * 2
        )
        
        location_user_ids = [m['user_id'] for m in location_matches]
        users = [
            u for u in fetch_users_by_ids(location_user_ids)
            if u.id != user_id and u.name and u.dob
        ][: limit * 2]
    else:
        # Original query logic using raw SQL
        matched_rows = pg_execute(
            """
            SELECT user2_id FROM matches WHERE user1_id = %s
            UNION
            SELECT user1_id FROM matches WHERE user2_id = %s
            """,
            [user_id, user_id],
            fetchall=True,
        ) or []
        matched_user_ids = [r[0] for r in matched_rows]
        params: List[Any] = [user_id]
        sql = """
            SELECT *
            FROM users
            WHERE id <> %s
              AND name IS NOT NULL
              AND dob IS NOT NULL
        """
        if matched_user_ids:
            placeholders = ", ".join(["%s"] * len(matched_user_ids))
            sql += f" AND id NOT IN ({placeholders})"
            params.extend(matched_user_ids)
        sql += " ORDER BY created_at DESC LIMIT %s"
        params.append(limit * 2)
        users = pg_fetchall_namedtuple(sql, params)
    
    # Build potential matches with enhanced data
    potential_matches = []
    for user in users:
        user_age = calculate_age(user.dob)
        photos = user.profile_photo_url.split(',') if user.profile_photo_url else []
        
        match_data = {
            "id": user.id,
            "name": user.name,
            "age": user_age,
            "gender": user.gender,
            "bio": user.bio,
            "photos": photos[:1],
            "is_verified": getattr(user, 'is_verified', False)
        }
        
        # Add location data if available
        if use_location and user.id in location_user_ids:
            location_match = next(
                (m for m in location_matches if m['user_id'] == user.id), 
                None
            )
            if location_match:
                match_data["distance"] = location_match.get('distance_display')
                match_data["city"] = location_match.get('city')
        
        # Add visual compatibility if both users have photos
        if current_user.profile_photo_url and user.profile_photo_url:
            try:
                compatibility = vision_analyzer.calculate_visual_compatibility(
                    current_user.profile_photo_url.split(',')[:1],
                    user.profile_photo_url.split(',')[:1]
                )
                match_data["visual_compatibility"] = compatibility
            except:
                pass
        
        potential_matches.append(match_data)
    
    # Apply AI recommendations
    if use_ai and len(potential_matches) > 0:
        try:
            ai_recommendations = matching_intelligence.get_smart_recommendations(
                user_id, potential_matches, limit
            )
            return {
                "users": ai_recommendations,
                "ai_powered": True,
                "location_enabled": use_location,
                "total_candidates": len(potential_matches)
            }
        except Exception as e:
            logger.error(f"AI recommendation error: {e}")
    
    return {
        "users": potential_matches[:limit],
        "ai_powered": False,
        "location_enabled": use_location,
        "total_candidates": len(potential_matches)
    }

# Alias route for frontend expectations
@app.get("/profiles/discover")
async def profiles_discover(
    limit: int = Query(10, le=50),
    use_ai: bool = Query(True),
    use_location: bool = Query(True),
    user_id: int = Depends(get_current_user_id),
):
    return await discover_users(limit=limit, use_ai=use_ai, use_location=use_location, user_id=user_id)

# --- WebSocket for Real-time Location Updates ---
@app.websocket("/ws/location/{user_id}")
async def websocket_location(websocket: WebSocket, user_id: int):
    """WebSocket for real-time location updates (premium feature)"""
    await websocket.accept()
    
    try:
        # Check if user has premium pass
        features = pass_manager.get_user_features(user_id)
        
        if not features.get('real_time_updates', False):
            await websocket.send_json({
                "error": "Real-time updates require a premium pass",
                "upgrade_options": {
                    "hourly": "$12 for 1 hour",
                    "daily": "$20 for 24 hours"
                }
            })
            await websocket.close()
            return
        
        # Add to real-time subscribers
        location_matcher.real_time_subscribers.add(user_id)
        
        while True:
            # Send nearby matches every 30 seconds
            matches = await location_matcher.find_nationwide_matches(user_id)
            
            await websocket.send_json({
                "type": "location_update",
                "matches": matches[:10],
                "timestamp": datetime.now().isoformat()
            })
            
            await asyncio.sleep(30)
            
    except WebSocketDisconnect:
        location_matcher.real_time_subscribers.discard(user_id)
    except Exception as e:
        logger.error(f"WebSocket error: {e}")
        location_matcher.real_time_subscribers.discard(user_id)

# --- Keep existing endpoints (matches, messages, etc.) ---
# [Include all your existing endpoints here - they remain the same]

# Like/Pass endpoints
@app.post("/match/like")
def like_user(request: LikeRequest, user_id: int = Depends(get_current_user_id)):
    target_id = request.target_user_id
    if user_id == target_id:
        raise HTTPException(status_code=400, detail="Cannot like yourself")
    match = get_or_create_match(user_id, target_id)
    was_mutual = bool(_attr(match, "is_mutual_match"))

    if match.user1_id == user_id:
        pg_execute(
            """
            UPDATE matches
            SET user1_liked = TRUE,
                is_mutual_match = COALESCE(user2_liked, FALSE),
                updated_at = NOW()
            WHERE id = %s
            """,
            [match.id],
        )
        match = fetch_match_by_id(match.id)
    else:
        pg_execute(
            """
            UPDATE matches
            SET user2_liked = TRUE,
                is_mutual_match = COALESCE(user1_liked, FALSE),
                updated_at = NOW()
            WHERE id = %s
            """,
            [match.id],
        )
        match = fetch_match_by_id(match.id)
    emit_event(
        "match.like",
        "match.like",
        {"user_id": user_id, "target_user_id": target_id, "match_id": match.id, "is_mutual": match.is_mutual_match}
    )
    try:
        matching_intelligence.record_swipe_action(user_id, target_id, 'like')
    except Exception:
        pass
    if match.is_mutual_match and not was_mutual:
        emit_event(
            "match.created",
            "match.created",
            {"match_id": match.id, "user1_id": match.user1_id, "user2_id": match.user2_id, "created_at": datetime.utcnow().isoformat()}
        )
    return {"match_id": match.id, "is_mutual": match.is_mutual_match}

@app.post("/match/pass")
def pass_user(request: LikeRequest, user_id: int = Depends(get_current_user_id)):
    target_id = request.target_user_id
    if user_id == target_id:
        raise HTTPException(status_code=400, detail="Cannot pass yourself")
    match = get_or_create_match(user_id, target_id)
    if match.user1_id == user_id:
        pg_execute(
            "UPDATE matches SET user1_liked = FALSE, is_mutual_match = FALSE, updated_at = NOW() WHERE id = %s",
            [match.id],
        )
    else:
        pg_execute(
            "UPDATE matches SET user2_liked = FALSE, is_mutual_match = FALSE, updated_at = NOW() WHERE id = %s",
            [match.id],
        )
    match = fetch_match_by_id(match.id)
    emit_event(
        "match.pass",
        "match.pass",
        {"user_id": user_id, "target_user_id": target_id, "match_id": match.id}
    )
    try:
        matching_intelligence.record_swipe_action(user_id, target_id, 'pass')
    except Exception:
        pass
    return {"match_id": match.id, "is_mutual": match.is_mutual_match}

# Aliases for frontend compatibility
@app.post("/profiles/like")
def profiles_like(request: LikeRequest, user_id: int = Depends(get_current_user_id)):
    return like_user(request, user_id=user_id)

@app.post("/profiles/pass")
def profiles_pass(request: LikeRequest, user_id: int = Depends(get_current_user_id)):
    return pass_user(request, user_id=user_id)

# Match detail + Calls
@app.get("/match/{match_id}")
def get_match_detail(match_id: str, user_id: int = Depends(get_current_user_id)):
    """Return the other user's profile (name, bio, location, photos) for a mutual match."""
    match_row = None
    if pg_pool:
        try:
            match_row = pg_execute(
                "SELECT id, user1_id, user2_id, is_mutual_match FROM matches WHERE id = %s",
                [match_id],
                fetchone=True,
            )
        except Exception as exc:
            logger.error(f"PG get_match_detail failed, fallback to ORM: {exc}")
    if match_row:
        _, u1, u2, is_mutual = match_row
        if user_id not in (u1, u2):
            raise HTTPException(status_code=403, detail="Not part of this match")
        if not is_mutual:
            raise HTTPException(status_code=403, detail="Match not mutual yet")
        other_id = u2 if user_id == u1 else u1
        try:
            row = pg_execute(
                "SELECT name, bio, location_text, profile_photos, profile_photo_url FROM users WHERE id = %s",
                [other_id],
                fetchone=True,
            )
            if not row:
                raise HTTPException(status_code=404, detail="User not found")
            name, bio, loc_text, photos_json, photos_csv = row
            photos = photos_json if photos_json else (photos_csv.split(",") if photos_csv else [])
            return {
                "match_id": match_id,
                "user_id": other_id,
                "name": name,
                "bio": bio,
                "location": loc_text,
                "photos": photos[:3] if photos else [],
                "is_mutual": True,
            }
        except HTTPException:
            raise
        except Exception as exc:
            logger.error(f"PG load other user failed: {exc}")
    raise HTTPException(status_code=404, detail="Match not found")

@app.post("/calls")
def create_call(
    request: Request,
    match_id: str = Body(...),
    call_type: str = Body("voice"),
    user_id: int = Depends(get_current_user_id),
):
    """Create an in-app call session and return a scoped signaling token."""
    row = pg_execute(
        "SELECT user1_id, user2_id, is_mutual_match FROM matches WHERE id = %s",
        [match_id],
        fetchone=True,
    )
    if not row:
        raise HTTPException(status_code=404, detail="Match not found")
    u1, u2, is_mutual = row
    if user_id not in (u1, u2):
        raise HTTPException(status_code=403, detail="Not part of this match")
    if not is_mutual:
        raise HTTPException(status_code=403, detail="Match not mutual yet")

    call_id = str(uuid.uuid4())
    token = generate_call_token(user_id, match_id, call_id, ttl_seconds=1800)
    call_sessions[call_id] = {
        "match_id": match_id,
        "call_type": call_type,
        "created_by": user_id,
        "created_at": datetime.utcnow(),
    }
    scheme = request.url.scheme
    ws_scheme = "wss" if scheme == "https" else "ws"
    host = request.url.hostname or "localhost"
    port = request.url.port
    port_part = f":{port}" if port else ""
    signaling_url = f"{ws_scheme}://{host}{port_part}/ws/call"
    return {
        "call_id": call_id,
        "call_type": call_type,
        "token": token,
        "match_id": match_id,
        "signaling_url": signaling_url,
        "expires_in": 1800,
    }

# WebSocket Chat Endpoint
@app.websocket("/ws/chat")
async def websocket_chat(websocket: WebSocket, match_id: str, token: str):
    await websocket.accept()
    try:
        user_id = decode_token(token)
        if not user_id:
            await websocket.close(code=4001)
            return
        if not validate_match_membership(match_id, user_id):
            await websocket.close(code=4003)
            return

        if match_id not in chat_rooms:
            chat_rooms[match_id] = []
        chat_rooms[match_id].append(websocket)

        while True:
            data = await websocket.receive_json()
            if data.get("type") != "message":
                continue
            content = data.get("content", "")
            receiver_id = data.get("receiver_id")
            if not content or not receiver_id:
                continue
            inserted = insert_message(match_id, user_id, int(receiver_id), content)
            payload = {
                "type": "message",
                "match_id": match_id,
                "sender_id": user_id,
                "receiver_id": receiver_id,
                "content": content,
                "message_id": inserted.get("id"),
                "created_at": inserted.get("created_at").isoformat() if inserted.get("created_at") else None,
            }
            for conn in list(chat_rooms.get(match_id, [])):
                try:
                    await conn.send_json(payload)
                except Exception:
                    try:
                        await conn.close()
                    except Exception:
                        pass
                    chat_rooms[match_id].remove(conn)

    except WebSocketDisconnect:
        if match_id in chat_rooms and websocket in chat_rooms[match_id]:
            chat_rooms[match_id].remove(websocket)
    except Exception as e:
        logger.error(f"Chat WS error: {e}")
        if match_id in chat_rooms and websocket in chat_rooms[match_id]:
            chat_rooms[match_id].remove(websocket)


@app.websocket("/ws/call")
async def websocket_call(websocket: WebSocket):
    params = websocket.query_params
    call_id = params.get("call_id")
    match_id = params.get("match_id")
    token = params.get("token")
    if not call_id or not match_id or not token:
        await websocket.close(code=4000)
        return

    payload = decode_call_token(token)
    if not payload or payload.get("call_id") != call_id or payload.get("match_id") != match_id:
        await websocket.close(code=4001)
        return

    user_id = int(payload.get("sub"))
    if not validate_match_membership(match_id, user_id):
        await websocket.close(code=4003)
        return
    session = call_sessions.get(call_id)
    if not session or session.get("match_id") != match_id:
        await websocket.close(code=4004)
        return

    await websocket.accept()
    if call_id not in call_rooms:
        call_rooms[call_id] = []
    call_rooms[call_id].append(websocket)
    try:
        await websocket.send_json(
            {"type": "joined", "call_id": call_id, "match_id": match_id, "user_id": user_id}
        )
        # Broadcast join to the other peer
        for conn in list(call_rooms.get(call_id, [])):
            if conn is websocket:
                continue
            try:
                await conn.send_json(
                    {"type": "participant_joined", "call_id": call_id, "user_id": user_id}
                )
            except Exception:
                continue

        while True:
            message = await websocket.receive_text()
            try:
                data = json.loads(message)
            except Exception:
                data = {"type": "message", "payload": message}
            if not isinstance(data, dict):
                data = {"type": "message", "payload": data}

            data["from_user_id"] = user_id
            data["call_id"] = call_id
            data["match_id"] = match_id
            for conn in list(call_rooms.get(call_id, [])):
                if conn is websocket:
                    continue
                try:
                    await conn.send_json(data)
                except Exception:
                    continue
    except WebSocketDisconnect:
        pass
    finally:
        if call_id in call_rooms and websocket in call_rooms[call_id]:
            call_rooms[call_id].remove(websocket)
        # Notify remaining peers that this participant left
        for conn in list(call_rooms.get(call_id, [])):
            try:
                await conn.send_json(
                    {"type": "participant_left", "call_id": call_id, "user_id": user_id}
                )
            except Exception:
                continue

# Profile status endpoint to report completion percentage
@app.get("/profile/status")
def profile_status(
    user_id: int = Depends(get_current_user_id),
):
    row = pg_execute(
        """
        SELECT name, dob, gender, bio, profile_photo_url, profile_photos,
               is_profile_complete
        FROM users
        WHERE id = %s
        """,
        [user_id],
        fetchone=True,
    )
    if not row:
        raise HTTPException(status_code=404, detail="User not found")
    name, dob, gender, bio, profile_photo_url, profile_photos, is_complete = row
    checks = [
        bool(name),
        bool(dob),
        bool(gender),
        bool(bio),
        bool(profile_photo_url or (profile_photos or [])),
    ]
    percent = 100 if is_complete else int(round((sum(1 for c in checks if c) / len(checks)) * 100))
    return {"profile_completion": min(100, percent), "is_profile_complete": bool(is_complete)}

@app.get("/profile/me")
def profile_me(
    user_id: int = Depends(get_current_user_id),
):
    """Return the full persisted profile so the app can render user data from Postgres."""
    user = fetch_user_by_id(user_id)
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    return serialize_user_profile(user)

@app.get("/location/search")
def location_search(q: str = Query(..., min_length=2, max_length=80), limit: int = Query(6, le=10)):
    """
    Lightweight location lookup. Tries OpenStreetMap Nominatim; falls back to a small built-in list
    so the app still shows suggestions offline.
    """
    fallback = [
        # India
        "Hyderabad, Telangana, India",
        "Bengaluru, Karnataka, India",
        "Chennai, Tamil Nadu, India",
        "Mumbai, Maharashtra, India",
        "Delhi, Delhi, India",
        "Kolkata, West Bengal, India",
        "Pune, Maharashtra, India",
        "Ahmedabad, Gujarat, India",
        "Jaipur, Rajasthan, India",
        # USA
        "San Jose, California, USA",
        "San Francisco, California, USA",
        "New York, New York, USA",
        "Austin, Texas, USA",
        "Houston, Texas, USA",
        "Dallas, Texas, USA",
        "Seattle, Washington, USA",
        "Chicago, Illinois, USA",
        "Los Angeles, California, USA",
        "Boston, Massachusetts, USA",
        # UK/EU
        "London, England, UK",
        "Manchester, England, UK",
        "Dublin, Ireland",
        "Paris, France",
        "Berlin, Germany",
        "Amsterdam, Netherlands",
        # APAC
        "Singapore",
        "Sydney, New South Wales, Australia",
        "Melbourne, Victoria, Australia",
    ]
    results: List[str] = []
    try:
        url = "https://nominatim.openstreetmap.org/search"
        params = {"q": q, "format": "json", "addressdetails": 0, "limit": limit}
        headers = {"User-Agent": "nava-app/1.0"}
        resp = requests.get(url, params=params, headers=headers, timeout=4)
        if resp.ok:
            data = resp.json()
            for item in data:
                display = item.get("display_name")
                if display:
                    results.append(display)
    except Exception as exc:
        logger.warning(f"Nominatim lookup failed: {exc}")

    if not results:
        # Simple substring match on fallback
        q_lower = q.lower()
        results = [c for c in fallback if q_lower in c.lower()][:limit]
        # If still empty, return top fallback list to avoid blank UI
        if not results:
            results = fallback[:limit]

    return {"results": results[:limit]}

@app.post("/update-bio")
def update_bio(
    payload: dict = Body(...),
    user_id: int = Depends(get_current_user_id),
):
    """Update only the bio for the current user."""
    bio = (payload or {}).get("bio", "")
    if not bio or len(bio.strip()) < 5:
        raise HTTPException(status_code=400, detail="Bio is required")
    # Prefer raw Postgres update
    try:
        pg_execute(
            """
            UPDATE users
            SET bio = %s,
                is_profile_complete = TRUE,
                updated_at = NOW()
            WHERE id = %s
            """,
            [bio.strip(), user_id],
        )
    except Exception as exc:
        logger.error(f"Failed to update bio: {exc}")
        raise HTTPException(status_code=500, detail="Could not update bio")
    return {"message": "Bio updated", "bio": bio.strip()}

# --- Health Check with All Systems ---
@app.get("/health")
def health_check():
    """Health check endpoint with all system statuses"""
    try:
        ai_stats = matching_intelligence.get_system_stats()
        ai_healthy = True
    except:
        ai_stats = {"error": "AI system unavailable"}
        ai_healthy = False
    
    try:
        vision_status = "healthy" if vision_analyzer else "unavailable"
    except:
        vision_status = "error"
    
    try:
        location_status = "healthy" if location_matcher else "unavailable"
    except:
        location_status = "error"
    
    return {
        "status": "healthy",
        "timestamp": datetime.utcnow(),
        "systems": {
            "ai": {"status": "healthy" if ai_healthy else "degraded", "stats": ai_stats},
            "vision": {"status": vision_status},
            "location": {"status": location_status},
            "student_discounts": {"status": "active"}
        },
        "features": {
            "reinforcement_learning": True,
            "federated_learning": True,
            "computer_vision": True,
            "selfie_verification": True,
            "location_matching": True,
            "student_discounts": True,
            "real_time_updates": True
        }
    }

# --- Spots Upload & Listing ---
@app.post("/spots")
async def upload_spot(
    title: str = Form(...),
    file: UploadFile = File(...),
    tags: Optional[str] = Form(None),  # comma separated
    city: Optional[str] = Form(None),
    is_global: bool = Form(True),
    duration_sec: Optional[int] = Form(None),
    start_sec: Optional[int] = Form(None),
    end_sec: Optional[int] = Form(None),
    user_id: int = Depends(get_current_user_id),
):
    user = fetch_user_by_id(user_id)
    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    if file.content_type not in ALLOWED_MEDIA_TYPES:
        raise HTTPException(status_code=400, detail="Unsupported media type")

    upload_dir = "uploads/spots"
    os.makedirs(upload_dir, exist_ok=True)
    filename = f"{user_id}_{int(datetime.utcnow().timestamp())}_{file.filename}"
    path = os.path.join(upload_dir, filename)
    content = await file.read()
    with open(path, "wb") as f:
        f.write(content)

    # Enforce 30s limit; allow trimming window
    if duration_sec and duration_sec > 30 and (start_sec is None or end_sec is None):
        raise HTTPException(status_code=400, detail="Please trim to 30 seconds (provide start_sec/end_sec)")
    if start_sec is not None and end_sec is not None:
        if end_sec - start_sec > 30:
            raise HTTPException(status_code=400, detail="Trim window must be 30s or less")

    # Stub renditions (add real ffmpeg pipeline later)
    renditions = [
        {"quality": "2160p", "url": path, "mime": "video/mp4"},
        {"quality": "1080p", "url": path, "mime": "video/mp4"},
        {"quality": "720p", "url": path, "mime": "video/mp4"},
        {"quality": "480p", "url": path, "mime": "video/mp4"},
    ]

    expires_at = None
    premium = is_premium_user(user)
    if not premium:
        expires_at = datetime.utcnow() + timedelta(days=15)

    tag_list = [t.strip() for t in (tags or "").split(",") if t.strip()] or None

    row = pg_execute(
        """
        INSERT INTO spots (user_id, title, original_url, poster_url, mime_type,
                           duration_sec, renditions, tags, city, is_global, expires_at, created_at, updated_at)
        VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, NOW(), NOW())
        RETURNING id
        """,
        [
            user_id,
            title,
            path,
            path,
            file.content_type or "application/octet-stream",
            min(duration_sec, 30) if duration_sec else None,
            Json(renditions),
            Json(tag_list) if tag_list else None,
            city,
            is_global,
            expires_at,
        ],
        fetchone=True,
    )
    if not row:
        raise HTTPException(status_code=500, detail="Failed to save spot")
    spot_id = row[0]

    emit_event(
        "spot.uploaded",
        "spot.uploaded",
        {"spot_id": spot_id, "user_id": user_id, "mime": file.content_type or "application/octet-stream", "title": title},
    )

    return {
        "id": spot_id,
        "title": title,
        "original_url": path,
        "renditions": renditions,
        "expires_at": expires_at,
    }


@app.get("/spots")
def list_spots(
    limit: int = Query(20, le=100),
    user_id: int = Depends(get_current_user_id),
):
    user = fetch_user_by_id(user_id)
    if not user:
        raise HTTPException(status_code=404, detail="User not found")

    premium = is_premium_user(user)

    params = [user_id]
    sql = """
        SELECT id, title, original_url, poster_url, renditions, mime_type,
               tags, city, is_global, expires_at, created_at
        FROM spots
        WHERE user_id = %s
    """
    if not premium:
        sql += " AND created_at >= %s"
        params.append(datetime.utcnow() - timedelta(days=15))
    sql += " ORDER BY created_at DESC LIMIT %s"
    params.append(2 if not premium else limit)
    rows = pg_execute(sql, params, fetchall=True) or []
    data = []
    for row in rows:
        (
            sid,
            title,
            original_url,
            poster_url,
            renditions,
            mime_type,
            tags,
            city,
            is_global,
            expires_at,
            created_at,
        ) = row
        data.append(
            {
                "id": sid,
                "title": title,
                "original_url": original_url,
                "poster_url": poster_url,
                "renditions": renditions or [],
                "mime_type": mime_type,
                "tags": tags,
                "city": city,
                "is_global": is_global,
                "expires_at": expires_at,
                "created_at": created_at,
            }
        )
    return {"spots": data, "premium": premium}

# --- System Statistics ---
@app.get("/admin/stats")
def get_system_statistics(
    user_id: int = Depends(get_current_user_id),
):
    """Get comprehensive system statistics (admin endpoint)"""
    
    # Check if user is admin (implement your admin check)
    # if not is_admin(user_id):
    #     raise HTTPException(status_code=403, detail="Admin access required")
    
    return {
        "ai_stats": matching_intelligence.get_system_stats(),
        "location_stats": {
            "active_passes": len(pass_manager.active_passes),
            "revenue": pass_manager.get_revenue_report()
        },
        "student_stats": pass_manager.student_verification.get_university_analytics(),
        "vision_stats": {
            "cache_size": len(vision_analyzer.cache),
            "verifications_performed": len(verification_system.verification_history)
        }
    }

# --- Run server ---
if __name__ == "__main__":
    import uvicorn
    logger.info("🚀 Starting AI-Powered Dating App v3.0...")
    logger.info("🤖 Features: AI/ML + Computer Vision + Location + Student Discounts")
    uvicorn.run(app, host="0.0.0.0", port=8000)
