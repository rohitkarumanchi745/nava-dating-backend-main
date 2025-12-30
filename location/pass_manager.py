"""
Location Pass Manager Module for Dating App
Handles premium pass purchases, activation, and feature management
"""

import hashlib
import json
import logging
import asyncio
from typing import Dict, List, Optional, Set
from dataclasses import dataclass
from datetime import datetime, timedelta
from decimal import Decimal
from enum import Enum
from collections import defaultdict
import redis
import stripe

# Configure logging
logger = logging.getLogger(__name__)

# ============================================
# Data Classes and Enums
# ============================================

@dataclass
class LocationPass:
    """Premium location pass for enhanced features"""
    user_id: int
    pass_type: str  
    start_time: datetime
    end_time: datetime
    amount_paid: Decimal
    payment_id: str
    is_active: bool
    search_radius: float  # Enhanced search radius in miles
    features_unlocked: Dict[str, bool]
    transaction_hash: str
    show_city_names: bool  # Ultra premium feature

@dataclass
class Location:
    """User location with city information"""
    user_id: int
    latitude: float
    longitude: float
    accuracy: float  # GPS accuracy in meters
    timestamp: datetime
    is_fuzzy: bool = False  
    city: Optional[str] = None
    state: Optional[str] = None
    country: Optional[str] = None
    neighborhood: Optional[str] = None

class PassType(Enum):
    """Pass types for location features"""
    FREE = "free"
    HOURLY = "hourly"  # $12/hour - 2 mile enhanced radius
    DAILY = "daily"    # $20/day - 5 mile enhanced radius  
    WEEKLY = "weekly"  # $99/week - 10 mile enhanced radius
    MONTHLY = "monthly" # $299/month - unlimited radius
    ULTRA = "ultra"    # $499/month - unlimited + city names

# ============================================
# Pass Configurations
# ============================================

PASS_CONFIGS = {
    PassType.FREE: {
        'price': 0,
        'duration_hours': float('inf'),
        'search_radius': float('inf'),  # Can search whole country
        'enhanced_radius': 0,  # But no enhanced features
        'features': {
            'search_nationwide': True,  # Can search anywhere
            'precise_location': False,  # Only approximate distances
            'real_time_updates': False,
            'see_exact_distance': False,
            'see_city_name': False,  # Can't see city names
            'priority_visibility': False,
            'unlimited_swipes_in_radius': False,
            'advanced_filters': False
        }
    },
    PassType.HOURLY: {
        'price': 12.00,
        'duration_hours': 1,
        'search_radius': float('inf'),  # Can search anywhere
        'enhanced_radius': 2.0,  # 2 miles enhanced features
        'features': {
            'search_nationwide': True,
            'precise_location': True,  # Within 2 miles
            'real_time_updates': True,
            'see_exact_distance': True,  # Within 2 miles
            'see_city_name': False,  # No city names
            'priority_visibility': True,
            'unlimited_swipes_in_radius': True,
            'advanced_filters': False
        }
    },
    PassType.DAILY: {
        'price': 20.00,
        'duration_hours': 24,
        'search_radius': float('inf'),
        'enhanced_radius': 5.0,  # 5 miles enhanced features
        'features': {
            'search_nationwide': True,
            'precise_location': True,  # Within 5 miles
            'real_time_updates': True,
            'see_exact_distance': True,  # Within 5 miles
            'see_city_name': True,  # CAN SEE CITY NAMES
            'priority_visibility': True,
            'unlimited_swipes_in_radius': True,
            'boost_included': True,
            'advanced_filters': True
        }
    },
    PassType.WEEKLY: {
        'price': 99.00,
        'duration_hours': 168,
        'search_radius': float('inf'),
        'enhanced_radius': 10.0,  # 10 miles enhanced features
        'features': {
            'search_nationwide': True,
            'precise_location': True,  # Within 10 miles
            'real_time_updates': True,
            'see_exact_distance': True,  # Within 10 miles
            'see_city_name': True,  # CAN SEE CITY NAMES
            'priority_visibility': True,
            'unlimited_swipes_in_radius': True,
            'boost_included': True,
            'advanced_filters': True,
            'see_who_liked': True
        }
    },
    PassType.MONTHLY: {
        'price': 299.00,
        'duration_hours': 720,
        'search_radius': float('inf'),
        'enhanced_radius': float('inf'),  # Unlimited enhanced radius
        'features': {
            'search_nationwide': True,
            'precise_location': True,  # Everywhere
            'real_time_updates': True,
            'see_exact_distance': True,  # Everywhere
            'see_city_name': True,  # CAN SEE CITY NAMES
            'priority_visibility': True,
            'unlimited_swipes_in_radius': True,
            'boost_included': True,
            'advanced_filters': True,
            'travel_mode': True,
            'analytics_dashboard': True,
            'see_who_liked': True
        }
    },
    PassType.ULTRA: {
        'price': 499.00,
        'duration_hours': 720,
        'search_radius': float('inf'),
        'enhanced_radius': float('inf'),
        'features': {
            'search_nationwide': True,
            'precise_location': True,
            'real_time_updates': True,
            'see_exact_distance': True,
            'see_city_name': True,  # EXCLUSIVE CITY NAME FEATURE
            'see_neighborhood': True,  # Even more precise
            'priority_visibility': True,
            'unlimited_swipes_in_radius': True,
            'boost_included': True,
            'advanced_filters': True,
            'travel_mode': True,
            'analytics_dashboard': True,
            'see_who_liked': True,
            'incognito_mode': True,
            'change_location': True,  # Can set location anywhere
            'verified_badge': True
        }
    }
}

