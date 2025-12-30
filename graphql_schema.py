from __future__ import annotations
import os
import uuid
import strawberry
import asyncio
import inspect
import uuid
import random
import logging
import math
from datetime import datetime, date
from typing import List, Optional
from strawberry.types import Info
from types import SimpleNamespace
from strawberry.schema.config import StrawberryConfig
from strawberry.file_uploads import Upload
from jose import jwt
from db_raw import pg_execute, pg_pool, Json, pg_fetchone_namedtuple, pg_fetchall_namedtuple
import io
import numpy as np
import face_recognition
from collections import defaultdict
from services.linucb import get_bandit

SECRET_KEY = os.getenv("SECRET_KEY", "your-secret-key-change-in-production")
ALGORITHM = "HS256"
SKIP_FACE_CHECK = os.getenv("SKIP_FACE_CHECK", "true").lower() in ("1", "true", "yes", "y")
logger = logging.getLogger(__name__)
if SKIP_FACE_CHECK:
    logger.warning("Face validation disabled (SKIP_FACE_CHECK=1); do not use in production")

# In-memory pub/sub for GraphQL subscriptions (message stream)
message_subscribers: dict[str, list[asyncio.Queue]] = defaultdict(list)
spot_subscribers: dict[int, list[asyncio.Queue]] = defaultdict(list)


def _attr(obj, key, default=None):
    if isinstance(obj, dict):
        return obj.get(key, default)
    return getattr(obj, key, default)


def fetch_user(user_id: int):
    return pg_fetchone_namedtuple("SELECT * FROM users WHERE id = %s", [user_id])


def fetch_users(limit: int, offset: int = 0, exclude_ids: Optional[list[int]] = None):
    params: list = []
    sql = "SELECT * FROM users WHERE 1=1"
    if exclude_ids:
        placeholders = ", ".join(["%s"] * len(exclude_ids))
        sql += f" AND id NOT IN ({placeholders})"
        params.extend(exclude_ids)
    sql += " ORDER BY created_at DESC LIMIT %s OFFSET %s"
    params.extend([limit, offset])
    return pg_fetchall_namedtuple(sql, params) or []


def fetch_user_preferences(user_id: int):
    return pg_fetchone_namedtuple("SELECT * FROM user_preferences WHERE user_id = %s", [user_id])


def fetch_user_location(user_id: int):
    return pg_fetchone_namedtuple("SELECT * FROM user_locations WHERE user_id = %s", [user_id])


def fetch_spots_for_user(user_id: int, limit: int = 10, offset: int = 0):
    return pg_fetchall_namedtuple(
        """
        SELECT * FROM spots
        WHERE user_id = %s
        ORDER BY created_at DESC
        LIMIT %s OFFSET %s
        """,
        [user_id, limit, offset],
    ) or []


def fetch_spot(spot_id: int):
    return pg_fetchone_namedtuple("SELECT * FROM spots WHERE id = %s", [spot_id])


def fetch_match(match_id: str):
    return pg_fetchone_namedtuple("SELECT * FROM matches WHERE id = %s", [match_id])


def fetch_messages_for_match(match_id: str, limit: int = 100, offset: int = 0):
    return pg_fetchall_namedtuple(
        """
        SELECT * FROM messages
        WHERE match_id = %s
        ORDER BY created_at ASC
        LIMIT %s OFFSET %s
        """,
        [match_id, limit, offset],
    ) or []


def fetch_spot_pair_status(spot_id: int, user_a: int, user_b: int):
    # Ensure canonical order (a <= b) before querying
    user_a, user_b = normalize_pair(user_a, user_b)
    return pg_fetchone_namedtuple(
        """
        SELECT * FROM spot_pair_status
        WHERE spot_id = %s AND user_a = %s AND user_b = %s
        """,
        [spot_id, user_a, user_b],
    )


def upsert_spot_pair_status(spot_id: int, user_a: int, user_b: int, a_count: int, b_count: int, request_status: str, match_id: Optional[str]):
    user_a, user_b = normalize_pair(user_a, user_b)
    # Some databases lack the composite unique constraint; fall back to manual select/update
    existing = fetch_spot_pair_status(spot_id, user_a, user_b)
    if existing:
        return pg_fetchone_namedtuple(
            """
            UPDATE spot_pair_status
            SET a_count = %s,
                b_count = %s,
                request_status = %s,
                match_id = %s,
                updated_at = NOW()
            WHERE spot_id = %s AND user_a = %s AND user_b = %s
            RETURNING *
            """,
            [a_count, b_count, request_status, match_id, spot_id, user_a, user_b],
        )
    return pg_fetchone_namedtuple(
        """
        INSERT INTO spot_pair_status (spot_id, user_a, user_b, a_count, b_count, request_status, match_id, created_at, updated_at)
        VALUES (%s, %s, %s, %s, %s, %s, %s, NOW(), NOW())
        RETURNING *
        """,
        [spot_id, user_a, user_b, a_count, b_count, request_status, match_id],
    )


def fetch_user_features(user_id: int):
    return pg_fetchone_namedtuple("SELECT * FROM user_features WHERE user_id = %s", [user_id])


def fetch_spot_embeddings(spot_ids: list[int]):
    if not spot_ids:
        return []
    placeholders = ", ".join(["%s"] * len(spot_ids))
    return pg_fetchall_namedtuple(
        f"SELECT * FROM spot_embeddings WHERE spot_id IN ({placeholders})",
        spot_ids,
    ) or []

@strawberry.type
class OtpResponse:
    # Explicit strawberry.field to satisfy schema generation on older versions
    message: str = strawberry.field()
    success: bool = strawberry.field()

# GraphQL types mapping to your SQLAlchemy models
@strawberry.type
class UserType:
    id: int
    name: Optional[str]
    email: Optional[str]
    phone_number: Optional[str]
    bio: Optional[str]
    gender: Optional[str]
    is_active: bool
    # Allow nullable so DB nulls don’t crash the schema
    is_verified: Optional[bool]
    is_student_verified: Optional[bool]
    age: Optional[int] = None
    photos: Optional[List[str]] = None
    is_profile_complete: Optional[bool] = None
    height_cm: Optional[int] = None
    languages: Optional[List[str]] = None
    location: Optional[str] = None
    interests: Optional[List[str]] = None
    looking_for: Optional[str] = None
    profession_category: Optional[str] = None
    profession_title: Optional[str] = None
    # Voice intro fields (optional, to satisfy existing app queries)
    voice_intro_url: Optional[str] = None
    voice_intro_duration: Optional[int] = None

@strawberry.type
class UserPreferencesType:
    id: int
    min_age: Optional[int]
    max_age: Optional[int]
    intent: Optional[str]
    languages: Optional[List[str]]
    distance_miles: Optional[int]
    preferred_genders: Optional[List[str]]
    only_verified: bool
    only_students: bool

