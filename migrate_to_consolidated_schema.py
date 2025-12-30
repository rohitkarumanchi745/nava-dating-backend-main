"""
Migration script to consolidate database schema
Run this to migrate from your current schema to the consolidated one
"""

import os
from sqlalchemy import create_engine, text
from sqlalchemy.orm import sessionmaker
from models import Base, User, Match, Message, VoiceMessage, UserLocation, UserPreferences
import json
from datetime import datetime

DATABASE_URL = os.getenv("DATABASE_URL", "sqlite:///./dating.db")
engine = create_engine(DATABASE_URL, connect_args={"check_same_thread": False} if "sqlite" in DATABASE_URL else {})
SessionLocal = sessionmaker(bind=engine)

def backup_existing_data():
    """Backup existing data before migration"""
    print("📦 Creating backup of existing data...")
    
    db = SessionLocal()
    try:
        # Export existing users if they exist
        try:
            existing_users = db.execute(text("SELECT * FROM users")).fetchall()
            with open(f"backup_users_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json", 'w') as f:
                json.dump([dict(row) for row in existing_users], f, default=str)
            print(f"✅ Backed up {len(existing_users)} users")
        except:
            print("ℹ️ No existing users table found")
        
        # Export existing matches if they exist
        try:
            existing_matches = db.execute(text("SELECT * FROM matches")).fetchall()
            with open(f"backup_matches_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json", 'w') as f:
                json.dump([dict(row) for row in existing_matches], f, default=str)
            print(f"✅ Backed up {len(existing_matches)} matches")
        except:
            print("ℹ️ No existing matches table found")
            
    finally:
        db.close()

def create_new_schema():
    """Create the new consolidated schema"""
    print("🏗️ Creating new consolidated schema...")
    
    try:
        # Create all tables
        Base.metadata.create_all(bind=engine)
        print("✅ New schema created successfully")
        
        # List all tables created
        inspector = engine.dialect.get_inspector(engine)
        tables = inspector.get_table_names()
        print(f"📋 Tables created: {', '.join(tables)}")
        
    except Exception as e:
        print(f"❌ Error creating schema: {e}")
        raise

def migrate_existing_data():
    """Migrate data from old schema to new schema"""
    print("🔄 Migrating existing data...")
    
    db = SessionLocal()
    try:
        # Check if old users table exists with old structure
        try:
            old_users = db.execute(text("""
                SELECT id, phone_number, name, dob, gender, bio, 
                       profile_photo_url, created_at, updated_at
                FROM users_old
            """)).fetchall()
            
            print(f"📊 Migrating {len(old_users)} users...")
            
            for old_user in old_users:
                # Parse profile photos if they exist
                photos_json = None
                if old_user.profile_photo_url:
                    photos = old_user.profile_photo_url.split(',')
                    photos_json = photos[:3]  # Take first 3
                    
                # Create new user record
                db.execute(text("""
                    INSERT INTO users (
                        id, phone_number, name, dob, gender, bio,
                        profile_photos, profile_photo_1, profile_photo_2, profile_photo_3,
                        is_profile_complete, created_at, updated_at
                    ) VALUES (
                        :id, :phone_number, :name, :dob, :gender, :bio,
                        :profile_photos, :photo1, :photo2, :photo3,
                        :is_complete, :created_at, :updated_at
                    )
                """), {
                    'id': old_user.id,
                    'phone_number': old_user.phone_number,
                    'name': old_user.name,
                    'dob': old_user.dob,
                    'gender': old_user.gender,
                    'bio': old_user.bio,
                    'profile_photos': json.dumps(photos_json) if photos_json else None,
                    'photo1': photos_json[0] if photos_json and len(photos_json) > 0 else None,
                    'photo2': photos_json[1] if photos_json and len(photos_json) > 1 else None,
                    'photo3': photos_json[2] if photos_json and len(photos_json) > 2 else None,
                    'is_complete': bool(old_user.name and old_user.dob),
                    'created_at': old_user.created_at,
                    'updated_at': old_user.updated_at
                })
                
            print("✅ Users migrated successfully")
            
        except Exception as e:
            print(f"ℹ️ No old users to migrate: {e}")
        
        db.commit()
        
    except Exception as e:
        print(f"❌ Error migrating data: {e}")
        db.rollback()
        raise
    finally:
        db.close()