# ============================================
# Payment Processor
# ============================================

class PaymentProcessor:
    """Handles payment processing for passes"""
    
    def __init__(self, stripe_api_key: Optional[str] = None):
        self.stripe_api_key = stripe_api_key or "sk_test_default_key"
        if self.stripe_api_key:
            stripe.api_key = self.stripe_api_key
        
        # Payment method validation
        self.valid_payment_methods = {
            'card', 'apple_pay', 'google_pay', 'paypal'
        }
        
        # Track failed payments for fraud prevention
        self.failed_payments = defaultdict(list)
        self.max_failed_attempts = 3
    
    async def process_payment(self, user_id: int, amount: float, 
                            payment_method: str, description: str) -> Dict:
        """Process payment through payment gateway"""
        
        # Check for too many failed attempts
        if len(self.failed_payments[user_id]) >= self.max_failed_attempts:
            last_attempt = self.failed_payments[user_id][-1]
            if (datetime.now() - last_attempt).seconds < 3600:  # 1 hour cooldown
                return {
                    'success': False,
                    'error': 'Too many failed payment attempts. Please try again later.'
                }
        
        try:
            # Validate payment method
            if payment_method not in self.valid_payment_methods and not payment_method.startswith('pm_'):
                return {
                    'success': False,
                    'error': 'Invalid payment method'
                }
            
            # In production, use actual Stripe API
            if self.stripe_api_key.startswith('sk_live'):
                # Production Stripe charge
                charge = stripe.Charge.create(
                    amount=int(amount * 100),  # Convert to cents
                    currency='usd',
                    source=payment_method,
                    description=description,
                    metadata={'user_id': user_id}
                )
                
                return {
                    'success': charge.status == 'succeeded',
                    'payment_id': charge.id,
                    'receipt_url': charge.receipt_url
                }
            else:
                # Test mode simulation
                if payment_method == "test_fail":
                    self.failed_payments[user_id].append(datetime.now())
                    return {'success': False, 'error': 'Card declined'}
                
                # Simulate successful payment
                payment_id = f"ch_{hashlib.md5(f'{user_id}{datetime.now()}'.encode()).hexdigest()[:24]}"
                
                return {
                    'success': True,
                    'payment_id': payment_id,
                    'receipt_url': f"https://receipts.example.com/{payment_id}"
                }
                
        except stripe.error.CardError as e:
            self.failed_payments[user_id].append(datetime.now())
            return {
                'success': False,
                'error': f"Card error: {str(e)}"
            }
        except stripe.error.StripeError as e:
            logger.error(f"Stripe error for user {user_id}: {e}")
            return {
                'success': False,
                'error': 'Payment processing error'
            }
        except Exception as e:
            logger.error(f"Unexpected payment error for user {user_id}: {e}")
            return {
                'success': False,
                'error': 'Payment failed'
            }
    
    async def refund_payment(self, payment_id: str, amount: Optional[float] = None) -> Dict:
        """Process refund for a payment"""
        try:
            if self.stripe_api_key.startswith('sk_live'):
                # Production refund
                refund = stripe.Refund.create(
                    charge=payment_id,
                    amount=int(amount * 100) if amount else None
                )
                
                return {
                    'success': refund.status == 'succeeded',
                    'refund_id': refund.id
                }
            else:
                # Test mode
                return {
                    'success': True,
                    'refund_id': f"re_{hashlib.md5(payment_id.encode()).hexdigest()[:24]}"
                }
                
        except Exception as e:
            logger.error(f"Refund failed for payment {payment_id}: {e}")
            return {
                'success': False,
                'error': str(e)
            }

