"""
Student Discount System for Dating App
Special pricing for students from top universities
"""

import hashlib
import json
import logging
import re
from typing import Dict, List, Optional, Set, Tuple
from collections import defaultdict
from dataclasses import dataclass
from datetime import datetime, timedelta
from decimal import Decimal
from enum import Enum
import asyncio
import requests

logger = logging.getLogger(__name__)

# ============================================
# University Database and Verification
# ============================================

@dataclass
class University:
    """University information"""
    name: str
    type: str  # 'public' or 'private'
    state: str
    city: str
    email_domains: List[str]
    coordinates: Tuple[float, float]
    ranking: int
    student_count: int
    is_eligible: bool

@dataclass
class StudentVerification:
    """Student verification record"""
    user_id: int
    university_name: str
    email: str
    student_id: str
    verification_date: datetime
    expiry_date: datetime
    is_verified: bool
    discount_tier: str
    verification_method: str  # 'email', 'id_card', 'api'

class StudentTier(Enum):
    """Student discount tiers"""
    REGULAR_STUDENT = "regular_student"  # Any verified student
    TOP_PUBLIC = "top_public"  # Top public university
    TOP_PRIVATE = "top_private"  # Top private university (Ivy League, etc.)
    GRAD_STUDENT = "grad_student"  # Graduate students
    ALUMNI = "alumni"  # Recent alumni (1 year)

# ============================================
# Top Universities Database
# ============================================

