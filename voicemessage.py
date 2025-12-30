# main.py - FastAPI Backend for Voice Messaging
from fastapi import FastAPI, File, UploadFile, Form, Depends, HTTPException, status
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from fastapi.staticfiles import StaticFiles
from fastapi.middleware.cors import CORSMiddleware
from sqlalchemy import create_engine, Column, Integer, String, DateTime, Boolean, Text
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker, Session
from pydantic import BaseModel
from datetime import datetime, timedelta
import os
import uuid
import shutil
import jwt
from passlib.context import CryptContext
from typing import List, Optional
import aiofiles
from mutagen import File as MutagenFile
import logging

# Database setup
SQLALCHEMY_DATABASE_URL = "sqlite:///./voice_dating.db"
engine = create_engine(SQLALCHEMY_DATABASE_URL, connect_args={"check_same_thread": False})
SessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=engine)
Base = declarative_base()

# FastAPI app
app = FastAPI(title="Voice Dating API", version="1.0.0")

# CORS middleware
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],  # Configure this for production
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Security
security = HTTPBearer()
pwd_context = CryptContext(schemes=["bcrypt"], deprecated="auto")
SECRET_KEY = "your-secret-key-change-in-production"
ALGORITHM = "HS256"
ACCESS_TOKEN_EXPIRE_MINUTES = 30

# Create uploads directory
os.makedirs("uploads/voice_messages", exist_ok=True)
os.makedirs("uploads/profile_photos", exist_ok=True)

# Serve static files
app.mount("/uploads", StaticFiles(directory="uploads"), name="uploads")

# Database Models
class User(Base):
    __tablename__ = "users"
    
    id = Column(Integer, primary_key=True, index=True)
    phone_number = Column(String, unique=True, index=True)
    name = Column(String)
    dob = Column(String)
    gender = Column(String)
    bio = Column(Text)
    voice_intro_url = Column(String)
    profile_photo_1 = Column(String)
    profile_photo_2 = Column(String)
    profile_photo_3 = Column(String)
    is_active = Column(Boolean, default=True)
    created_at = Column(DateTime, default=datetime.utcnow)

class VoiceMessage(Base):
    __tablename__ = "voice_messages"
    
    id = Column(Integer, primary_key=True, index=True)
    sender_id = Column(Integer, index=True)
    receiver_id = Column(Integer, index=True)
    match_id = Column(String, index=True)
    voice_url = Column(String)
    duration = Column(Integer)  # in seconds
    is_played = Column(Boolean, default=False)
    created_at = Column(DateTime, default=datetime.utcnow)

class Match(Base):
    __tablename__ = "matches"
    
    id = Column(String, primary_key=True, index=True)
    user1_id = Column(Integer, index=True)
    user2_id = Column(Integer, index=True)
    status = Column(String, default="active")  # active, blocked, reported
    last_message_at = Column(DateTime)
    created_at = Column(DateTime, default=datetime.utcnow)

class TextMessage(Base):
    __tablename__ = "text_messages"
    
    id = Column(Integer, primary_key=True, index=True)
    sender_id = Column(Integer, index=True)
    receiver_id = Column(Integer, index=True)
    match_id = Column(String, index=True)
    content = Column(Text)
    is_read = Column(Boolean, default=False)
    created_at = Column(DateTime, default=datetime.utcnow)

# Create tables
Base.metadata.create_all(bind=engine)

# Pydantic models
class VoiceMessageResponse(BaseModel):
    id: int
    sender_id: int
    receiver_id: int
    voice_url: str
    duration: int
    is_played: bool
    created_at: datetime

class MessageResponse(BaseModel):
    id: int
    sender_id: int
    receiver_id: int
    content: Optional[str] = None
    voice_url: Optional[str] = None
    duration: Optional[int] = None
    message_type: str  # 'text' or 'voice'
    created_at: datetime

# Dependency
def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()

# Auth functions
def create_access_token(data: dict, expires_delta: Optional[timedelta] = None):
    to_encode = data.copy()
    if expires_delta:
        expire = datetime.utcnow() + expires_delta
    else:
        expire = datetime.utcnow() + timedelta(minutes=15)
    to_encode.update({"exp": expire})
    encoded_jwt = jwt.encode(to_encode, SECRET_KEY, algorithm=ALGORITHM)
    return encoded_jwt

