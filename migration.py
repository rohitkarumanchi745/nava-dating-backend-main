"""
Simple migration script to add new tables while keeping existing users
Run this once after updating models.py
"""

from sqlalchemy import create_engine
from models import Base, User, Match, Message, UserPreferences

# Database configuration
DATABASE_URL = "sqlite:///./test.db"
engine = create_engine(DATABASE_URL, connect_args={"check_same_thread": False})

def migrate_database():
    """Create new tables while keeping existing ones"""
    try:
        # This will create only the tables that don't exist
        Base.metadata.create_all(bind=engine)
        print("✅ Database migration completed successfully!")
        print("✅ New tables created: matches, messages, user_preferences")
        print("✅ Existing user data preserved")
    except Exception as e:
        print(f"❌ Migration failed: {e}")

if __name__ == "__main__":
    migrate_database()