TOP_UNIVERSITIES = {
    # Top Public Universities
    'public': [
        University(
            name="University of California, Berkeley",
            type="public",
            state="CA",
            city="Berkeley",
            email_domains=["berkeley.edu", "cal.berkeley.edu"],
            coordinates=(37.8719, -122.2585),
            ranking=1,
            student_count=45000,
            is_eligible=True
        ),
        University(
            name="University of Michigan",
            type="public",
            state="MI",
            city="Ann Arbor",
            email_domains=["umich.edu", "med.umich.edu"],
            coordinates=(42.2780, -83.7382),
            ranking=2,
            student_count=46000,
            is_eligible=True
        ),
        University(
            name="University of California, Los Angeles",
            type="public",
            state="CA",
            city="Los Angeles",
            email_domains=["ucla.edu", "g.ucla.edu"],
            coordinates=(34.0689, -118.4452),
            ranking=3,
            student_count=47000,
            is_eligible=True
        ),
        University(
            name="University of Virginia",
            type="public",
            state="VA",
            city="Charlottesville",
            email_domains=["virginia.edu", "uva.edu"],
            coordinates=(38.0336, -78.5080),
            ranking=4,
            student_count=25000,
            is_eligible=True
        ),
        University(
            name="University of North Carolina at Chapel Hill",
            type="public",
            state="NC",
            city="Chapel Hill",
            email_domains=["unc.edu", "email.unc.edu"],
            coordinates=(35.9049, -79.0469),
            ranking=5,
            student_count=30000,
            is_eligible=True
        ),
        University(
            name="University of Texas at Austin",
            type="public",
            state="TX",
            city="Austin",
            email_domains=["utexas.edu", "cs.utexas.edu"],
            coordinates=(30.2849, -97.7341),
            ranking=6,
            student_count=51000,
            is_eligible=True
        ),
        University(
            name="Georgia Institute of Technology",
            type="public",
            state="GA",
            city="Atlanta",
            email_domains=["gatech.edu", "cc.gatech.edu"],
            coordinates=(33.7756, -84.3963),
            ranking=7,
            student_count=40000,
            is_eligible=True
        ),
        University(
            name="University of Florida",
            type="public",
            state="FL",
            city="Gainesville",
            email_domains=["ufl.edu", "med.ufl.edu"],
            coordinates=(29.6436, -82.3549),
            ranking=8,
            student_count=52000,
            is_eligible=True
        ),
        University(
            name="University of Washington",
            type="public",
            state="WA",
            city="Seattle",
            email_domains=["uw.edu", "u.washington.edu"],
            coordinates=(47.6553, -122.3035),
            ranking=9,
            student_count=48000,
            is_eligible=True
        ),
        University(
            name="University of Wisconsin-Madison",
            type="public",
            state="WI",
            city="Madison",
            email_domains=["wisc.edu", "cs.wisc.edu"],
            coordinates=(43.0766, -89.4125),
            ranking=10,
            student_count=45000,
            is_eligible=True
        )
    ],
    
    # Top Private Universities (Ivy League + Elite)
    'private': [
        University(
            name="Harvard University",
            type="private",
            state="MA",
            city="Cambridge",
            email_domains=["harvard.edu", "college.harvard.edu"],
            coordinates=(42.3770, -71.1167),
            ranking=1,
            student_count=23000,
            is_eligible=True
        ),
        University(
            name="Stanford University",
            type="private",
            state="CA",
            city="Stanford",
            email_domains=["stanford.edu", "cs.stanford.edu"],
            coordinates=(37.4275, -122.1697),
            ranking=2,
            student_count=17000,
            is_eligible=True
        ),
        University(
            name="Massachusetts Institute of Technology",
            type="private",
            state="MA",
            city="Cambridge",
            email_domains=["mit.edu", "csail.mit.edu"],
            coordinates=(42.3601, -71.0942),
            ranking=3,
            student_count=12000,
            is_eligible=True
        ),
        University(
            name="Yale University",
            type="private",
            state="CT",
            city="New Haven",
            email_domains=["yale.edu", "law.yale.edu"],
            coordinates=(41.3163, -72.9223),
            ranking=4,
            student_count=14000,
            is_eligible=True
        ),
        University(
            name="Princeton University",
            type="private",
            state="NJ",
            city="Princeton",
            email_domains=["princeton.edu", "cs.princeton.edu"],
            coordinates=(40.3440, -74.6514),
            ranking=5,
            student_count=8500,
            is_eligible=True
        ),
        University(
            name="Columbia University",
            type="private",
            state="NY",
            city="New York",
            email_domains=["columbia.edu", "barnard.edu"],
            coordinates=(40.8075, -73.9626),
            ranking=6,
            student_count=33000,
            is_eligible=True
        ),
        University(
            name="University of Pennsylvania",
            type="private",
            state="PA",
            city="Philadelphia",
            email_domains=["upenn.edu", "wharton.upenn.edu"],
            coordinates=(39.9522, -75.1932),
            ranking=7,
            student_count=25000,
            is_eligible=True
        ),
        University(
            name="Cornell University",
            type="private",
            state="NY",
            city="Ithaca",
            email_domains=["cornell.edu", "cs.cornell.edu"],
            coordinates=(42.4534, -76.4735),
            ranking=8,
            student_count=25000,
            is_eligible=True
        ),
        University(
            name="Brown University",
            type="private",
            state="RI",
            city="Providence",
            email_domains=["brown.edu", "cs.brown.edu"],
            coordinates=(41.8268, -71.4025),
            ranking=9,
            student_count=10000,
            is_eligible=True
        ),
        University(
            name="Dartmouth College",
            type="private",
            state="NH",
            city="Hanover",
            email_domains=["dartmouth.edu", "tuck.dartmouth.edu"],
            coordinates=(43.7044, -72.2887),
            ranking=10,
            student_count=6600,
            is_eligible=True
        ),
        University(
            name="Duke University",
            type="private",
            state="NC",
            city="Durham",
            email_domains=["duke.edu", "fuqua.duke.edu"],
            coordinates=(36.0014, -78.9382),
            ranking=11,
            student_count=16000,
            is_eligible=True
        ),
        University(
            name="Northwestern University",
            type="private",
            state="IL",
            city="Evanston",
            email_domains=["northwestern.edu", "kellogg.northwestern.edu"],
            coordinates=(42.0565, -87.6753),
            ranking=12,
            student_count=22000,
            is_eligible=True
        ),
        University(
            name="University of Chicago",
            type="private",
            state="IL",
            city="Chicago",
            email_domains=["uchicago.edu", "booth.uchicago.edu"],
            coordinates=(41.7886, -87.5987),
            ranking=13,
            student_count=18000,
            is_eligible=True
        ),
        University(
            name="California Institute of Technology",
            type="private",
            state="CA",
            city="Pasadena",
            email_domains=["caltech.edu"],
            coordinates=(34.1377, -118.1253),
            ranking=14,
            student_count=2400,
            is_eligible=True
        ),
        University(
            name="Johns Hopkins University",
            type="private",
            state="MD",
            city="Baltimore",
            email_domains=["jhu.edu", "jhmi.edu"],
            coordinates=(39.3299, -76.6205),
            ranking=15,
            student_count=28000,
            is_eligible=True
        )
    ]
}