@strawberry.input
class PreferencesInput:
    min_age: Optional[int] = None
    max_age: Optional[int] = None
    intent: Optional[str] = None
    languages: Optional[List[str]] = None
    distance_miles: Optional[int] = None
    preferred_genders: Optional[List[str]] = None
    only_verified: Optional[bool] = None

@strawberry.type
class RankedUserType:
    user: UserType
    score: float
    rank: int
    slate_id: str
@strawberry.type
class MatchType:
    id: str
    user1_id: int
    user2_id: int
    is_mutual_match: bool
    status: Optional[str]

@strawberry.type
class MessageType:
    id: int
    match_id: str
    sender_id: int
    receiver_id: int
    content: str
    message_type: Optional[str]
    is_read: bool
    created_at: Optional[datetime] = None


@strawberry.type
class SpotMessageType:
    id: int
    spot_id: int
    sender_id: int
    text: str
    created_at: datetime


@strawberry.type
class SpotConversationPayload:
    messages: List[SpotMessageType]
    a_count: int
    b_count: int
    eligible: bool
    request_status: str
    match_id: Optional[str]

@strawberry.type
class SpotType:
    id: int
    user_id: Optional[int]
    title: Optional[str]
    original_url: Optional[str]
    poster_url: Optional[str]
    mime_type: Optional[str]
    tags: Optional[List[str]]
    is_global: bool
    expires_at: Optional[datetime]
    created_at: Optional[datetime]

@strawberry.type
class RankedSpotType:
    spot: SpotType
    score: float
    rank: int
    slate_id: str

@strawberry.type
class MePayload:
    user: Optional[UserType] = None
    preferences: Optional[UserPreferencesType] = None
    spots: Optional[List[SpotType]] = None


@strawberry.type
class AuthPayload:
    access_token: str
    user_id: Optional[int] = None
    is_profile_complete: Optional[bool] = None
    token_type: Optional[str] = "bearer"
    profile_photo_1_url: Optional[str] = None
    profile_photo_2_url: Optional[str] = None
    profile_photo_3_url: Optional[str] = None


def get_db(info: Info):
    # ORM removed; retained for compatibility
    return None

def get_user_id_from_context(info: Info) -> Optional[int]:
    request = info.context.get("request") if info.context else None
    if not request:
        return None
    auth = request.headers.get("authorization")
    if auth and auth.lower().startswith("bearer "):
        token = auth.split(" ", 1)[1]
        try:
            payload = jwt.decode(token, SECRET_KEY, algorithms=[ALGORITHM])
            return int(payload.get("sub"))
        except Exception:
            return None
    return None


def to_user_type(user: User) -> UserType:
    # Build a merged photos list from the JSON column or legacy columns
    photos = []
    profile_photos = _attr(user, "profile_photos")
    if profile_photos:
        photos = [p for p in profile_photos if p]
    else:
        for p in [_attr(user, "profile_photo_1"), _attr(user, "profile_photo_2"), _attr(user, "profile_photo_3")]:
            if p:
                photos.append(p)

    # Compute age from dob if available
    age = None
    if _attr(user, "dob"):
        today = datetime.utcnow().date()
        age = today.year - user.dob.year - ((today.month, today.day) < (user.dob.month, user.dob.day))

    # Basic location string if location_data exists
    loc = None
    if _attr(user, "location_text"):
        loc = _attr(user, "location_text")

    return UserType(
        id=_attr(user, "id"),
        name=_attr(user, "name"),
        email=_attr(user, "email"),
        phone_number=_attr(user, "phone_number"),
        bio=_attr(user, "bio"),
        gender=_attr(user, "gender"),
        is_active=bool(_attr(user, "is_active", True)),
        is_verified=False if _attr(user, "is_verified") is None else bool(_attr(user, "is_verified")),
        is_student_verified=False if _attr(user, "is_student_verified") is None else bool(_attr(user, "is_student_verified")),
        age=age,
        photos=photos or None,
        is_profile_complete=_attr(user, "is_profile_complete"),
        height_cm=_attr(user, "height_cm"),
        languages=_attr(user, "languages") if isinstance(_attr(user, "languages"), list) else None,
        location=loc,
        interests=_attr(user, "interests") if isinstance(_attr(user, "interests"), list) else None,
        looking_for=_attr(user, "looking_for"),
        profession_category=_attr(user, "profession_category"),
        profession_title=_attr(user, "profession_title"),
        voice_intro_url=_attr(user, "voice_intro_url"),
        voice_intro_duration=_attr(user, "voice_intro_duration"),
    )


def to_preferences_type(pref: UserPreferences) -> UserPreferencesType:
    return UserPreferencesType(
        id=pref.id,
        min_age=pref.min_age,
        max_age=pref.max_age,
        intent=pref.intent,
        languages=pref.languages if isinstance(pref.languages, list) else None,
        distance_miles=pref.distance_miles if pref.distance_miles is not None else pref.max_distance,
        preferred_genders=pref.preferred_genders if isinstance(pref.preferred_genders, list) else None,
        only_verified=pref.only_verified,
        only_students=pref.only_students,
    )


def to_match_type(match: Match) -> MatchType:
    return MatchType(
        id=match.id,
        user1_id=match.user1_id,
        user2_id=match.user2_id,
        is_mutual_match=match.is_mutual_match,
        status=match.status,
    )


def to_message_type(message: Message) -> MessageType:
    return MessageType(
        id=message.id,
        match_id=message.match_id,
        sender_id=message.sender_id,
        receiver_id=message.receiver_id,
        content=message.content,
        message_type=message.message_type,
        is_read=message.is_read,
        created_at=getattr(message, "created_at", None),
    )


def to_spot_message_type(msg: SpotMessage) -> SpotMessageType:
    return SpotMessageType(
        id=msg.id,
        spot_id=msg.spot_id,
        sender_id=msg.sender_id,
        text=msg.text,
        created_at=getattr(msg, "created_at", datetime.utcnow()),
    )

def age_from_dob(dob) -> Optional[int]:
    if not dob:
        return None
    if isinstance(dob, str):
        try:
            dob = datetime.fromisoformat(dob).date()
        except Exception:
            return None
    if isinstance(dob, datetime):
        dob = dob.date()
    if not isinstance(dob, date):
        return None
    today = date.today()
    return today.year - dob.year - ((today.month, today.day) < (dob.month, dob.day))