def add_sample_data():
    """Add some sample data for testing"""
    print("🌱 Adding sample data...")
    
    db = SessionLocal()
    try:
        # Check if we already have data
        user_count = db.execute(text("SELECT COUNT(*) FROM users")).scalar()
        
        if user_count == 0:
            print("📝 Adding sample users...")
            
            # Sample users
            sample_users = [
                {
                    'phone_number': '+1234567890',
                    'name': 'Alice Johnson',
                    'dob': '1995-06-15',
                    'gender': 'female',
                    'bio': 'Love hiking, coffee, and good conversations!',
                    'is_profile_complete': True,
                    'is_active': True
                },
                {
                    'phone_number': '+1234567891',
                    'name': 'Bob Smith',
                    'dob': '1993-03-22',
                    'gender': 'male',
                    'bio': 'Software engineer who enjoys traveling and photography.',
                    'is_profile_complete': True,
                    'is_active': True
                }
            ]
            
            for user_data in sample_users:
                db.execute(text("""
                    INSERT INTO users (
                        phone_number, name, dob, gender, bio, 
                        is_profile_complete, is_active, created_at, updated_at
                    ) VALUES (
                        :phone_number, :name, :dob, :gender, :bio,
                        :is_profile_complete, :is_active, :created_at, :updated_at
                    )
                """), {
                    **user_data,
                    'created_at': datetime.utcnow(),
                    'updated_at': datetime.utcnow()
                })
            
            print("✅ Sample users added")
        else:
            print(f"ℹ️ Database already has {user_count} users, skipping sample data")
        
        db.commit()
        
    except Exception as e:
        print(f"❌ Error adding sample data: {e}")
        db.rollback()
        raise
    finally:
        db.close()

def verify_migration():
    """Verify the migration was successful"""
    print("🔍 Verifying migration...")
    
    db = SessionLocal()
    try:
        # Check all tables exist
        inspector = engine.dialect.get_inspector(engine)
        tables = inspector.get_table_names()
        
        expected_tables = [
            'users', 'user_preferences', 'user_locations', 'matches', 
            'messages', 'voice_messages', 'user_subscriptions', 
            'student_verifications', 'user_activities', 'ai_model_data',
            'content_moderation'
        ]
        
        missing_tables = [t for t in expected_tables if t not in tables]
        if missing_tables:
            print(f"⚠️ Missing tables: {missing_tables}")
        else:
            print("✅ All expected tables exist")
        
        # Check data counts
        user_count = db.execute(text("SELECT COUNT(*) FROM users")).scalar()
        match_count = db.execute(text("SELECT COUNT(*) FROM matches")).scalar()
        message_count = db.execute(text("SELECT COUNT(*) FROM messages")).scalar()
        
        print(f"📊 Data summary:")
        print(f"   Users: {user_count}")
        print(f"   Matches: {match_count}")
        print(f"   Messages: {message_count}")
        
        print("✅ Migration verification completed")
        
    except Exception as e:
        print(f"❌ Error verifying migration: {e}")
        raise
    finally:
        db.close()

def main():
    """Main migration function"""
    print("🚀 Starting database consolidation migration...")
    print("=" * 50)
    
    try:
        # Step 1: Backup existing data
        backup_existing_data()
        
        # Step 2: Create new schema
        create_new_schema()
        
        # Step 3: Migrate existing data
        migrate_existing_data()
        
        # Step 4: Add sample data if needed
        add_sample_data()
        
        # Step 5: Verify migration
        verify_migration()
        
        print("=" * 50)
        print("🎉 Migration completed successfully!")
        print("=" * 50)
        
        print("\n📋 Next steps:")
        print("1. Update your FastAPI endpoints to use the new schema")
        print("2. Test all functionality with the new database structure")
        print("3. Update your frontend to handle the new data formats")
        print("4. Consider adding proper database migrations for production")
        
    except Exception as e:
        print(f"\n❌ Migration failed: {e}")
        print("\n🔧 Troubleshooting:")
        print("1. Check your database connection")
        print("2. Make sure you have backup of your data")
        print("3. Review the error message above")
        print("4. You can restore from backup files if needed")

if __name__ == "__main__":
    main()