# ============================================
# Student Discount Configurations
# ============================================

STUDENT_PASS_CONFIGS = {
    StudentTier.TOP_PRIVATE: {
        # Elite private university students (70% discount)
        'discount_percentage': 0.70,
        'pass_prices': {
            'hourly': 3.60,    # $3.60 instead of $12
            'daily': 6.00,     # $6 instead of $20
            'weekly': 29.70,   # $29.70 instead of $99
            'monthly': 89.70,  # $89.70 instead of $299
            'ultra': 149.70    # $149.70 instead of $499
        },
        'special_features': {
            'university_badge': True,
            'campus_events': True,
            'study_buddy_matching': True,
            'alumni_network': True,
            'verified_student_badge': True
        }
    },
    StudentTier.TOP_PUBLIC: {
        # Top public university students (60% discount)
        'discount_percentage': 0.60,
        'pass_prices': {
            'hourly': 4.80,    # $4.80 instead of $12
            'daily': 8.00,     # $8 instead of $20
            'weekly': 39.60,   # $39.60 instead of $99
            'monthly': 119.60, # $119.60 instead of $299
            'ultra': 199.60    # $199.60 instead of $499
        },
        'special_features': {
            'university_badge': True,
            'campus_events': True,
            'study_buddy_matching': True,
            'verified_student_badge': True
        }
    },
    StudentTier.GRAD_STUDENT: {
        # Graduate students (50% discount)
        'discount_percentage': 0.50,
        'pass_prices': {
            'hourly': 6.00,    # $6 instead of $12
            'daily': 10.00,    # $10 instead of $20
            'weekly': 49.50,   # $49.50 instead of $99
            'monthly': 149.50, # $149.50 instead of $299
            'ultra': 249.50    # $249.50 instead of $499
        },
        'special_features': {
            'university_badge': True,
            'professional_networking': True,
            'verified_student_badge': True
        }
    },
    StudentTier.ALUMNI: {
        # Recent alumni - 1 year after graduation (30% discount)
        'discount_percentage': 0.30,
        'pass_prices': {
            'hourly': 8.40,    # $8.40 instead of $12
            'daily': 14.00,    # $14 instead of $20
            'weekly': 69.30,   # $69.30 instead of $99
            'monthly': 209.30, # $209.30 instead of $299
            'ultra': 349.30    # $349.30 instead of $499
        },
        'special_features': {
            'alumni_badge': True,
            'alumni_network': True,
            'professional_networking': True
        }
    }
}

# ============================================
# Student Verification System
# ============================================