def verify_token(credentials: HTTPAuthorizationCredentials = Depends(security)):
    token = credentials.credentials
    try:
        payload = jwt.decode(token, SECRET_KEY, algorithms=[ALGORITHM])
        phone_number: str = payload.get("sub")
        if phone_number is None:
            raise HTTPException(status_code=401, detail="Invalid token")
        return phone_number
    except jwt.PyJWTError:
        raise HTTPException(status_code=401, detail="Invalid token")

def get_current_user(db: Session = Depends(get_db), phone_number: str = Depends(verify_token)):
    user = db.query(User).filter(User.phone_number == phone_number).first()
    if user is None:
        raise HTTPException(status_code=401, detail="User not found")
    return user

# Utility functions
def get_audio_duration(file_path: str) -> int:
    """Get duration of audio file in seconds"""
    try:
        audio_file = MutagenFile(file_path)
        if audio_file is not None:
            return int(audio_file.info.length)
        return 0
    except Exception:
        return 0

def validate_audio_file(file: UploadFile) -> bool:
    """Validate audio file type and size"""
    allowed_types = ['audio/mpeg', 'audio/mp4', 'audio/wav', 'audio/m4a']
    max_size = 10 * 1024 * 1024  # 10MB
    
    if file.content_type not in allowed_types:
        return False
    
    # Note: file.size might not be available in all cases
    return True

# API Endpoints

@app.post("/send-voice-message")
async def send_voice_message(
    match_id: str = Form(...),
    voice_message: UploadFile = File(...),
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
):
    """Send a voice message to a matched user"""
    
    # Validate match exists and user is part of it
    match = db.query(Match).filter(Match.id == match_id).first()
    if not match:
        raise HTTPException(status_code=404, detail="Match not found")
    
    if current_user.id not in [match.user1_id, match.user2_id]:
        raise HTTPException(status_code=403, detail="Access denied to this match")
    
    # Determine receiver
    receiver_id = match.user2_id if current_user.id == match.user1_id else match.user1_id
    
    # Validate audio file
    if not validate_audio_file(voice_message):
        raise HTTPException(status_code=400, detail="Invalid audio file")
    
    try:
        # Generate unique filename
        file_extension = voice_message.filename.split('.')[-1] if voice_message.filename else 'mp4'
        file_name = f"voice_{uuid.uuid4()}.{file_extension}"
        file_path = f"uploads/voice_messages/{file_name}"
        
        # Save file
        async with aiofiles.open(file_path, 'wb') as f:
            content = await voice_message.read()
            await f.write(content)
        
        # Get audio duration
        duration = get_audio_duration(file_path)
        
        # Save to database
        voice_msg = VoiceMessage(
            sender_id=current_user.id,
            receiver_id=receiver_id,
            match_id=match_id,
            voice_url=f"/uploads/voice_messages/{file_name}",
            duration=duration
        )
        
        db.add(voice_msg)
        db.commit()
        db.refresh(voice_msg)
        
        # Update match last message time
        match.last_message_at = datetime.utcnow()
        db.commit()
        
        return {
            "message_id": voice_msg.id,
            "voice_url": voice_msg.voice_url,
            "duration": duration,
            "status": "sent"
        }
        
    except Exception as e:
        # Clean up file if database save failed
        if os.path.exists(file_path):
            os.remove(file_path)
        raise HTTPException(status_code=500, detail=f"Failed to send voice message: {str(e)}")

@app.post("/send-text-message")
async def send_text_message(
    match_id: str = Form(...),
    content: str = Form(...),
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
):
    """Send a text message to a matched user"""
    
    # Check if user can send text messages (after 5 voice messages exchanged)
    voice_count = db.query(VoiceMessage).filter(
        VoiceMessage.match_id == match_id
    ).count()
    
    if voice_count < 5:
        raise HTTPException(
            status_code=403, 
            detail="Text messaging unlocked after 5 voice messages exchanged"
        )
    
    # Validate match exists and user is part of it
    match = db.query(Match).filter(Match.id == match_id).first()
    if not match:
        raise HTTPException(status_code=404, detail="Match not found")
    
    if current_user.id not in [match.user1_id, match.user2_id]:
        raise HTTPException(status_code=403, detail="Access denied to this match")
    
    # Determine receiver
    receiver_id = match.user2_id if current_user.id == match.user1_id else match.user1_id
    
    # Validate content
    if not content.strip() or len(content) > 1000:
        raise HTTPException(status_code=400, detail="Invalid message content")
    
    # Save to database
    text_msg = TextMessage(
        sender_id=current_user.id,
        receiver_id=receiver_id,
        match_id=match_id,
        content=content.strip()
    )
    
    db.add(text_msg)
    db.commit()
    db.refresh(text_msg)
    
    # Update match last message time
    match.last_message_at = datetime.utcnow()
    db.commit()
    
    return {
        "message_id": text_msg.id,
        "content": text_msg.content,
        "status": "sent"
    }