def score_candidate(pref: Optional[object], candidate: object) -> float:
    """Heuristic scorer combining verification, language overlap, intent match, and age proximity."""
    score = 0.0
    if not candidate:
        return score
    # Verified boost
    if _attr(candidate, "is_verified", False):
        score += 1.0
    # Intent
    if pref and _attr(pref, "intent") and _attr(candidate, "looking_for") and str(_attr(pref, "intent")).lower() == str(_attr(candidate, "looking_for")).lower():
        score += 1.0
    # Languages
    cand_langs = set(_attr(candidate, "languages") or []) if isinstance(_attr(candidate, "languages"), list) else set()
    if pref and isinstance(_attr(pref, "languages"), list):
        overlap = len(cand_langs.intersection(set(_attr(pref, "languages"))))
        score += min(overlap, 3) * 0.5
    # Age proximity
    cand_age = age_from_dob(_attr(candidate, "dob"))
    if pref and cand_age is not None and _attr(pref, "min_age") and _attr(pref, "max_age"):
        center = (_attr(pref, "min_age") + _attr(pref, "max_age")) / 2
        diff = abs(cand_age - center)
        score += max(0, 1.5 - diff * 0.05)  # decay with distance from center
    return score

def score_spot(pref: Optional[object], owner: Optional[object]) -> float:
    """Heuristic scorer for a spot using owner metadata and preferences."""
    if not owner:
        return 0.0
    return score_candidate(pref, owner)

def broadcast_message(match_id: str, message: MessageType):
    """Push a message to all async subscribers for this match."""
    if not message_subscribers.get(match_id):
        return
    for q in list(message_subscribers.get(match_id, [])):
        try:
            q.put_nowait(message)
        except Exception:
            try:
                message_subscribers[match_id].remove(q)
            except Exception:
                pass

async def read_upload_bytes(file: Upload) -> bytes:
    """Safely read Upload content inside an active event loop."""
    content = file.read()
    if inspect.isawaitable(content):
        content = await content
    if isinstance(content, bytes):
        return content
    return bytes(content)

def extract_face_embedding(image_bytes: bytes) -> Optional[np.ndarray]:
    try:
        image = face_recognition.load_image_file(io.BytesIO(image_bytes))
        encs = face_recognition.face_encodings(image)
        if len(encs) != 1:
            return None
        return np.array(encs[0], dtype=np.float32)
    except Exception:
        return None

def cosine_similarity(a: np.ndarray, b: np.ndarray) -> float:
    denom = (np.linalg.norm(a) * np.linalg.norm(b)) + 1e-8
    return float(np.dot(a, b) / denom)

def haversine_miles(lat1: float, lon1: float, lat2: float, lon2: float) -> Optional[float]:
    try:
        lat1, lon1, lat2, lon2 = float(lat1), float(lon1), float(lat2), float(lon2)
    except Exception:
        return None
    R = 3958.8  # Earth radius in miles
    dlat = math.radians(lat2 - lat1)
    dlon = math.radians(lon2 - lon1)
    a = math.sin(dlat / 2) ** 2 + math.cos(math.radians(lat1)) * math.cos(math.radians(lat2)) * math.sin(dlon / 2) ** 2
    c = 2 * math.asin(math.sqrt(a))
    return R * c


def normalize_pair(user_id: int, other_id: int) -> tuple[int, int]:
    return (user_id, other_id) if user_id <= other_id else (other_id, user_id)

def to_np(embedding) -> Optional[np.ndarray]:
    try:
        if embedding is None:
            return None
        return np.array(embedding, dtype=np.float32)
    except Exception:
        return None


def build_spot_conversation(spot_id: int, user_a: int, user_b: int) -> SpotConversationPayload:
    """Assemble conversation payload for a given spot and user pair."""
    a_id, b_id = normalize_pair(user_a, user_b)
    status = fetch_spot_pair_status(spot_id, a_id, b_id)
    if not status:
        status = upsert_spot_pair_status(spot_id, a_id, b_id, 0, 0, "none", None)

    # Only return messages between this viewer and the spot owner to keep threads private
    messages = pg_fetchall_namedtuple(
        """
        SELECT * FROM spot_messages
        WHERE spot_id = %s AND sender_id IN (%s, %s)
        ORDER BY created_at ASC
        """,
        [spot_id, a_id, b_id],
    ) or []
    eligible_flag = _attr(status, "eligible_for_match") or (_attr(status, "a_count", 0) >= 5 and _attr(status, "b_count", 0) >= 5)
    if eligible_flag and not _attr(status, "eligible_for_match"):
        status = upsert_spot_pair_status(
            spot_id,
            a_id,
            b_id,
            _attr(status, "a_count", 0),
            _attr(status, "b_count", 0),
            _attr(status, "request_status", "none"),
            _attr(status, "match_id"),
        )

    return SpotConversationPayload(
        messages=[to_spot_message_type(m) for m in messages],
        a_count=_attr(status, "a_count", 0),
        b_count=_attr(status, "b_count", 0),
        eligible=eligible_flag,
        request_status=_attr(status, "request_status", "none"),
        match_id=_attr(status, "match_id"),
    )


def broadcast_spot_conversation(spot_id: int, payload: SpotConversationPayload):
    # Disabled broadcast to avoid leaking one pair's messages to other viewers of the same spot
    return

def to_spot_type(spot: object) -> SpotType:
    return SpotType(
        id=_attr(spot, "id"),
        user_id=_attr(spot, "user_id"),
        title=_attr(spot, "title"),
        original_url=_attr(spot, "original_url"),
        poster_url=_attr(spot, "poster_url"),
        mime_type=_attr(spot, "mime_type"),
        tags=_attr(spot, "tags") if isinstance(_attr(spot, "tags"), list) else None,
        is_global=_attr(spot, "is_global"),
        expires_at=_attr(spot, "expires_at"),
        created_at=_attr(spot, "created_at"),
    )

def build_spot_candidate_features(spot: Spot, owner: Optional[User], user_vec: Optional[np.ndarray], spot_vecs: dict) -> dict:
    cand = {
        "id": spot.id,
        "creator_id": owner.id if owner else None,
        "creator_verified": getattr(owner, "is_verified", False),
        "quality_score": 0.6,
        "engagement_rate": 0.0,
        "trend_velocity": 0.0,
        "freshness": 0.5,
        "distance_score": 0.5,
        "similarity_score": 0.0,
    }
    if getattr(spot, "created_at", None):
        age_hours = max(1, (datetime.utcnow() - spot.created_at).total_seconds() / 3600.0)
        cand["freshness"] = float(np.exp(-age_hours / 48.0))
        cand["trend_velocity"] = max(0.0, 1.0 - age_hours / 72.0)
    if user_vec is not None:
        vec = spot_vecs.get(spot.id)
        if vec is not None:
            cand["similarity_score"] = cosine_similarity(user_vec, vec)
    return cand