class StudentVerificationSystem:
    """Handles student verification and discount eligibility"""
    
    def __init__(self, redis_client=None):
        self.redis_client = redis_client
        self.verified_students: Dict[int, StudentVerification] = {}
        self.university_database = self._build_university_database()
        self.verification_attempts = defaultdict(list)
        self.max_attempts = 3
        
        # Analytics
        self.university_stats = defaultdict(lambda: {
            'total_users': 0,
            'active_users': 0,
            'revenue': Decimal('0'),
            'most_popular_pass': None
        })
        
        # Start background tasks
        asyncio.create_task(self._check_verification_expiry())
    
    def _build_university_database(self) -> Dict[str, University]:
        """Build searchable university database"""
        db = {}
        for uni_type in ['public', 'private']:
            for uni in TOP_UNIVERSITIES[uni_type]:
                db[uni.name.lower()] = uni
                # Also index by email domains
                for domain in uni.email_domains:
                    db[domain] = uni
        return db
    
    async def verify_student_email(self, user_id: int, email: str, 
                                  student_id: Optional[str] = None) -> Dict:
        """Verify student status via university email"""
        
        # Check attempt limits
        if len(self.verification_attempts[user_id]) >= self.max_attempts:
            last_attempt = self.verification_attempts[user_id][-1]
            if (datetime.now() - last_attempt).seconds < 3600:
                return {
                    'success': False,
                    'error': 'Too many verification attempts. Try again in 1 hour.'
                }
        
        # Extract domain from email
        email_pattern = r'^[a-zA-Z0-9._%+-]+@([a-zA-Z0-9.-]+\.[a-zA-Z]{2,})$'
        match = re.match(email_pattern, email.lower())
        
        if not match:
            self.verification_attempts[user_id].append(datetime.now())
            return {
                'success': False,
                'error': 'Invalid email format'
            }
        
        domain = match.group(1)
        
        # Check if domain belongs to eligible university
        university = self.university_database.get(domain)
        
        if not university or not university.is_eligible:
            self.verification_attempts[user_id].append(datetime.now())
            return {
                'success': False,
                'error': 'Email domain not from eligible university'
            }
        
        # Determine student tier
        if university.type == 'private':
            # Check if grad student (contains 'grad', 'phd', 'mba', etc.)
            if any(keyword in email.lower() for keyword in ['grad', 'phd', 'mba', 'ms', 'ma']):
                tier = StudentTier.GRAD_STUDENT
            else:
                tier = StudentTier.TOP_PRIVATE
        else:  # public
            if any(keyword in email.lower() for keyword in ['grad', 'phd', 'mba', 'ms', 'ma']):
                tier = StudentTier.GRAD_STUDENT
            else:
                tier = StudentTier.TOP_PUBLIC
        
        # Send verification email (in production)
        verification_code = self._generate_verification_code(user_id, email)
        
        # For demo, auto-verify
        verification = StudentVerification(
            user_id=user_id,
            university_name=university.name,
            email=email,
            student_id=student_id or f"AUTO_{user_id}",
            verification_date=datetime.now(),
            expiry_date=datetime.now() + timedelta(days=365),  # 1 year validity
            is_verified=True,
            discount_tier=tier.value,
            verification_method='email'
        )
        
        self.verified_students[user_id] = verification
        self._store_verification(verification)
        
        # Update university stats
        self.university_stats[university.name]['total_users'] += 1
        
        return {
            'success': True,
            'verification': {
                'university': university.name,
                'university_type': university.type,
                'tier': tier.value,
                'discount_percentage': STUDENT_PASS_CONFIGS[tier]['discount_percentage'],
                'expiry_date': verification.expiry_date.isoformat(),
                'special_features': STUDENT_PASS_CONFIGS[tier]['special_features']
            },
            'message': f'Welcome {university.name} student! You get {int(STUDENT_PASS_CONFIGS[tier]["discount_percentage"]*100)}% off all passes!'
        }
    
    def _generate_verification_code(self, user_id: int, email: str) -> str:
        """Generate verification code for email"""
        data = f"{user_id}{email}{datetime.now()}"
        return hashlib.sha256(data.encode()).hexdigest()[:8]
    
    def _store_verification(self, verification: StudentVerification):
        """Store verification in Redis"""
        if self.redis_client:
            key = f"student_verification:{verification.user_id}"
            data = {
                'university_name': verification.university_name,
                'email': verification.email,
                'discount_tier': verification.discount_tier,
                'expiry_date': verification.expiry_date.isoformat(),
                'is_verified': verification.is_verified
            }
            ttl = int((verification.expiry_date - datetime.now()).total_seconds())
            if ttl > 0:
                self.redis_client.setex(key, ttl, json.dumps(data))
    
    def get_student_status(self, user_id: int) -> Optional[StudentVerification]:
        """Get student verification status"""
        if user_id in self.verified_students:
            verification = self.verified_students[user_id]
            if verification.expiry_date > datetime.now():
                return verification
            else:
                # Expired
                del self.verified_students[user_id]
        
        # Try loading from Redis
        if self.redis_client:
            key = f"student_verification:{user_id}"
            data = self.redis_client.get(key)
            if data:
                verification_data = json.loads(data)
                # Reconstruct verification object
                return StudentVerification(
                    user_id=user_id,
                    university_name=verification_data['university_name'],
                    email=verification_data['email'],
                    student_id='',
                    verification_date=datetime.now(),
                    expiry_date=datetime.fromisoformat(verification_data['expiry_date']),
                    is_verified=verification_data['is_verified'],
                    discount_tier=verification_data['discount_tier'],
                    verification_method='email'
                )
        
        return None
    
    def calculate_student_price(self, user_id: int, pass_type: str, 
                               original_price: Decimal) -> Tuple[Decimal, Optional[str]]:
        """Calculate discounted price for verified students"""
        
        verification = self.get_student_status(user_id)
        
        if not verification or not verification.is_verified:
            return original_price, None
        
        # Get student tier
        try:
            tier = StudentTier(verification.discount_tier)
        except ValueError:
            return original_price, None
        
        # Get discounted price
        if tier in STUDENT_PASS_CONFIGS:
            config = STUDENT_PASS_CONFIGS[tier]
            if pass_type in config['pass_prices']:
                discounted_price = Decimal(str(config['pass_prices'][pass_type]))
                return discounted_price, verification.university_name
        
        # Apply percentage discount as fallback
        discount_percentage = STUDENT_PASS_CONFIGS[tier]['discount_percentage']
        discounted_price = original_price * Decimal(str(1 - discount_percentage))
        
        return discounted_price, verification.university_name
    
    async def verify_with_student_id_card(self, user_id: int, 
                                         id_card_image: bytes,
                                         university_name: str) -> Dict:
        """Verify using student ID card (with OCR in production)"""
        
        # Check if university is eligible
        uni = self.university_database.get(university_name.lower())
        
        if not uni or not uni.is_eligible:
            return {
                'success': False,
                'error': 'University not eligible for student discount'
            }
        
        # In production, use OCR to verify ID card
        # For demo, simulate verification
        
        # Determine tier
        tier = StudentTier.TOP_PRIVATE if uni.type == 'private' else StudentTier.TOP_PUBLIC
        
        verification = StudentVerification(
            user_id=user_id,
            university_name=uni.name,
            email=f"verified_{user_id}@{uni.email_domains[0]}",
            student_id=f"ID_{user_id}",
            verification_date=datetime.now(),
            expiry_date=datetime.now() + timedelta(days=365),
            is_verified=True,
            discount_tier=tier.value,
            verification_method='id_card'
        )
        
        self.verified_students[user_id] = verification
        self._store_verification(verification)
        
        return {
            'success': True,
            'verification': {
                'university': uni.name,
                'tier': tier.value,
                'discount_percentage': STUDENT_PASS_CONFIGS[tier]['discount_percentage']
            }
        }
    
    def convert_to_alumni(self, user_id: int) -> bool:
        """Convert graduated student to alumni status"""
        
        verification = self.get_student_status(user_id)
        
        if not verification:
            return False
        
        # Create alumni verification
        alumni_verification = StudentVerification(
            user_id=user_id,
            university_name=verification.university_name,
            email=verification.email,
            student_id=verification.student_id,
            verification_date=datetime.now(),
            expiry_date=datetime.now() + timedelta(days=365),  # 1 year alumni discount
            is_verified=True,
            discount_tier=StudentTier.ALUMNI.value,
            verification_method='alumni_conversion'
        )
        
        self.verified_students[user_id] = alumni_verification
        self._store_verification(alumni_verification)
        
        return True
    
    async def _check_verification_expiry(self):
        """Background task to check and handle expired verifications"""
        while True:
            current_time = datetime.now()
            expired_users = []
            
            for user_id, verification in self.verified_students.items():
                if current_time > verification.expiry_date:
                    expired_users.append(user_id)
            
            for user_id in expired_users:
                # Check if eligible for alumni status
                verification = self.verified_students[user_id]
                if verification.discount_tier in [StudentTier.TOP_PUBLIC.value, 
                                                 StudentTier.TOP_PRIVATE.value]:
                    # Convert to alumni
                    self.convert_to_alumni(user_id)
                    logger.info(f"Converted user {user_id} to alumni status")
                else:
                    # Remove verification
                    del self.verified_students[user_id]
                    logger.info(f"Student verification expired for user {user_id}")
            
            await asyncio.sleep(3600)  # Check every hour
    
    def get_university_analytics(self) -> Dict:
        """Get analytics for university participation"""
        
        # Calculate top universities by users
        uni_rankings = []
        for uni_name, stats in self.university_stats.items():
            uni_rankings.append({
                'university': uni_name,
                'total_users': stats['total_users'],
                'active_users': stats['active_users'],
                'revenue': str(stats['revenue']),
                'type': self.university_database.get(uni_name.lower(), {}).type
            })
        
        uni_rankings.sort(key=lambda x: x['total_users'], reverse=True)
        
        # Calculate tier distribution
        tier_distribution = defaultdict(int)
        for verification in self.verified_students.values():
            tier_distribution[verification.discount_tier] += 1
        
        return {
            'top_universities': uni_rankings[:10],
            'total_verified_students': len(self.verified_students),
            'tier_distribution': dict(tier_distribution),
            'public_vs_private': {
                'public': sum(1 for v in self.verified_students.values() 
                            if StudentTier(v.discount_tier) == StudentTier.TOP_PUBLIC),
                'private': sum(1 for v in self.verified_students.values() 
                             if StudentTier(v.discount_tier) == StudentTier.TOP_PRIVATE)
            }
        }
    
    def find_campus_matches(self, user_id: int) -> List[int]:
        """Find other verified students from same university"""
        
        verification = self.get_student_status(user_id)
        if not verification:
            return []
        
        campus_matches = []
        for other_id, other_verification in self.verified_students.items():
            if (other_id != user_id and 
                other_verification.university_name == verification.university_name):
                campus_matches.append(other_id)
        
        return campus_matches

