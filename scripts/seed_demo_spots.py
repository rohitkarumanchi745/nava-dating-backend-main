"""
Seed a small set of demo users, spot, and pre-match chat so the Playground flow shows data.
Run once from repo root:
  PYTHONPATH=. python scripts/seed_demo_spots.py
Requires DATABASE_URL to point at your running DB.
"""

import os
import uuid
from datetime import datetime

from sqlalchemy import create_engine
from sqlalchemy.orm import Session, sessionmaker

from models import User, Spot, SpotMessage, SpotPairStatus, Match


def get_session() -> Session:
    url = os.environ.get("DATABASE_URL")
    if not url:
        raise RuntimeError("DATABASE_URL is not set")
    engine = create_engine(url)
    return sessionmaker(bind=engine)()


def get_or_create_user(db: Session, phone: str, name: str) -> User:
    user = db.query(User).filter(User.phone_number == phone).first()
    if user:
        return user
    user = User(
        phone_number=phone,
        name=name,
        bio="Demo bio for " + name,
        is_active=True,
        is_verified=False,
        is_student_verified=False,
        created_at=datetime.utcnow(),
    )
    db.add(user)
    db.commit()
    db.refresh(user)
    return user


def seed():
    db = get_session()
    try:
        alice = get_or_create_user(db, "+911111111111", "Aanya")
        bob = get_or_create_user(db, "+922222222222", "Kiran")

        # Create a demo spot for Alice
        spot = db.query(Spot).filter(Spot.title == "Monsoon chai ritual").first()
        if not spot:
            spot = Spot(
                user_id=alice.id,
                title="Monsoon chai ritual",
                original_url="https://sample-videos.com/video321/mp4/720/big_buck_bunny_720p_1mb.mp4",
                poster_url=None,
                mime_type="video/mp4",
                tags=["Food", "Culture"],
                is_global=True,
                created_at=datetime.utcnow(),
            )
            db.add(spot)
            db.commit()
            db.refresh(spot)

        # Conversation: 5 messages each way
        a_id, b_id = sorted([alice.id, bob.id])
        status = (
            db.query(SpotPairStatus)
            .filter(
                SpotPairStatus.spot_id == spot.id,
                SpotPairStatus.user_a == a_id,
                SpotPairStatus.user_b == b_id,
            )
            .first()
        )
        if not status:
            status = SpotPairStatus(
                spot_id=spot.id,
                user_a=a_id,
                user_b=b_id,
                a_count=5,
                b_count=5,
                eligible_for_match=True,
                request_status="accepted",
            )
            db.add(status)
            db.commit()
            db.refresh(status)

        if db.query(SpotMessage).filter(SpotMessage.spot_id == spot.id).count() < 10:
            texts = [
                (alice.id, "Hi there! Check my 30s spot."),
                (bob.id, "Looks cool! Where was this?"),
                (alice.id, "Goa, monsoon mornings."),
                (bob.id, "Love it. Let’s chat more."),
                (alice.id, "Sure!"),
                (bob.id, "Sent you a request."),
                (alice.id, "Accepted :)"),
                (bob.id, "Great, see you in Matches."),
                (alice.id, "Awesome."),
                (bob.id, "Talk soon!"),
            ]
            for sender_id, text in texts:
                db.add(SpotMessage(spot_id=spot.id, sender_id=sender_id, text=text))
            db.commit()

        if not status.match_id:
            match_id = str(uuid.uuid4())
            match = Match(
                id=match_id,
                user1_id=a_id,
                user2_id=b_id,
                user1_liked=True,
                user2_liked=True,
                is_mutual_match=True,
                status="active",
                match_reason="spot",
            )
            db.add(match)
            db.commit()
            status.match_id = match_id
            db.commit()

        print("Seeded demo users, spot, messages, and match.")
    finally:
        db.close()


if __name__ == "__main__":
    seed()
