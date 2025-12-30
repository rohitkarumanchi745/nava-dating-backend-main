from sqlmodel import create_engine, SQLModel, Session
from models import User

DATABASE_URL = "sqlite:///./dating.db"  # You’ll later switch this to PostgreSQL

engine = create_engine(DATABASE_URL, echo=True)

def create_db_and_tables():
    SQLModel.metadata.create_all(engine)

def get_session():
    return Session(engine)