# ============================================
# Enhanced Pass Manager with Student Discounts
# ============================================

def integrate_student_discounts(pass_manager_instance):
    """Integrate student discount system with existing pass manager"""
    
    # Add student verification system
    pass_manager_instance.student_verification = StudentVerificationSystem(
        pass_manager_instance.redis_client
    )
    
    # Override purchase_pass method to include student discounts
    original_purchase_pass = pass_manager_instance.purchase_pass
    
    async def enhanced_purchase_pass(user_id: int, pass_type, 
                                    payment_method: str, 
                                    promo_code: Optional[str] = None):
        """Enhanced purchase with student discount"""
        
        # Check for student discount
        verification = pass_manager_instance.student_verification.get_student_status(user_id)
        
        if verification and verification.is_verified:
            # Get original price
            original_config = pass_manager_instance.PASS_CONFIGS[pass_type]
            original_price = Decimal(str(original_config['price']))
            
            # Calculate student price
            student_price, university = pass_manager_instance.student_verification.calculate_student_price(
                user_id, pass_type.value, original_price
            )
            
            # Temporarily modify config for this purchase
            pass_manager_instance.PASS_CONFIGS[pass_type]['price'] = float(student_price)
            
            # Process purchase with student price
            result = await original_purchase_pass(user_id, pass_type, payment_method, promo_code)
            
            # Restore original price
            pass_manager_instance.PASS_CONFIGS[pass_type]['price'] = float(original_price)
            
            # Add student discount info to result
            if result['success']:
                result['student_discount'] = {
                    'university': university,
                    'original_price': str(original_price),
                    'discounted_price': str(student_price),
                    'saved': str(original_price - student_price),
                    'tier': verification.discount_tier
                }
                
                # Update university stats
                uni_stats = pass_manager_instance.student_verification.university_stats[university]
                uni_stats['revenue'] += student_price
                uni_stats['active_users'] += 1
            
            return result
        
        # No student discount, process normally
        return await original_purchase_pass(user_id, pass_type, payment_method, promo_code)
    
    # Replace method
    pass_manager_instance.purchase_pass = enhanced_purchase_pass
    
    logger.info("Student discount system integrated with pass manager")
    
    return pass_manager_instance