# ============================================
# Enhanced Location Pass Manager
# ============================================

class EnhancedLocationPassManager:
    """Manages location passes with city features"""
    
    def __init__(self, redis_client: Optional[redis.Redis] = None, 
                 stripe_api_key: Optional[str] = None):
        self.active_passes: Dict[int, LocationPass] = {}
        self.pass_history: Dict[int, List[LocationPass]] = defaultdict(list)
        self.redis_client = redis_client
        
        # Initialize payment processor
        self.payment_processor = PaymentProcessor(stripe_api_key)
        
        # Revenue tracking
        self.revenue_tracker = {
            'hourly': {'count': 0, 'total': Decimal('0'), 'refunds': 0},
            'daily': {'count': 0, 'total': Decimal('0'), 'refunds': 0},
            'weekly': {'count': 0, 'total': Decimal('0'), 'refunds': 0},
            'monthly': {'count': 0, 'total': Decimal('0'), 'refunds': 0},
            'ultra': {'count': 0, 'total': Decimal('0'), 'refunds': 0}
        }
        
        # Pass usage analytics
        self.usage_analytics = defaultdict(lambda: {
            'searches_performed': 0,
            'matches_found': 0,
            'cities_viewed': set(),
            'total_distance_searched': 0.0
        })
        
        # Promotional codes
        self.promo_codes = {
            'FIRST20': {'discount': 0.20, 'uses_remaining': 1000},
            'WEEKLY50': {'discount': 0.50, 'uses_remaining': 100, 'applicable_to': ['weekly']},
            'ULTRA100': {'discount': 100.00, 'uses_remaining': 10, 'applicable_to': ['ultra'], 'type': 'fixed'}
        }
        
        # Start background tasks
        asyncio.create_task(self._check_pass_expiration())
        asyncio.create_task(self._sync_with_redis())
    
    async def purchase_pass(self, user_id: int, pass_type: PassType, 
                           payment_method: str, promo_code: Optional[str] = None) -> Dict:
        """Process pass purchase with payment"""
        
        # Check if user already has an active pass
        if self.has_active_pass(user_id):
            current_pass = self.active_passes[user_id]
            
            # Allow upgrade
            if self._is_upgrade(current_pass.pass_type, pass_type.value):
                return await self._upgrade_pass(user_id, current_pass, pass_type, payment_method)
            
            return {
                'success': False,
                'error': 'Active pass already exists',
                'current_pass': self._serialize_pass(current_pass)
            }
        
        # Get pass configuration
        pass_config = PASS_CONFIGS[pass_type]
        price = Decimal(str(pass_config['price']))
        
        # Apply promo code if valid
        if promo_code:
            discount_result = self._apply_promo_code(promo_code, pass_type, price)
            if discount_result['valid']:
                price = discount_result['final_price']
            else:
                return {
                    'success': False,
                    'error': discount_result['error']
                }
        
        # Process payment (skip for free tier)
        if price > 0:
            payment_result = await self.payment_processor.process_payment(
                user_id=user_id,
                amount=float(price),
                payment_method=payment_method,
                description=f"Location Pass - {pass_type.value}"
            )
            
            if not payment_result['success']:
                return {
                    'success': False,
                    'error': payment_result['error']
                }
            
            payment_id = payment_result['payment_id']
            receipt_url = payment_result.get('receipt_url')
        else:
            payment_id = 'free_tier'
            receipt_url = None
        
        # Create and activate pass
        location_pass = await self._create_pass(
            user_id=user_id,
            pass_type=pass_type,
            payment_id=payment_id,
            amount_paid=price
        )
        
        # Activate pass
        self._activate_pass(location_pass)
        
        # Track revenue
        if price > 0:
            self._track_revenue(pass_type, price)
        
        # Log to audit trail
        await self._log_transaction(location_pass)
        
        # Send confirmation
        response = {
            'success': True,
            'pass': self._serialize_pass(location_pass),
            'message': self._get_activation_message(pass_type),
            'receipt_url': receipt_url
        }
        
        if promo_code and price < pass_config['price']:
            response['discount_applied'] = {
                'code': promo_code,
                'original_price': str(pass_config['price']),
                'final_price': str(price),
                'saved': str(Decimal(str(pass_config['price'])) - price)
            }
        
        return response
    
    def _is_upgrade(self, current_type: str, new_type: str) -> bool:
        """Check if new pass is an upgrade"""
        hierarchy = ['free', 'hourly', 'daily', 'weekly', 'monthly', 'ultra']
        try:
            return hierarchy.index(new_type) > hierarchy.index(current_type)
        except ValueError:
            return False
    
    async def _upgrade_pass(self, user_id: int, current_pass: LocationPass, 
                           new_pass_type: PassType, payment_method: str) -> Dict:
        """Handle pass upgrade"""
        
        # Calculate prorated refund for remaining time
        remaining_time = (current_pass.end_time - datetime.now()).total_seconds()
        if remaining_time > 0 and current_pass.amount_paid > 0:
            total_duration = (current_pass.end_time - current_pass.start_time).total_seconds()
            refund_ratio = remaining_time / total_duration
            refund_amount = float(current_pass.amount_paid) * refund_ratio
            
            # Process refund
            refund_result = await self.payment_processor.refund_payment(
                current_pass.payment_id, refund_amount
            )
            
            if not refund_result['success']:
                logger.warning(f"Refund failed for user {user_id}: {refund_result}")
        else:
            refund_amount = 0
        
        # Deactivate current pass
        self._deactivate_pass(user_id)
        
        # Purchase new pass with credit
        new_config = PASS_CONFIGS[new_pass_type]
        upgrade_price = Decimal(str(new_config['price'])) - Decimal(str(refund_amount))
        
        # Process payment for difference
        if upgrade_price > 0:
            payment_result = await self.payment_processor.process_payment(
                user_id=user_id,
                amount=float(upgrade_price),
                payment_method=payment_method,
                description=f"Pass Upgrade to {new_pass_type.value}"
            )
            
            if not payment_result['success']:
                # Reactivate old pass if payment fails
                self._activate_pass(current_pass)
                return {
                    'success': False,
                    'error': payment_result['error']
                }
            
            payment_id = payment_result['payment_id']
        else:
            payment_id = f"upgrade_{current_pass.payment_id}"
        
        # Create new pass
        new_pass = await self._create_pass(
            user_id=user_id,
            pass_type=new_pass_type,
            payment_id=payment_id,
            amount_paid=upgrade_price
        )
        
        self._activate_pass(new_pass)
        
        return {
            'success': True,
            'message': f'Successfully upgraded to {new_pass_type.value} pass',
            'pass': self._serialize_pass(new_pass),
            'credit_applied': str(refund_amount),
            'amount_charged': str(upgrade_price)
        }
    
    def _apply_promo_code(self, code: str, pass_type: PassType, price: Decimal) -> Dict:
        """Apply promotional code to purchase"""
        
        if code not in self.promo_codes:
            return {'valid': False, 'error': 'Invalid promo code'}
        
        promo = self.promo_codes[code]
        
        # Check if promo has uses remaining
        if promo['uses_remaining'] <= 0:
            return {'valid': False, 'error': 'Promo code has been fully redeemed'}
        
        # Check if promo applies to this pass type
        if 'applicable_to' in promo:
            if pass_type.value not in promo['applicable_to']:
                return {'valid': False, 'error': f'Promo code not valid for {pass_type.value} pass'}
        
        # Calculate discount
        if promo.get('type') == 'fixed':
            discount = Decimal(str(promo['discount']))
        else:
            discount = price * Decimal(str(promo['discount']))
        
        final_price = max(price - discount, Decimal('0'))
        
        # Consume promo use
        promo['uses_remaining'] -= 1
        
        return {
            'valid': True,
            'final_price': final_price,
            'discount': discount
        }
    
    async def _create_pass(self, user_id: int, pass_type: PassType, 
                          payment_id: str, amount_paid: Decimal) -> LocationPass:
        """Create a new location pass"""
        
        config = PASS_CONFIGS[pass_type]
        start_time = datetime.now()
        
        # Set end time (infinite for free tier)
        if pass_type == PassType.FREE:
            end_time = datetime.max
        else:
            end_time = start_time + timedelta(hours=config['duration_hours'])
        
        # Generate unique transaction hash
        transaction_data = f"{user_id}{pass_type.value}{start_time}{payment_id}"
        transaction_hash = hashlib.sha256(transaction_data.encode()).hexdigest()
        
        # Check if user can see city names
        show_city_names = config['features'].get('see_city_name', False)
        
        location_pass = LocationPass(
            user_id=user_id,
            pass_type=pass_type.value,
            start_time=start_time,
            end_time=end_time,
            amount_paid=amount_paid,
            payment_id=payment_id,
            is_active=True,
            search_radius=config['enhanced_radius'],
            features_unlocked=config['features'],
            transaction_hash=transaction_hash,
            show_city_names=show_city_names
        )
        
        return location_pass
    
    def _activate_pass(self, location_pass: LocationPass):
        """Activate a location pass"""
        self.active_passes[location_pass.user_id] = location_pass
        self.pass_history[location_pass.user_id].append(location_pass)
        
        # Store in Redis for persistence
        if self.redis_client and location_pass.pass_type != 'free':
            self._store_pass_in_redis(location_pass)
    
    def _store_pass_in_redis(self, location_pass: LocationPass):
        """Store pass in Redis with appropriate TTL"""
        if not self.redis_client:
            return
        
        key = f"location_pass:{location_pass.user_id}"
        ttl = int((location_pass.end_time - datetime.now()).total_seconds())
        
        if ttl > 0:
            pass_data = {
                'user_id': location_pass.user_id,
                'pass_type': location_pass.pass_type,
                'end_time': location_pass.end_time.isoformat(),
                'search_radius': location_pass.search_radius,
                'features': location_pass.features_unlocked,
                'transaction_hash': location_pass.transaction_hash
            }
            
            self.redis_client.setex(
                key,
                ttl,
                json.dumps(pass_data)
            )
    
    def has_active_pass(self, user_id: int) -> bool:
        """Check if user has an active PAID pass"""
        if user_id not in self.active_passes:
            return False
        
        pass_obj = self.active_passes[user_id]
        
        # Free tier doesn't count as "active pass"
        if pass_obj.pass_type == 'free':
            return False
        
        if datetime.now() > pass_obj.end_time:
            # Pass expired
            self._deactivate_pass(user_id)
            return False
        
        return True
    
    def get_user_features(self, user_id: int) -> Dict[str, bool]:
        """Get user's current features based on pass"""
        if user_id in self.active_passes:
            return self.active_passes[user_id].features_unlocked
        
        # Return free tier features by default
        return PASS_CONFIGS[PassType.FREE]['features']
    
    def get_enhanced_radius(self, user_id: int) -> float:
        """Get user's enhanced features radius"""
        if user_id in self.active_passes:
            return self.active_passes[user_id].search_radius
        return 0  # No enhanced radius for free users
    
    def can_see_city_names(self, user_id: int) -> bool:
        """Check if user can see city names"""
        features = self.get_user_features(user_id)
        return features.get('see_city_name', False)
    
    def _deactivate_pass(self, user_id: int):
        """Deactivate expired pass"""
        if user_id in self.active_passes:
            self.active_passes[user_id].is_active = False
            del self.active_passes[user_id]
            
            # Remove from Redis
            if self.redis_client:
                self.redis_client.delete(f"location_pass:{user_id}")
    
    async def _check_pass_expiration(self):
        """Background task to check and deactivate expired passes"""
        while True:
            current_time = datetime.now()
            expired_users = []
            
            for user_id, pass_obj in self.active_passes.items():
                if pass_obj.pass_type != 'free' and current_time > pass_obj.end_time:
                    expired_users.append(user_id)
            
            for user_id in expired_users:
                self._deactivate_pass(user_id)
                logger.info(f"Pass expired for user {user_id}")
                
                # Send expiration notification (implement notification system)
                # await self._send_expiration_notification(user_id)
            
            await asyncio.sleep(60)  # Check every minute
    
    async def _sync_with_redis(self):
        """Sync passes with Redis periodically"""
        while True:
            if self.redis_client:
                try:
                    # Sync active passes to Redis
                    for user_id, pass_obj in self.active_passes.items():
                        if pass_obj.pass_type != 'free':
                            self._store_pass_in_redis(pass_obj)
                except Exception as e:
                    logger.error(f"Redis sync error: {e}")
            
            await asyncio.sleep(300)  # Sync every 5 minutes
    
    def _track_revenue(self, pass_type: PassType, amount: Decimal):
        """Track revenue from pass sales"""
        key = pass_type.value
        if key in self.revenue_tracker:
            self.revenue_tracker[key]['count'] += 1
            self.revenue_tracker[key]['total'] += amount
    
    async def _log_transaction(self, location_pass: LocationPass):
        """Log transaction for audit trail"""
        log_entry = {
            'timestamp': datetime.now().isoformat(),
            'user_id': location_pass.user_id,
            'pass_type': location_pass.pass_type,
            'amount': str(location_pass.amount_paid),
            'payment_id': location_pass.payment_id,
            'transaction_hash': location_pass.transaction_hash,
            'features': location_pass.features_unlocked
        }
        
        logger.info(f"Transaction logged: {log_entry}")
        
        # In production, also log to database or audit service
        # await self._store_transaction_in_db(log_entry)
    
    def _serialize_pass(self, pass_obj: LocationPass) -> Dict:
        """Serialize pass object for API response"""
        return {
            'id': pass_obj.transaction_hash,
            'type': pass_obj.pass_type,
            'starts': pass_obj.start_time.isoformat(),
            'expires': pass_obj.end_time.isoformat() if pass_obj.end_time != datetime.max else 'Never',
            'enhanced_radius': pass_obj.search_radius,
            'features': pass_obj.features_unlocked,
            'amount_paid': str(pass_obj.amount_paid),
            'show_city_names': pass_obj.show_city_names
        }
    
    def _get_activation_message(self, pass_type: PassType) -> str:
        """Get appropriate activation message"""
        messages = {
            PassType.FREE: "Free tier active! Search nationwide with basic features.",
            PassType.HOURLY: "1-hour pass activated! Enhanced features within 2 miles.",
            PassType.DAILY: "24-hour pass activated! Enhanced features within 5 miles + city names!",
            PassType.WEEKLY: "Weekly pass activated! Enhanced features within 10 miles + city names!",
            PassType.MONTHLY: "Monthly pass activated! Unlimited enhanced features + city names!",
            PassType.ULTRA: "Ultra Premium activated! All features + neighborhoods unlocked!"
        }
        return messages.get(pass_type, "Pass activated!")
    
    def get_revenue_report(self) -> Dict:
        """Get comprehensive revenue report"""
        total_revenue = sum(
            data['total'] for data in self.revenue_tracker.values()
        )
        total_refunds = sum(
            data.get('refunds', 0) for data in self.revenue_tracker.values()
        )
        
        return {
            'total_revenue': str(total_revenue),
            'total_refunds': total_refunds,
            'net_revenue': str(total_revenue),  # Refunds already deducted
            'by_pass_type': {
                pass_type: {
                    'count': data['count'],
                    'total': str(data['total']),
                    'average': str(data['total'] / data['count']) if data['count'] > 0 else '0',
                    'refunds': data.get('refunds', 0)
                }
                for pass_type, data in self.revenue_tracker.items()
            },
            'active_passes': len(self.active_passes),
            'total_passes_sold': sum(data['count'] for data in self.revenue_tracker.values()),
            'conversion_metrics': self._calculate_conversion_metrics()
        }
    
    def _calculate_conversion_metrics(self) -> Dict:
        """Calculate conversion and retention metrics"""
        total_users = len(self.pass_history)
        paid_users = sum(1 for passes in self.pass_history.values() 
                        if any(p.pass_type != 'free' for p in passes))
        
        repeat_buyers = sum(1 for passes in self.pass_history.values() 
                           if len([p for p in passes if p.pass_type != 'free']) > 1)
        
        return {
            'conversion_rate': paid_users / max(total_users, 1),
            'repeat_purchase_rate': repeat_buyers / max(paid_users, 1),
            'avg_lifetime_value': str(
                sum(self.revenue_tracker[pt]['total'] for pt in self.revenue_tracker) / 
                max(paid_users, 1)
            )
        }
    
    def track_usage(self, user_id: int, action: str, **kwargs):
        """Track pass usage for analytics"""
        if user_id in self.usage_analytics:
            analytics = self.usage_analytics[user_id]
            
            if action == 'search':
                analytics['searches_performed'] += 1
                analytics['total_distance_searched'] += kwargs.get('distance', 0)
            elif action == 'match_found':
                analytics['matches_found'] += 1
            elif action == 'city_viewed':
                city = kwargs.get('city')
                if city:
                    analytics['cities_viewed'].add(city)
    
    def get_user_usage_stats(self, user_id: int) -> Dict:
        """Get usage statistics for a user"""
        if user_id not in self.usage_analytics:
            return {
                'searches_performed': 0,
                'matches_found': 0,
                'cities_viewed': 0,
                'total_distance_searched': 0
            }
        
        analytics = self.usage_analytics[user_id]
        return {
            'searches_performed': analytics['searches_performed'],
            'matches_found': analytics['matches_found'],
            'cities_viewed': len(analytics['cities_viewed']),
            'total_distance_searched': analytics['total_distance_searched'],
            'unique_cities': list(analytics['cities_viewed'])[:10]  # Top 10 cities
        }

# Export main components
__all__ = [
    'LocationPass',
    'Location',
    'PassType',
    'PASS_CONFIGS',
    'PaymentProcessor',
    'EnhancedLocationPassManager'
]