@app.get("/chat/{match_id}")
async def get_chat_messages(
    match_id: str,
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
):
    """Get all messages for a specific match"""
    
    # Validate match access
    match = db.query(Match).filter(Match.id == match_id).first()
    if not match:
        raise HTTPException(status_code=404, detail="Match not found")
    
    if current_user.id not in [match.user1_id, match.user2_id]:
        raise HTTPException(status_code=403, detail="Access denied to this match")
    
    # Get voice messages
    voice_messages = db.query(VoiceMessage).filter(
        VoiceMessage.match_id == match_id
    ).order_by(VoiceMessage.created_at).all()
    
    # Get text messages
    text_messages = db.query(TextMessage).filter(
        TextMessage.match_id == match_id
    ).order_by(TextMessage.created_at).all()
    
    # Combine and sort messages
    all_messages = []
    
    for vm in voice_messages:
        all_messages.append({
            "id": vm.id,
            "sender_id": vm.sender_id,
            "receiver_id": vm.receiver_id,
            "voice_url": vm.voice_url,
            "duration": vm.duration,
            "message_type": "voice",
            "created_at": vm.created_at
        })
    
    for tm in text_messages:
        all_messages.append({
            "id": tm.id,
            "sender_id": tm.sender_id,
            "receiver_id": tm.receiver_id,
            "content": tm.content,
            "message_type": "text",
            "created_at": tm.created_at
        })
    
    # Sort by timestamp
    all_messages.sort(key=lambda x: x["created_at"])
    
    # Get other user info
    other_user_id = match.user2_id if current_user.id == match.user1_id else match.user1_id
    other_user = db.query(User).filter(User.id == other_user_id).first()
    
    return {
        "messages": all_messages,
        "user": {
            "id": other_user.id,
            "name": other_user.name,
            "profile_photo_1": other_user.profile_photo_1
        },
        "can_send_text": len(voice_messages) >= 5
    }

@app.post("/voice-intro")
async def upload_voice_intro(
    voice_intro: UploadFile = File(...),
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
):
    """Upload 15-second voice introduction"""
    
    # Validate audio file
    if not validate_audio_file(voice_intro):
        raise HTTPException(status_code=400, detail="Invalid audio file")
    
    try:
        # Generate unique filename
        file_extension = voice_intro.filename.split('.')[-1] if voice_intro.filename else 'mp4'
        file_name = f"intro_{current_user.id}_{uuid.uuid4()}.{file_extension}"
        file_path = f"uploads/voice_messages/{file_name}"
        
        # Save file
        async with aiofiles.open(file_path, 'wb') as f:
            content = await voice_intro.read()
            await f.write(content)
        
        # Check duration (should be around 15 seconds)
        duration = get_audio_duration(file_path)
        if duration > 20:  # Allow some flexibility
            os.remove(file_path)
            raise HTTPException(status_code=400, detail="Voice intro must be 15 seconds or less")
        
        # Update user profile
        current_user.voice_intro_url = f"/uploads/voice_messages/{file_name}"
        db.commit()
        
        return {
            "voice_intro_url": current_user.voice_intro_url,
            "duration": duration,
            "status": "uploaded"
        }
        
    except Exception as e:
        if os.path.exists(file_path):
            os.remove(file_path)
        raise HTTPException(status_code=500, detail=f"Failed to upload voice intro: {str(e)}")

@app.post("/mark-voice-played/{message_id}")
async def mark_voice_played(
    message_id: int,
    current_user: User = Depends(get_current_user),
    db: Session = Depends(get_db)
):
    """Mark voice message as played"""
    
    voice_msg = db.query(VoiceMessage).filter(
        VoiceMessage.id == message_id,
        VoiceMessage.receiver_id == current_user.id
    ).first()
    
    if not voice_msg:
        raise HTTPException(status_code=404, detail="Voice message not found")
    
    voice_msg.is_played = True
    db.commit()
    
    return {"status": "marked_played"}

@app.get("/health")
async def health_check():
    """Health check endpoint"""
    return {
        "status": "healthy",
        "timestamp": datetime.utcnow().isoformat(),
        "version": "1.0.0"
    }

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)