# ============================================
# API Endpoints for Student Features
# ============================================

from fastapi import FastAPI, UploadFile, File, HTTPException
from pydantic import BaseModel

class StudentVerificationRequest(BaseModel):
    user_id: int
    email: str
    student_id: Optional[str] = None

class StudentIDVerificationRequest(BaseModel):
    user_id: int
    university_name: str

async def add_student_endpoints(app: FastAPI, pass_manager):
    """Add student-specific endpoints to the API"""
    
    @app.post("/api/student/verify-email")
    async def verify_student_email(request: StudentVerificationRequest):
        """Verify student status via university email"""
        
        result = await pass_manager.student_verification.verify_student_email(
            user_id=request.user_id,
            email=request.email,
            student_id=request.student_id
        )
        
        if not result['success']:
            raise HTTPException(status_code=400, detail=result['error'])
        
        return result
    
    @app.post("/api/student/verify-id")
    async def verify_student_id(user_id: int, university_name: str, 
                               id_card: UploadFile = File(...)):
        """Verify student status via ID card"""
        
        id_card_data = await id_card.read()
        
        result = await pass_manager.student_verification.verify_with_student_id_card(
            user_id=user_id,
            id_card_image=id_card_data,
            university_name=university_name
        )
        
        if not result['success']:
            raise HTTPException(status_code=400, detail=result['error'])
        
        return result
    
    @app.get("/api/student/status/{user_id}")
    async def get_student_status(user_id: int):
        """Get student verification status"""
        
        verification = pass_manager.student_verification.get_student_status(user_id)
        
        if not verification:
            return {
                'verified': False,
                'message': 'Not verified as student'
            }
        
        tier = StudentTier(verification.discount_tier)
        config = STUDENT_PASS_CONFIGS[tier]
        
        return {
            'verified': True,
            'university': verification.university_name,
            'tier': verification.discount_tier,
            'expiry_date': verification.expiry_date.isoformat(),
            'discount_percentage': config['discount_percentage'],
            'special_prices': config['pass_prices'],
            'special_features': config['special_features']
        }
    
    @app.get("/api/student/universities")
    async def get_eligible_universities():
        """Get list of eligible universities"""
        
        universities = []
        for uni_type in ['public', 'private']:
            for uni in TOP_UNIVERSITIES[uni_type]:
                universities.append({
                    'name': uni.name,
                    'type': uni.type,
                    'state': uni.state,
                    'city': uni.city,
                    'ranking': uni.ranking,
                    'discount': '70%' if uni.type == 'private' else '60%'
                })
        
        return {
            'total': len(universities),
            'universities': universities
        }
    
    @app.get("/api/student/campus-matches/{user_id}")
    async def find_campus_matches(user_id: int):
        """Find matches from same university"""
        
        verification = pass_manager.student_verification.get_student_status(user_id)
        
        if not verification:
            raise HTTPException(status_code=403, detail="Student verification required")
        
        campus_matches = pass_manager.student_verification.find_campus_matches(user_id)
        
        return {
            'university': verification.university_name,
            'total_campus_matches': len(campus_matches),
            'match_ids': campus_matches[:50]  # Limit to 50
        }
    
    @app.get("/api/student/analytics")
    async def get_student_analytics():
        """Get analytics for student program"""
        
        return pass_manager.student_verification.get_university_analytics()

# Export components
__all__ = [
    'University',
    'StudentVerification',
    'StudentTier',
    'StudentVerificationSystem',
    'STUDENT_PASS_CONFIGS',
    'TOP_UNIVERSITIES',
    'integrate_student_discounts',
    'add_student_endpoints'
]