@strawberry.type
class Query:
    @strawberry.field
    def user(self, info: Info, id: int) -> Optional[UserType]:
        user = fetch_user(id)
        return to_user_type(user) if user else None

    @strawberry.field
    def users(self, info: Info, limit: int = 20) -> List[UserType]:
        results = fetch_users(limit)
        return [to_user_type(u) for u in results]

    @strawberry.field
    def matches(self, info: Info, user_id: int) -> List[MatchType]:
        results = pg_fetchall_namedtuple(
            "SELECT * FROM matches WHERE user1_id = %s OR user2_id = %s",
            [user_id, user_id],
        ) or []
        return [to_match_type(m) for m in results]

    @strawberry.field
    def messages(self, info: Info, match_id: str) -> List[MessageType]:
        results = fetch_messages_for_match(match_id)
        return [to_message_type(m) for m in results]

    @strawberry.field
    def conversation(self, info: Info, match_id: str, limit: int = 100, offset: int = 0) -> List[MessageType]:
        """Fetch messages for a match, ordered by time."""
        results = fetch_messages_for_match(match_id, limit=limit, offset=offset)
        return [to_message_type(m) for m in results]

    @strawberry.field
    def spot_conversation(self, info: Info, spot_id: int) -> Optional[SpotConversationPayload]:
        """Get spot pre-match conversation, counts, and request status for the viewer and spot owner."""
        user_id = get_user_id_from_context(info)
        if not user_id:
            return None
        spot = fetch_spot(spot_id)
        if not spot:
            return None
        other_id = spot.user_id
        if other_id == user_id:
            # Owners can still see conversation if a status exists, but they need a counterpart
            return None
        return build_spot_conversation(spot_id, user_id, other_id)

    @strawberry.field
    def preferences(self, info: Info, user_id: int) -> Optional[UserPreferencesType]:
        pref = fetch_user_preferences(user_id)
        return to_preferences_type(pref) if pref else None

    @strawberry.field
    def my_preferences(self, info: Info) -> Optional[UserPreferencesType]:
        user_id = get_user_id_from_context(info)
        if not user_id:
            return None
        pref = fetch_user_preferences(user_id)
        return to_preferences_type(pref) if pref else None

    @strawberry.field
    def me(self, info: Info) -> Optional[UserType]:
        user_id = get_user_id_from_context(info)
        if not user_id:
            return None
        user = fetch_user(user_id)
        return to_user_type(user) if user else None

    @strawberry.field
    def me_profile(self, info: Info, limit_spots: int = 5) -> Optional[MePayload]:
        user_id = get_user_id_from_context(info)
        if not user_id:
            return None
        user = fetch_user(user_id)
        pref = fetch_user_preferences(user_id)
        spots = fetch_spots_for_user(user_id, limit_spots)
        return MePayload(
            user=to_user_type(user) if user else None,
            preferences=to_preferences_type(pref) if pref else None,
            spots=[to_spot_type(s) for s in spots] if spots else [],
        )

    @strawberry.field
    def spots(self, info: Info, limit: int = 20) -> List[SpotType]:
        user_id = get_user_id_from_context(info)
        if not user_id:
            return []
        results = fetch_spots_for_user(user_id, limit)
        return [to_spot_type(s) for s in results]

    @strawberry.field
    def discoverProfiles(self, info: Info, limit: int = 20) -> List[UserType]:
        """Basic discover feed: returns other users, limited; excludes current user."""
        user_id = get_user_id_from_context(info)
        rows = pg_fetchall_namedtuple(
            """
            SELECT id, name, email, phone_number, bio, gender, dob, profile_photos, profile_photo_url, location_text,
                   interests, languages, looking_for, profession_category, profession_title, height_cm,
                   is_active, is_verified, is_student_verified
            FROM users
            WHERE id != %s
            ORDER BY created_at DESC
            LIMIT %s
            """,
            [user_id or 0, limit],
        ) or []

        pref = fetch_user_preferences(user_id) if user_id else None
        raw_users = list(rows)

        if pref:
            filtered = []
            pref_langs = set(_attr(pref, "languages") or []) if isinstance(_attr(pref, "languages"), list) else set()
            pref_genders = set([g.lower() for g in (_attr(pref, "preferred_genders") or [])]) if isinstance(_attr(pref, "preferred_genders"), list) else set()
            for u in raw_users:
                u_age = age_from_dob(_attr(u, "dob"))
                u_langs = set(_attr(u, "languages") or []) if isinstance(_attr(u, "languages"), list) else set()
                u_gender = str(_attr(u, "gender")).lower() if _attr(u, "gender") else None
                if _attr(pref, "min_age") and u_age is not None and u_age < _attr(pref, "min_age"):
                    continue
                if _attr(pref, "max_age") and u_age is not None and u_age > _attr(pref, "max_age"):
                    continue
                if _attr(pref, "intent") and _attr(u, "looking_for") and str(_attr(pref, "intent")).lower() != str(_attr(u, "looking_for")).lower():
                    continue
                if pref_langs and u_langs and not pref_langs.intersection(u_langs):
                    continue
                if pref_genders and u_gender and u_gender not in pref_genders:
                    continue
                if _attr(pref, "only_verified") and not _attr(u, "is_verified"):
                    continue
                filtered.append(u)
            raw_users = filtered

        return [to_user_type(u) for u in raw_users[:limit]]

    @strawberry.field
    def ranked_discover(self, info: Info, limit: int = 20, offset: int = 0) -> List[UserType]:
        """Discover feed with simple heuristic ranking using preferences and verification/language/age."""
        user_id = get_user_id_from_context(info)
        # Raw SQL path
        rows = pg_execute(
            """
            SELECT id, name, email, phone_number, bio, gender, dob, profile_photos, profile_photo_url, location_text,
                   interests, languages, looking_for, profession_category, profession_title, height_cm,
                   is_active, is_verified, is_student_verified
            FROM users
            WHERE id != %s
            ORDER BY created_at DESC
            LIMIT %s OFFSET %s
            """,
            [user_id or 0, limit * 3, offset],
            fetchall=True,
        ) or []

        raw_users: List[SimpleNamespace] = []
        for row in rows:
            (
                uid, name, email, phone, bio, gender, dob, profile_photos, profile_photo_url, location_text,
                interests, languages, looking_for, prof_cat, prof_title, height_cm,
                is_active, is_verified, is_student_verified
            ) = row
            u = SimpleNamespace(
                id=uid,
                name=name,
                email=email,
                phone_number=phone,
                bio=bio,
                gender=gender,
                dob=dob,
                profile_photos=profile_photos,
                profile_photo_url=profile_photo_url,
                location_text=location_text,
                interests=interests,
                languages=languages,
                looking_for=looking_for,
                profession_category=prof_cat,
                profession_title=prof_title,
                height_cm=height_cm,
                is_active=is_active,
                is_verified=is_verified,
                is_student_verified=is_student_verified,
            )
            raw_users.append(u)

        pref = fetch_user_preferences(user_id) if user_id else None

        pref_genders = set([g.lower() for g in pref.preferred_genders]) if pref and isinstance(pref.preferred_genders, list) else set()
        filtered_scored = []
        for u in raw_users:
            u_age = age_from_dob(getattr(u, "dob", None))
            if pref:
                if pref.min_age and u_age is not None and u_age < pref.min_age:
                    continue
                if pref.max_age and u_age is not None and u_age > pref.max_age:
                    continue
                if pref.intent and u.looking_for and str(pref.intent).lower() != str(u.looking_for).lower():
                    continue
                if pref.languages:
                    u_langs = set(u.languages) if isinstance(u.languages, list) else set()
                    if u_langs and not u_langs.intersection(set(pref.languages)):
                        continue
                if pref.only_verified and not getattr(u, "is_verified", False):
                    continue
                if pref_genders:
                    u_gender = str(u.gender).lower() if getattr(u, "gender", None) else None
                    if u_gender and u_gender not in pref_genders:
                        continue
            filtered_scored.append((score_candidate(pref, u), u))
        filtered_scored.sort(key=lambda x: x[0], reverse=True)
        return [to_user_type(u) for _, u in filtered_scored[:limit]]

    @strawberry.field
    def ranked_discover_with_slate(self, info: Info, limit: int = 20, offset: int = 0, epsilon: float = 0.05) -> List[RankedUserType]:
        """Return ranked users plus slate_id and rank, with epsilon-greedy exploration."""
        user_id = get_user_id_from_context(info)
        slate_id = str(uuid.uuid4())

        pref = fetch_user_preferences(user_id) if user_id else None
        pref_genders = set([g.lower() for g in pref.preferred_genders]) if pref and isinstance(pref.preferred_genders, list) else set()
        filtered_scored = []
        for u in raw_users:
            u_age = age_from_dob(getattr(u, "dob", None))
            if pref:
                if pref.min_age and u_age is not None and u_age < pref.min_age:
                    continue
                if pref.max_age and u_age is not None and u_age > pref.max_age:
                    continue
                if pref.intent and u.looking_for and str(pref.intent).lower() != str(u.looking_for).lower():
                    continue
                if pref.languages:
                    u_langs = set(u.languages) if isinstance(u.languages, list) else set()
                    if u_langs and not u_langs.intersection(set(pref.languages)):
                        continue
                if pref.only_verified and not getattr(u, "is_verified", False):
                    continue
                if pref_genders:
                    u_gender = str(u.gender).lower() if getattr(u, "gender", None) else None
                    if u_gender and u_gender not in pref_genders:
                        continue
            base_score = score_candidate(pref, u)
            if random.random() < epsilon:
                base_score += random.uniform(-0.5, 0.5)
            filtered_scored.append((base_score, u))
        filtered_scored.sort(key=lambda x: x[0], reverse=True)
        ranked = [RankedUserType(user=to_user_type(u), score=s, rank=idx, slate_id=slate_id) for idx, (s, u) in enumerate(filtered_scored[:limit])]
        return ranked

    @strawberry.field
    def playground_ranked_feed(self, info: Info, limit: int = 20, feed_mode: str = "global", epsilon: float = 0.05, tags: Optional[List[str]] = None, radius: Optional[int] = None) -> List[RankedSpotType]:
        """Ranked Playground feed using heuristic + embedding similarity with exploration."""
        user_id = get_user_id_from_context(info)
        slate_id = str(uuid.uuid4())
        pref = fetch_user_preferences(user_id) if user_id else None
        # Load embeddings up front
        user_feat = fetch_user_features(user_id) if user_id else None
        user_vec = to_np(_attr(user_feat, "embedding"))

        user_loc = fetch_user_location(user_id) if user_id else None
        radius_miles = max(1, min(int(radius) if radius is not None else 50, 200))
        qs = []
        scope_filter = "is_global = TRUE OR is_global IS NULL"
        if feed_mode.lower() == "local":
            # Require user location for local feed
            if not user_loc or _attr(user_loc, "latitude") is None or _attr(user_loc, "longitude") is None:
                return []
            raw_local = pg_fetchall_namedtuple(
                """
                SELECT s.*, ul.latitude AS owner_lat, ul.longitude AS owner_lon
                FROM spots s
                JOIN user_locations ul ON ul.user_id = s.user_id
                WHERE COALESCE(s.is_global, FALSE) = FALSE
                ORDER BY s.created_at DESC
                LIMIT %s
                """,
                [limit * 8],
            ) or []
            # Filter by radius miles
            for spot in raw_local:
                dist = haversine_miles(
                    _attr(user_loc, "latitude"),
                    _attr(user_loc, "longitude"),
                    _attr(spot, "owner_lat"),
                    _attr(spot, "owner_lon"),
                )
                if dist is None or dist > radius_miles:
                    continue
                spot = SimpleNamespace(**spot._asdict(), distance_miles=dist)
                qs.append(spot)
        else:
            qs = pg_fetchall_namedtuple(
                """
                SELECT * FROM spots
                WHERE is_global = TRUE OR is_global IS NULL
                ORDER BY created_at DESC
                LIMIT %s
                """,
                [limit * 5],
            ) or []
        spot_vecs = {
            _attr(se, "spot_id"): to_np(_attr(se, "embedding"))
            for se in fetch_spot_embeddings([_attr(s, "id") for s in qs])
        }
        pref_genders = set([g.lower() for g in _attr(pref, "preferred_genders")]) if pref and isinstance(_attr(pref, "preferred_genders"), list) else set()
        pref_langs = set(_attr(pref, "languages")) if pref and isinstance(_attr(pref, "languages"), list) else set()

        # Build candidate features
        candidates = []
        for spot in qs:
            owner = fetch_user(_attr(spot, "user_id"))
            if not owner or (user_id and _attr(owner, "id") == user_id):
                continue
            if feed_mode.lower() == "local" and _attr(spot, "is_global", True):
                continue
            if feed_mode.lower() == "global" and not _attr(spot, "is_global", True):
                continue
            if tags:
                spot_tags = set(_attr(spot, "tags") or [])
                if not spot_tags.intersection(set(tags)):
                    continue
            # Distance-based score boost for local feed
            dist_score = 0.5
            dist_val = _attr(spot, "distance_miles")
            if feed_mode.lower() == "local" and dist_val is not None:
                dist_score = max(0.1, 1.0 - (float(dist_val) / float(radius_miles)))
            o_age = age_from_dob(_attr(owner, "dob"))
            if pref:
                if _attr(pref, "min_age") and o_age is not None and o_age < _attr(pref, "min_age"):
                    continue
                if _attr(pref, "max_age") and o_age is not None and o_age > _attr(pref, "max_age"):
                    continue
                if _attr(pref, "intent") and _attr(owner, "looking_for") and str(_attr(pref, "intent")).lower() != str(_attr(owner, "looking_for")).lower():
                    continue
                if pref_langs:
                    o_langs = set(_attr(owner, "languages") or []) if isinstance(_attr(owner, "languages"), list) else set()
                    if o_langs and not o_langs.intersection(pref_langs):
                        continue
                if _attr(pref, "only_verified") and not _attr(owner, "is_verified"):
                    continue
                if pref_genders:
                    o_gender = str(_attr(owner, "gender")).lower() if _attr(owner, "gender") else None
                    if o_gender and o_gender not in pref_genders:
                        continue
            cand = build_spot_candidate_features(spot, owner, user_vec, spot_vecs)
            cand["distance_score"] = dist_score
            cand["spot_obj"] = spot
            candidates.append(cand)

        bandit = get_bandit()
        ctx = {
            "hour_of_day": datetime.utcnow().hour,
            "day_of_week": datetime.utcnow().weekday(),
            "is_weekend": datetime.utcnow().weekday() >= 5,
        }
        # bandit.score_candidates previously used SQLAlchemy session; pass None to use stateless scoring
        scored = bandit.score_candidates(None, candidates, ctx, user_id=user_id, exploration_rate=epsilon)
        scored.sort(key=lambda x: x.get("bandit_score", 0), reverse=True)
        top = scored[:limit]

        ranked: List[RankedSpotType] = []
        for idx, cand in enumerate(top):
            sp = cand["spot_obj"]
            s = cand.get("bandit_score", 0.0)
            try:
                # Impression logging disabled in raw-SQL path to avoid ORM usage
                pass
            except Exception:
                pass
            ranked.append(RankedSpotType(spot=to_spot_type(sp), score=s, rank=idx, slate_id=slate_id))
        return ranked

    @strawberry.field
    def profession_options(self, category: Optional[str] = None) -> List[str]:
        """Return profession options; currently mocked."""
        options = {
            "technology": ["Software Engineer", "Product Manager", "Data Scientist", "Designer"],
            "healthcare": ["Doctor", "Nurse", "Pharmacist", "Therapist"],
            "finance": ["Accountant", "Analyst", "Banker", "Trader"],
            "education": ["Teacher", "Professor", "Tutor", "Coach"],
            "creative": ["Writer", "Photographer", "Musician", "Artist"],
            "other": ["Student", "Entrepreneur", "Consultant", "Freelancer"]
        }
        if category and category.lower() in options:
            return options[category.lower()]
        # default: flatten unique options
        seen = set()
        merged = []
        for vals in options.values():
            for v in vals:
                if v not in seen:
                    seen.add(v)
                    merged.append(v)
        return merged


