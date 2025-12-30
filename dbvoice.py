# database.py - Separate database configuration
from sqlalchemy import create_engine, Column, Integer, String, DateTime, Boolean, Text
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.orm import sessionmaker
from datetime import datetime
import os

# Database URL - use PostgreSQL in production
DATABASE_URL = os.getenv("DATABASE_URL", "sqlite:///./voice_dating.db")

if DATABASE_URL.startswith("postgres://"):
    DATABASE_URL = DATABASE_URL.replace("postgres://", "postgresql://", 1)

engine = create_engine(DATABASE_URL, connect_args={"check_same_thread": False} if "sqlite" in DATABASE_URL else {})
SessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=engine)
Base = declarative_base()

# Database Models
class User(Base):
    __tablename__ = "users"
    
    id = Column(Integer, primary_key=True, index=True)
    phone_number = Column(String, unique=True, index=True, nullable=False)
    name = Column(String, nullable=True)
    dob = Column(String, nullable=True)
    gender = Column(String, nullable=True)
    bio = Column(Text, nullable=True)
    voice_intro_url = Column(String, nullable=True)
    profile_photo_1 = Column(String, nullable=True)
    profile_photo_2 = Column(String, nullable=True)
    profile_photo_3 = Column(String, nullable=True)
    is_profile_complete = Column(Boolean, default=False)
    is_active = Column(Boolean, default=True)
    created_at = Column(DateTime, default=datetime.utcnow)
    updated_at = Column(DateTime, default=datetime.utcnow, onupdate=datetime.utcnow)

class OTP(Base):
    __tablename__ = "otps"
    
    id = Column(Integer, primary_key=True, index=True)
    phone_number = Column(String, index=True)
    otp = Column(String)
    is_verified = Column(Boolean, default=False)
    expires_at = Column(DateTime)
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

class TextMessage(Base):
    __tablename__ = "text_messages"
    
    id = Column(Integer, primary_key=True, index=True)
    sender_id = Column(Integer, index=True)
    receiver_id = Column(Integer, index=True)
    match_id = Column(String, index=True)
    content = Column(Text)
    is_read = Column(Boolean, default=False)
    created_at = Column(DateTime, default=datetime.utcnow)

class Match(Base):
    __tablename__ = "matches"
    
    id = Column(String, primary_key=True, index=True)
    user1_id = Column(Integer, index=True)
    user2_id = Column(Integer, index=True)
    status = Column(String, default="active")  # active, blocked, reported
    last_message_at = Column(DateTime)
    created_at = Column(DateTime, default=datetime.utcnow)

# Dependency to get DB session
def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()

# Create tables
def create_tables():
    Base.metadata.create_all(bind=engine)