@strawberry.type
class Mutation:
    @strawberry.mutation
    def save_preferences(self, info: Info, input: PreferencesInput) -> UserPreferencesType:
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")
        pref_row = pg_execute(
            """
            INSERT INTO user_preferences (user_id, min_age, max_age, intent, languages, preferred_genders, only_verified, distance_miles, max_distance, updated_at, created_at)
            VALUES (%s, COALESCE(%s, 18), COALESCE(%s, 50), %s, %s, %s, COALESCE(%s, FALSE), %s, %s, NOW(), NOW())
            ON CONFLICT (user_id) DO UPDATE
            SET min_age = EXCLUDED.min_age,
                max_age = EXCLUDED.max_age,
                intent = EXCLUDED.intent,
                languages = EXCLUDED.languages,
                preferred_genders = EXCLUDED.preferred_genders,
                only_verified = EXCLUDED.only_verified,
                distance_miles = EXCLUDED.distance_miles,
                max_distance = EXCLUDED.max_distance,
                updated_at = NOW()
            RETURNING user_id, min_age, max_age, intent, languages, preferred_genders, only_verified, distance_miles, max_distance
            """,
            [
                user_id,
                input.min_age,
                input.max_age,
                input.intent,
                Json(list(input.languages)) if input.languages is not None else None,
                Json(list(input.preferred_genders)) if input.preferred_genders is not None else None,
                input.only_verified,
                input.distance_miles,
                input.distance_miles if input.distance_miles is not None else None,
            ],
            fetchone=True,
        )
        if not pref_row:
            raise Exception("Could not save preferences")
        (
            _, min_age, max_age, intent, languages, preferred_genders, only_verified, distance_miles, max_distance
        ) = pref_row
        ns = SimpleNamespace(
            min_age=min_age,
            max_age=max_age,
            intent=intent,
            languages=languages,
            preferred_genders=preferred_genders,
            only_verified=only_verified,
            distance_miles=distance_miles,
            max_distance=max_distance,
        )
        return to_preferences_type(ns)

    @strawberry.mutation
    def send_spot_message(self, info: Info, spot_id: int, text: str) -> SpotConversationPayload:
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")
        spot = fetch_spot(spot_id)
        if not spot:
            raise Exception("Spot not found")
        other_id = spot.user_id
        if other_id == user_id:
            raise Exception("Cannot message yourself on your own spot")

        # Upsert status
        a_id, b_id = normalize_pair(user_id, other_id)
        status = fetch_spot_pair_status(spot_id, a_id, b_id)
        a_count = _attr(status, "a_count", 0)
        b_count = _attr(status, "b_count", 0)
        req_status = _attr(status, "request_status", "none")
        match_id = _attr(status, "match_id")
        if user_id == a_id:
            a_count += 1
        else:
            b_count += 1
        eligible = a_count >= 5 and b_count >= 5
        upsert_spot_pair_status(spot_id, a_id, b_id, a_count, b_count, req_status, match_id)
        pg_execute(
            "INSERT INTO spot_messages (spot_id, sender_id, text, created_at) VALUES (%s, %s, %s, NOW())",
            [spot_id, user_id, text],
        )
        payload = build_spot_conversation(spot_id, user_id, other_id)
        broadcast_spot_conversation(spot_id, payload)
        return payload

    @strawberry.mutation
    def request_profile(self, info: Info, spot_id: int) -> SpotConversationPayload:
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")
        spot = fetch_spot(spot_id)
        if not spot:
            raise Exception("Spot not found")
        other_id = spot.user_id
        if other_id == user_id:
            raise Exception("Cannot request your own profile")
        a_id, b_id = normalize_pair(user_id, other_id)
        status = fetch_spot_pair_status(spot_id, a_id, b_id)
        if not status or not _attr(status, "eligible_for_match"):
            raise Exception("Not eligible yet; send more messages")
        if _attr(status, "request_status") not in ("none", "requested", "received"):
            return build_spot_conversation(spot_id, user_id, other_id)
        # Mark directional request
        req_status = "requested" if user_id == a_id else "received"
        upsert_spot_pair_status(
            spot_id,
            a_id,
            b_id,
            _attr(status, "a_count", 0),
            _attr(status, "b_count", 0),
            req_status,
            _attr(status, "match_id"),
        )
        payload = build_spot_conversation(spot_id, user_id, other_id)
        broadcast_spot_conversation(spot_id, payload)
        return payload

    @strawberry.mutation
    def accept_profile_request(self, info: Info, spot_id: int) -> SpotConversationPayload:
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")
        spot = fetch_spot(spot_id)
        if not spot:
            raise Exception("Spot not found")
        other_id = spot.user_id
        if other_id == user_id:
            raise Exception("Cannot accept your own request")
        a_id, b_id = normalize_pair(user_id, other_id)
        status = fetch_spot_pair_status(spot_id, a_id, b_id)
        if not status or not _attr(status, "eligible_for_match"):
            raise Exception("Not eligible")
        # Only the receiver of a pending request can accept
        if user_id == a_id and _attr(status, "request_status") != "received":
            raise Exception("No incoming request to accept")
        if user_id == b_id and _attr(status, "request_status") != "requested":
            raise Exception("No incoming request to accept")

        # Create match if absent
        match_id = _attr(status, "match_id")
        if not match_id:
            match_id = str(uuid.uuid4())
            pg_execute(
                """
                INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, match_reason, created_at, updated_at)
                VALUES (%s, %s, %s, TRUE, TRUE, TRUE, 'active', 'spot', NOW(), NOW())
                ON CONFLICT (id) DO NOTHING
                """,
                [match_id, a_id, b_id],
            )
        upsert_spot_pair_status(
            spot_id,
            a_id,
            b_id,
            _attr(status, "a_count", 0),
            _attr(status, "b_count", 0),
            "accepted",
            match_id,
        )
        payload = build_spot_conversation(spot_id, user_id, other_id)
        broadcast_spot_conversation(spot_id, payload)
        return payload

    @strawberry.mutation
    def log_event(self, info: Info, event_type: str, target_user_id: int, metadata: Optional[str] = None, slate_id: Optional[str] = None, rank: Optional[int] = None, surface: Optional[str] = None, reward: Optional[float] = None) -> bool:
        """Lightweight logging of interactions/impressions to support ranking."""
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")
        db = get_db(info)
        evt = InteractionEvent(
            user_id=user_id,
            target_user_id=target_user_id,
            event_type=event_type,
            event_metadata={"meta": metadata} if metadata else None,
            slate_id=slate_id,
            rank=rank,
            surface=surface,
            reward=reward,
        )
        db.add(evt)
        db.commit()
        return True
    @strawberry.mutation
    def send_message(self, info: Info, match_id: str, sender_id: int, receiver_id: int, content: str) -> MessageType:
        # Raw PG insert if available
        abusive_terms = {"hate", "kill", "slur", "abuse", "harass"}
        is_abusive = any(term in content.lower() for term in abusive_terms)
        if pg_pool:
            try:
                row = pg_execute(
                    """
                    INSERT INTO messages (match_id, sender_id, receiver_id, content, message_type, is_flagged, moderation_status, created_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, NOW())
                    RETURNING id
                    """,
                    [match_id, sender_id, receiver_id, content, "text", is_abusive, "pending" if is_abusive else None],
                    fetchone=True,
                )
                mid = row[0] if row else None
                return MessageType(
                    id=mid or 0,
                    match_id=match_id,
                    sender_id=sender_id,
                    receiver_id=receiver_id,
                    content=content,
                    message_type="text",
                    is_read=False,
                )
            except Exception as exc:
                print(f"PG send_message failed, fallback to SQLAlchemy: {exc}")

        raise Exception("Failed to send message")

    @strawberry.mutation
    def send_chat_message(self, info: Info, match_id: str, content: str) -> MessageType:
        """Auth-aware message sender: only participants in a mutual match can post."""
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")

        match = fetch_match(match_id)
        if not match:
            raise Exception("Match not found")
        if user_id not in (_attr(match, "user1_id"), _attr(match, "user2_id")):
            raise Exception("Not a participant in this match")
        if not _attr(match, "is_mutual_match"):
            raise Exception("Chat unlocked only after mutual match")

        receiver_id = _attr(match, "user2_id") if _attr(match, "user1_id") == user_id else _attr(match, "user1_id")

        abusive_terms = {"hate", "kill", "slur", "abuse", "harass"}
        is_abusive = any(term in content.lower() for term in abusive_terms)

        row = pg_execute(
            """
            INSERT INTO messages (match_id, sender_id, receiver_id, content, message_type, is_flagged, moderation_status, created_at)
            VALUES (%s, %s, %s, %s, %s, %s, %s, NOW())
            RETURNING id, match_id, sender_id, receiver_id, content, message_type, is_flagged, created_at
            """,
            [
                match_id,
                user_id,
                receiver_id,
                content,
                "text",
                is_abusive,
                "pending" if is_abusive else None,
            ],
            fetchone=True,
        )
        if not row:
            raise Exception("Failed to send message")
        mid, mid_match, sid, rid, c_text, m_type, flagged, created_at = row
        msg = to_message_type(
            SimpleNamespace(
                id=mid,
                match_id=mid_match,
                sender_id=sid,
                receiver_id=rid,
                content=c_text,
                message_type=m_type,
                is_read=False,
                is_flagged=flagged,
                created_at=created_at,
            )
        )
        # Fan-out to subscribers
        broadcast_message(match_id, msg)
        return msg

    @strawberry.mutation
    def verify_otp(self, info: Info, phone_number: str, otp: str) -> AuthPayload:
        # Mock verification: accept only OTP 1234
        if otp != "1234":
            raise Exception("Invalid OTP")
        # In a real implementation, generate JWT/access token and fetch user
        return AuthPayload(
            access_token=f"mock-token-for-{phone_number}",
            user_id=1,
            is_profile_complete=False,
            token_type="bearer",
        )

    @strawberry.mutation
    def add_profession_option(self, info: Info, category: str, name: str) -> bool:
        # Mock: accept and return True. Persist if/when a model/table is added.
        return True

    @strawberry.mutation
    def send_otp(self, phone_number: str) -> OtpResponse:
        # Mock OTP sender; replace with real SMS gateway as needed
        return OtpResponse(message="OTP sent", success=True)

    @strawberry.mutation
    async def update_profile(
        self,
        info: Info,
        name: str,
        dob: str,
        gender: str,
        bio: str,
        location: str,
        looking_for: str,
        interests: List[str],
        languages: List[str],
        height_cm: Optional[int] = None,
        profession_category: Optional[str] = None,
        profession_title: Optional[str] = None,
        profile_photo_1: Optional[Upload] = None,
        profile_photo_2: Optional[Upload] = None,
        profile_photo_3: Optional[Upload] = None,
    ) -> bool:
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")

        # Save uploads to local /uploads and return URLs
        async def save_upload(file: Upload) -> str:
            uploads_dir = os.path.join(os.getcwd(), "uploads")
            os.makedirs(uploads_dir, exist_ok=True)
            ext = os.path.splitext(file.filename or "")[1] or ".jpg"
            filename = f"{uuid.uuid4().hex}{ext}"
            dest = os.path.join(uploads_dir, filename)
            content = file.read()
            if asyncio.iscoroutine(content):
                content = await content
            with open(dest, "wb") as f:
                f.write(content)
            return f"/uploads/{filename}"

        photo_urls = [None, None, None]
        embeddings: List[Optional[np.ndarray]] = [None, None, None]
        uploads = [profile_photo_1, profile_photo_2, profile_photo_3]
        for idx, file in enumerate(uploads):
            if file:
                if not SKIP_FACE_CHECK:
                    photo_bytes = await read_upload_bytes(file)
                    embedding = extract_face_embedding(photo_bytes)
                    if embedding is None:
                        raise Exception(f"Photo {idx+1} must contain exactly one clear face")
                    embeddings[idx] = embedding
                photo_urls[idx] = await save_upload(file)

        # Persist basic profile fields
        user = fetch_user(user_id)
        if not user:
            return False

        # Parse dob
        parsed_dob = None
        for fmt in ("%Y-%m-%d", "%Y/%m/%d", "%b %d %Y", "%a %b %d %Y", "%Y-%m-%dT%H:%M:%S"):
            try:
                parsed_dob = datetime.strptime(dob, fmt).date()
                break
            except Exception:
                continue
        if parsed_dob is None:
            try:
                parsed_dob = datetime.fromisoformat(dob).date()
            except Exception:
                parsed_dob = None

        pg_execute(
            """
            UPDATE users
            SET name = %s,
                gender = %s,
                bio = %s,
                location_text = %s,
                interests = %s,
                languages = %s,
                looking_for = %s,
                profession_category = %s,
                profession_title = %s,
                height_cm = %s,
                dob = %s,
                profile_photo_url = %s,
                profile_photos = %s,
                profile_photo_1 = %s,
                profile_photo_2 = %s,
                profile_photo_3 = %s,
                is_profile_complete = TRUE,
                updated_at = NOW()
            WHERE id = %s
            """,
            [
                name,
                gender,
                bio,
                location,
                Json(interests or []),
                Json(languages or []),
                looking_for,
                profession_category,
                profession_title,
                height_cm,
                parsed_dob,
                ",".join([u for u in photo_urls if u]),
                Json([u for u in photo_urls if u]),
                photo_urls[0],
                photo_urls[1],
                photo_urls[2],
                user_id,
            ],
        )

        # Face verification: if user has a stored embedding, compare; else if we have an embedding from photo_1 set it as reference
        if pg_pool:
            try:
                row = pg_execute(
                    "SELECT verified_face_embedding FROM users WHERE id = %s",
                    [user_id],
                    fetchone=True,
                )
                ref = row[0] if row else None
            except Exception:
                ref = None
        else:
            ref = getattr(user, "verified_face_embedding", None)

        if ref:
            ref_vec = np.array(ref, dtype=np.float32)
            for idx, emb in enumerate(embeddings):
                if emb is None:
                    continue
                score = cosine_similarity(emb, ref_vec)
                if score < 0.5:
                    raise Exception(f"Photo {idx+1} face does not match verified selfie (score {score:.2f})")
        else:
            # set reference from first available embedding
            for emb in embeddings:
                if emb is not None:
                    ref = emb.tolist()
                    break

        # Normalize existing photos and merge with new uploads
        existing_photos: List[str] = []
        if _attr(user, "profile_photos"):
            existing_photos = [p for p in _attr(user, "profile_photos") or [] if p]
        elif _attr(user, "profile_photo_url"):
            existing_photos = [p for p in str(_attr(user, "profile_photo_url")).split(",") if p]

        merged_photos = [p for p in photo_urls if p] or existing_photos
        photo1 = photo_urls[0] or _attr(user, "profile_photo_1")
        photo2 = photo_urls[1] or _attr(user, "profile_photo_2")
        photo3 = photo_urls[2] or _attr(user, "profile_photo_3")

        # Persist profile with raw SQL to avoid ORM mutability issues
        pg_execute(
            """
            UPDATE users
            SET name = %s,
                gender = %s,
                bio = %s,
                location_text = %s,
                interests = %s,
                languages = %s,
                looking_for = %s,
                profession_category = %s,
                profession_title = %s,
                height_cm = %s,
                profile_photo_1 = %s,
                profile_photo_2 = %s,
                profile_photo_3 = %s,
                profile_photos = %s,
                profile_photo_url = %s,
                dob = %s,
                is_profile_complete = TRUE,
                updated_at = NOW()
            WHERE id = %s
            """,
            [
                name,
                gender,
                bio,
                location,
                Json(interests),
                Json(languages),
                looking_for,
                profession_category,
                profession_title,
                height_cm,
                photo1,
                photo2,
                photo3,
                Json(merged_photos),
                ",".join(merged_photos) if merged_photos else None,
                parsed_dob,
                user_id,
            ],
        )

        # Persist verified_face_embedding if we captured one and none existed
        if ref and pg_pool:
            try:
                pg_execute(
                    "UPDATE users SET verified_face_embedding = %s, face_verified_at = NOW() WHERE id = %s",
                    [Json(ref), user_id],
                )
            except Exception:
                pass

        return True

    @strawberry.mutation
    async def verify_selfie(self, info: Info, selfie: Upload) -> bool:
        """Enroll a selfie as the verified face embedding."""
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")
        ref = None
        if not SKIP_FACE_CHECK:
            photo_bytes = await read_upload_bytes(selfie)
            embedding = extract_face_embedding(photo_bytes)
            if embedding is None:
                raise Exception("Selfie must contain exactly one clear face")
            ref = embedding.tolist()
        if pg_pool:
            try:
                if ref is not None:
                    pg_execute(
                        "UPDATE users SET verified_face_embedding = %s, face_verified_at = NOW() WHERE id = %s",
                        [Json(ref), user_id],
                    )
                return True
            except Exception as exc:
                raise Exception(f"Failed to store selfie: {exc}")
        raise Exception("Postgres unavailable for selfie storage")

@strawberry.type
class Subscription:
    @strawberry.subscription
    async def message_stream(self, info: Info, match_id: str) -> MessageType:
        """Realtime messages for a match_id. Requires auth and match membership."""
        user_id = get_user_id_from_context(info)
        if not user_id:
            raise Exception("Unauthorized")
        match = fetch_match(match_id)
        if not match or user_id not in (_attr(match, "user1_id"), _attr(match, "user2_id")):
            raise Exception("Not part of this match")
        if not _attr(match, "is_mutual_match"):
            raise Exception("Chat unlocked only after mutual match")

        queue: asyncio.Queue = asyncio.Queue()
        message_subscribers[match_id].append(queue)
        try:
            while True:
                msg: MessageType = await queue.get()
                yield msg
        finally:
            try:
                message_subscribers[match_id].remove(queue)
            except Exception:
                pass

    @strawberry.subscription
    async def spot_conversation_updated(self, info: Info, spot_id: int) -> SpotConversationPayload:
        """Realtime updates are disabled to keep spot conversations private."""
        raise Exception("Spot conversation realtime updates are disabled for privacy")


schema = strawberry.Schema(
    query=Query,
    mutation=Mutation,
    subscription=Subscription,
    config=StrawberryConfig(auto_camel_case=False),
)
