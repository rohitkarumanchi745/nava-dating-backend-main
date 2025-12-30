"""
Location Matcher Module for Dating App
Handles nationwide matching with premium location features
"""

import math
import numpy as np
from typing import Dict, List, Tuple, Optional, Set, Union
from dataclasses import dataclass
from datetime import datetime, timedelta
from collections import defaultdict, deque
import hashlib
import heapq
from scipy.spatial import KDTree
import asyncio
import logging
import reverse_geocoder as rg
import requests
import json

# Configure logging
logger = logging.getLogger(__name__)

# ============================================
# Location Service for Geocoding
# ============================================

class LocationService:
    """Service for geocoding and location details"""
    
    def __init__(self):
        self.city_cache = {}
        self.cache_ttl = 3600  # 1 hour cache
        self.last_cache_clear = datetime.now()
        
        # Popular cities with coordinates
        self.major_cities = {
            'new york': {'lat': 40.7128, 'lon': -74.0060, 'state': 'NY'},
            'los angeles': {'lat': 34.0522, 'lon': -118.2437, 'state': 'CA'},
            'chicago': {'lat': 41.8781, 'lon': -87.6298, 'state': 'IL'},
            'houston': {'lat': 29.7604, 'lon': -95.3698, 'state': 'TX'},
            'phoenix': {'lat': 33.4484, 'lon': -112.0740, 'state': 'AZ'},
            'philadelphia': {'lat': 39.9526, 'lon': -75.1652, 'state': 'PA'},
            'san antonio': {'lat': 29.4241, 'lon': -98.4936, 'state': 'TX'},
            'san diego': {'lat': 32.7157, 'lon': -117.1611, 'state': 'CA'},
            'dallas': {'lat': 32.7767, 'lon': -96.7970, 'state': 'TX'},
            'san jose': {'lat': 37.3382, 'lon': -121.8863, 'state': 'CA'},
            'austin': {'lat': 30.2672, 'lon': -97.7431, 'state': 'TX'},
            'jacksonville': {'lat': 30.3322, 'lon': -81.6557, 'state': 'FL'},
            'san francisco': {'lat': 37.7749, 'lon': -122.4194, 'state': 'CA'},
            'columbus': {'lat': 39.9612, 'lon': -82.9988, 'state': 'OH'},
            'fort worth': {'lat': 32.7555, 'lon': -97.3308, 'state': 'TX'},
            'indianapolis': {'lat': 39.7684, 'lon': -86.1581, 'state': 'IN'},
            'charlotte': {'lat': 35.2271, 'lon': -80.8431, 'state': 'NC'},
            'seattle': {'lat': 47.6062, 'lon': -122.3321, 'state': 'WA'},
            'denver': {'lat': 39.7392, 'lon': -104.9903, 'state': 'CO'},
            'washington': {'lat': 38.9072, 'lon': -77.0369, 'state': 'DC'},
            'boston': {'lat': 42.3601, 'lon': -71.0589, 'state': 'MA'},
            'nashville': {'lat': 36.1627, 'lon': -86.7816, 'state': 'TN'},
            'miami': {'lat': 25.7617, 'lon': -80.1918, 'state': 'FL'},
            'atlanta': {'lat': 33.7490, 'lon': -84.3880, 'state': 'GA'},
            'las vegas': {'lat': 36.1699, 'lon': -115.1398, 'state': 'NV'}
        }
        
        # Neighborhood data for ultra premium
        self.neighborhoods = {
            'new york': ['Manhattan', 'Brooklyn', 'Queens', 'Bronx', 'Staten Island', 
                        'Upper East Side', 'Upper West Side', 'SoHo', 'Tribeca', 
                        'Greenwich Village', 'Chelsea', 'Williamsburg', 'DUMBO'],
            'los angeles': ['Hollywood', 'Beverly Hills', 'Santa Monica', 'Venice', 
                          'Downtown LA', 'Silver Lake', 'Los Feliz', 'Manhattan Beach',
                          'Pasadena', 'Burbank', 'Glendale', 'Long Beach'],
            'san francisco': ['Mission District', 'Castro', 'Haight-Ashbury', 'Marina',
                            'Pacific Heights', 'Nob Hill', 'SOMA', 'Financial District',
                            'Chinatown', 'North Beach', 'Potrero Hill', 'Sunset'],
            'chicago': ['Loop', 'River North', 'Gold Coast', 'Lincoln Park', 'Lakeview',
                       'Wicker Park', 'Bucktown', 'Old Town', 'West Loop', 'Hyde Park']
        }
    
    def get_city_info(self, latitude: float, longitude: float) -> Dict:
        """Get city, state, and country from coordinates"""
        
        # Clear cache periodically
        if (datetime.now() - self.last_cache_clear).seconds > self.cache_ttl:
            self.city_cache.clear()
            self.last_cache_clear = datetime.now()
        
        # Check cache
        cache_key = f"{latitude:.4f},{longitude:.4f}"
        if cache_key in self.city_cache:
            return self.city_cache[cache_key]
        
        try:
            # Use reverse geocoder for city lookup
            results = rg.search((latitude, longitude))
            
            if results:
                result = results[0]
                city_info = {
                    'city': result.get('name', 'Unknown'),
                    'state': result.get('admin1', 'Unknown'),
                    'country': result.get('cc', 'US'),
                    'full_location': f"{result.get('name', '')}, {result.get('admin1', '')}"
                }
                
                # Cache the result
                self.city_cache[cache_key] = city_info
                return city_info
        except Exception as e:
            logger.error(f"Error getting city info: {e}")
        
        # Fallback: find nearest major city
        nearest_city = self._find_nearest_major_city(latitude, longitude)
        if nearest_city:
            return nearest_city
        
        return {
            'city': 'Unknown',
            'state': 'Unknown',
            'country': 'US',
            'full_location': 'Location unavailable'
        }
    
    def _find_nearest_major_city(self, lat: float, lon: float) -> Optional[Dict]:
        """Find the nearest major city to given coordinates"""
        min_distance = float('inf')
        nearest_city = None
        
        for city_name, city_data in self.major_cities.items():
            distance = self._calculate_distance(lat, lon, city_data['lat'], city_data['lon'])
            if distance < min_distance:
                min_distance = distance
                nearest_city = {
                    'city': city_name.title(),
                    'state': city_data['state'],
                    'country': 'US',
                    'full_location': f"{city_name.title()}, {city_data['state']}"
                }
        
        return nearest_city
    
    def _calculate_distance(self, lat1: float, lon1: float, lat2: float, lon2: float) -> float:
        """Simple distance calculation for finding nearest city"""
        return math.sqrt((lat2 - lat1)**2 + (lon2 - lon1)**2)
    
    def get_neighborhood(self, latitude: float, longitude: float, city: str) -> str:
        """Get neighborhood name (ultra premium feature)"""
        
        # Check if we have neighborhood data for this city
        city_lower = city.lower() if city else ''
        if city_lower in self.neighborhoods:
            neighborhoods = self.neighborhoods[city_lower]
            # Simple hash to consistently return same neighborhood
            index = int(abs(latitude * longitude * 1000)) % len(neighborhoods)
            return neighborhoods[index]
        
        # Generic neighborhoods for other cities
        generic_neighborhoods = [
            "Downtown", "Midtown", "Uptown", "East Side", "West Side",
            "North End", "South End", "Old Town", "Arts District", "Waterfront"
        ]
        
        index = int(abs(latitude * longitude * 1000)) % len(generic_neighborhoods)
        return generic_neighborhoods[index]
    
    def get_coordinates_from_city(self, city_name: str) -> Optional[Tuple[float, float]]:
        """Get coordinates for a city name"""
        city_lower = city_name.lower().strip()
        
        # Check major cities
        if city_lower in self.major_cities:
            city_data = self.major_cities[city_lower]
            return (city_data['lat'], city_data['lon'])
        
        # Try reverse geocoding API (in production, use Google Geocoding API)
        # For now, return None if not found
        return None

# ============================================
# Nationwide Location Matcher
# ============================================

class NationwideLocationMatcher:
    """Location-based matching with nationwide search and premium features"""
    
    def __init__(self, pass_manager):
        self.pass_manager = pass_manager
        self.location_service = LocationService()
        self.earth_radius_miles = 3959.0
        self.user_locations: Dict[int, 'Location'] = {}
        self.kdtree = None
        self.location_coords = []
        self.location_user_ids = []
        
        # Cache for performance
        self.distance_cache = {}
        self.cache_ttl = 300  # 5 minutes
        self.last_cache_clear = datetime.now()
        
        # Real-time location updates for premium users
        self.real_time_subscribers: Set[int] = set()
        
        # Activity tracking for heatmaps
        self.activity_grid = defaultdict(lambda: defaultdict(int))
        self.grid_resolution = 0.01  # ~0.69 miles per grid cell
        
        # User clustering for better matching
        self.user_clusters = defaultdict(set)
        self.cluster_update_interval = timedelta(minutes=30)
        self.last_cluster_update = datetime.now()
    
    def haversine_distance(self, lat1: float, lon1: float, 
                          lat2: float, lon2: float) -> float:
        """Calculate distance between two points in miles using Haversine formula"""
        
        # Check cache first
        cache_key = f"{lat1:.4f},{lon1:.4f}-{lat2:.4f},{lon2:.4f}"
        if cache_key in self.distance_cache:
            return self.distance_cache[cache_key]
        
        # Convert to radians
        lat1, lon1, lat2, lon2 = map(math.radians, [lat1, lon1, lat2, lon2])
        
        # Haversine formula
        dlat = lat2 - lat1
        dlon = lon2 - lon1
        a = math.sin(dlat/2)**2 + math.cos(lat1) * math.cos(lat2) * math.sin(dlon/2)**2
        c = 2 * math.asin(math.sqrt(a))
        
        distance = self.earth_radius_miles * c
        
        # Cache the result
        self.distance_cache[cache_key] = distance
        
        # Clear cache periodically
        if (datetime.now() - self.last_cache_clear).seconds > self.cache_ttl:
            self.distance_cache.clear()
            self.last_cache_clear = datetime.now()
        
        return distance
    
    def update_user_location(self, user_id: int, latitude: float, 
                           longitude: float, accuracy: float = 10.0):
        """Update user's location with city information"""
        
        # Get city information
        city_info = self.location_service.get_city_info(latitude, longitude)
        
        # Get neighborhood for ultra premium users
        neighborhood = None
        features = self.pass_manager.get_user_features(user_id)
        if features.get('see_neighborhood', False):
            neighborhood = self.location_service.get_neighborhood(
                latitude, longitude, city_info['city']
            )
        
        # Import Location class from pass_manager
        from location.pass_manager import Location
        
        location = Location(
            user_id=user_id,
            latitude=latitude,
            longitude=longitude,
            accuracy=accuracy,
            timestamp=datetime.now(),
            is_fuzzy=False,
            city=city_info['city'],
            state=city_info['state'],
            country=city_info['country'],
            neighborhood=neighborhood
        )
        
        self.user_locations[user_id] = location
        
        # Update activity heatmap
        self._update_activity_grid(latitude, longitude)
        
        # Update user clusters
        self._update_user_clusters(user_id, city_info['city'])
        
        # Rebuild KDTree for efficient spatial queries
        self._rebuild_kdtree()
        
        # Notify real-time subscribers if premium
        if user_id in self.real_time_subscribers:
            asyncio.create_task(self._notify_nearby_users(user_id, location))
    
    def _update_activity_grid(self, latitude: float, longitude: float):
        """Update activity grid for heatmap generation"""
        grid_x = int(latitude / self.grid_resolution)
        grid_y = int(longitude / self.grid_resolution)
        self.activity_grid[grid_x][grid_y] += 1
    
    def _update_user_clusters(self, user_id: int, city: str):
        """Update user clustering by city"""
        # Remove user from old clusters
        for cluster_users in self.user_clusters.values():
            cluster_users.discard(user_id)
        
        # Add to new cluster
        if city:
            self.user_clusters[city].add(user_id)
    
    def _rebuild_kdtree(self):
        """Rebuild KDTree for efficient nearest neighbor queries"""
        if not self.user_locations:
            return
        
        self.location_coords = []
        self.location_user_ids = []
        
        for user_id, location in self.user_locations.items():
            # Convert lat/lon to 3D coordinates for KDTree
            lat_rad = math.radians(location.latitude)
            lon_rad = math.radians(location.longitude)
            
            # Convert to Cartesian coordinates
            x = math.cos(lat_rad) * math.cos(lon_rad)
            y = math.cos(lat_rad) * math.sin(lon_rad)
            z = math.sin(lat_rad)
            
            self.location_coords.append([x, y, z])
            self.location_user_ids.append(user_id)
        
        if self.location_coords:
            self.kdtree = KDTree(self.location_coords)
    
    async def find_nationwide_matches(self, user_id: int, 
                                     search_location: Optional[Tuple[float, float]] = None,
                                     search_city: Optional[str] = None,
                                     max_results: int = 100,
                                     filters: Optional[Dict] = None) -> List[Dict]:
        """
        Find matches nationwide with enhanced features for premium users
        
        FREE users: Can search anywhere but get limited info
        PAID users: Get enhanced features within their radius
        """
        
        # Determine search location
        if search_city:
            coords = self.location_service.get_coordinates_from_city(search_city)
            if coords:
                search_lat, search_lon = coords
            else:
                return {'error': f"City '{search_city}' not found"}
        elif search_location:
            search_lat, search_lon = search_location
        elif user_id in self.user_locations:
            user_location = self.user_locations[user_id]
            search_lat = user_location.latitude
            search_lon = user_location.longitude
        else:
            return []
        
        # Get user's features and enhanced radius
        user_features = self.pass_manager.get_user_features(user_id)
        enhanced_radius = self.pass_manager.get_enhanced_radius(user_id)
        can_see_cities = self.pass_manager.can_see_city_names(user_id)
        
        # Track usage for analytics
        self.pass_manager.track_usage(user_id, 'search', distance=enhanced_radius)
        
        # Find ALL users (nationwide search for everyone)
        all_matches = []
        
        # If searching in a specific city, prioritize users in that city
        if search_city and search_city in self.user_clusters:
            city_users = self.user_clusters[search_city]
            prioritized_users = list(city_users) + [uid for uid in self.location_user_ids 
                                                    if uid not in city_users]
        else:
            prioritized_users = self.location_user_ids
        
        for match_user_id in prioritized_users:
            if match_user_id == user_id:
                continue
            
            if match_user_id not in self.user_locations:
                continue
            
            match_location = self.user_locations[match_user_id]
            
            # Calculate distance
            distance = self.haversine_distance(
                search_lat, search_lon,
                match_location.latitude, match_location.longitude
            )
            
            # Apply filters if provided
            if filters and not self._apply_filters(match_user_id, filters):
                continue
            
            # Determine what information to show
            match_info = {
                'user_id': match_user_id,
                'distance': distance,
                'distance_display': self._format_distance(distance, enhanced_radius, user_features),
                'location_display': self._format_location(match_location, distance, enhanced_radius, can_see_cities),
                'enhanced': distance <= enhanced_radius if enhanced_radius > 0 else False,
                'last_updated': match_location.timestamp.isoformat()
            }
            
            # Add premium features if within enhanced radius
            if enhanced_radius > 0 and distance <= enhanced_radius:
                match_info['exact_distance'] = f"{distance:.1f} miles"
                match_info['is_online'] = self._is_user_online(match_user_id)
                
                if can_see_cities:
                    match_info['city'] = match_location.city
                    match_info['state'] = match_location.state
                    
                    # Track city view
                    self.pass_manager.track_usage(user_id, 'city_viewed', city=match_location.city)
                    
                    if user_features.get('see_neighborhood', False) and match_location.neighborhood:
                        match_info['neighborhood'] = match_location.neighborhood
            
            # Add match quality score based on various factors
            match_info['match_quality'] = self._calculate_match_quality(
                user_id, match_user_id, distance, enhanced_radius
            )
            
            all_matches.append(match_info)
            
            # Track match found
            self.pass_manager.track_usage(user_id, 'match_found')
        
        # Sort matches
        all_matches = self._sort_matches(all_matches, enhanced_radius, search_city)
        
        return all_matches[:max_results]
    
    def _apply_filters(self, user_id: int, filters: Dict) -> bool:
        """Apply search filters"""
        # Implement filter logic based on your user data
        # For now, return True for all
        return True
    
    def _calculate_match_quality(self, user_id: int, match_id: int, 
                                distance: float, enhanced_radius: float) -> float:
        """Calculate match quality score"""
        # Base score from distance (closer is better)
        if distance < 1:
            distance_score = 1.0
        elif distance < 5:
            distance_score = 0.8
        elif distance < 10:
            distance_score = 0.6
        elif distance < 25:
            distance_score = 0.4
        elif distance < 50:
            distance_score = 0.2
        else:
            distance_score = 0.1
        
        # Bonus for being in enhanced radius
        enhanced_bonus = 0.2 if enhanced_radius > 0 and distance <= enhanced_radius else 0
        
        # Activity score (recently active users score higher)
        if match_id in self.user_locations:
            time_since_update = (datetime.now() - self.user_locations[match_id].timestamp).seconds
            if time_since_update < 300:  # Active in last 5 minutes
                activity_score = 0.3
            elif time_since_update < 3600:  # Active in last hour
                activity_score = 0.2
            elif time_since_update < 86400:  # Active in last day
                activity_score = 0.1
            else:
                activity_score = 0
        else:
            activity_score = 0
        
        return min(distance_score + enhanced_bonus + activity_score, 1.0)
    
    def _sort_matches(self, matches: List[Dict], enhanced_radius: float, 
                     search_city: Optional[str]) -> List[Dict]:
        """Sort matches with premium users' enhanced matches first"""
        
        # Separate into categories
        enhanced_close = []  # Within 2 miles and enhanced
        enhanced_medium = []  # 2-10 miles and enhanced  
        enhanced_far = []  # 10+ miles and enhanced
        regular_close = []  # Within 10 miles, not enhanced
        regular_far = []  # 10+ miles, not enhanced
        
        for match in matches:
            distance = match['distance']
            is_enhanced = match['enhanced']
            
            if is_enhanced:
                if distance < 2:
                    enhanced_close.append(match)
                elif distance < 10:
                    enhanced_medium.append(match)
                else:
                    enhanced_far.append(match)
            else:
                if distance < 10:
                    regular_close.append(match)
                else:
                    regular_far.append(match)
        
        # Sort each category by match quality
        for category in [enhanced_close, enhanced_medium, enhanced_far, 
                        regular_close, regular_far]:
            category.sort(key=lambda x: x.get('match_quality', 0), reverse=True)
        
        # Combine in priority order
        return enhanced_close + enhanced_medium + enhanced_far + regular_close + regular_far
    
    def _format_distance(self, distance: float, enhanced_radius: float, 
                        features: Dict[str, bool]) -> str:
        """Format distance display based on user's pass"""
        
        # Within enhanced radius - show exact distance
        if enhanced_radius > 0 and distance <= enhanced_radius:
            if distance < 0.1:
                return f"{int(distance * 5280)} feet away"
            elif distance < 1:
                return f"{distance:.1f} miles away"
            else:
                return f"{int(distance)} miles away"
        
        # Outside enhanced radius or free user - show approximate
        if distance < 1:
            return "< 1 mile"
        elif distance < 5:
            return "1-5 miles"
        elif distance < 10:
            return "5-10 miles"
        elif distance < 25:
            return "10-25 miles"
        elif distance < 50:
            return "25-50 miles"
        elif distance < 100:
            return "50-100 miles"
        elif distance < 500:
            return "100-500 miles"
        elif distance < 1000:
            return "500-1000 miles"
        else:
            return "1000+ miles"
    
    def _format_location(self, location, distance: float, 
                        enhanced_radius: float, can_see_cities: bool) -> str:
        """Format location display based on user's pass"""
        
        # Premium users within enhanced radius
        if enhanced_radius > 0 and distance <= enhanced_radius and can_see_cities:
            if location.neighborhood:  # Ultra premium
                return f"{location.neighborhood}, {location.city}, {location.state}"
            else:
                return f"{location.city}, {location.state}"
        
        # Outside enhanced radius but can see cities (for daily+ passes)
        elif can_see_cities:
            return f"{location.city}, {location.state}"
        
        # Free users - only show state
        return location.state if location.state else "United States"
    
    def _is_user_online(self, user_id: int) -> bool:
        """Check if user is currently online"""
        if user_id not in self.user_locations:
            return False
        
        last_update = self.user_locations[user_id].timestamp
        return (datetime.now() - last_update).seconds < 300  # 5 minutes
    
    def find_users_in_radius(self, latitude: float, longitude: float, 
                            radius_miles: float, exclude_user: Optional[int] = None) -> List[Tuple[int, float]]:
        """Find all users within specified radius using KDTree"""
        
        if not self.kdtree:
            return []
        
        results = []
        
        # Convert radius to approximate tree distance
        # This is an approximation for the KDTree search
        tree_distance = (radius_miles / self.earth_radius_miles) * 2
        
        # Query KDTree
        lat_rad = math.radians(latitude)
        lon_rad = math.radians(longitude)
        
        x = math.cos(lat_rad) * math.cos(lon_rad)
        y = math.cos(lat_rad) * math.sin(lon_rad)
        z = math.sin(lat_rad)
        
        indices = self.kdtree.query_ball_point([x, y, z], tree_distance)
        
        # Calculate actual distances and filter
        for idx in indices:
            other_user_id = self.location_user_ids[idx]
            
            if exclude_user and other_user_id == exclude_user:
                continue
            
            other_location = self.user_locations[other_user_id]
            
            actual_distance = self.haversine_distance(
                latitude, longitude,
                other_location.latitude, other_location.longitude
            )
            
            if actual_distance <= radius_miles:
                results.append((other_user_id, actual_distance))
        
        return results
    
    async def _notify_nearby_users(self, user_id: int, location):
        """Send real-time notifications to nearby premium users"""
        
        # Find premium users within their enhanced radius
        for other_id in self.real_time_subscribers:
            if other_id == user_id:
                continue
            
            enhanced_radius = self.pass_manager.get_enhanced_radius(other_id)
            if enhanced_radius == 0:
                continue
            
            if other_id in self.user_locations:
                distance = self.haversine_distance(
                    location.latitude, location.longitude,
                    self.user_locations[other_id].latitude,
                    self.user_locations[other_id].longitude
                )
                
                if distance <= enhanced_radius:
                    await self._send_proximity_notification(other_id, user_id, distance)
    
    async def _send_proximity_notification(self, recipient_id: int, 
                                          nearby_user_id: int, distance: float):
        """Send proximity notification to user"""
        logger.info(f"User {nearby_user_id} is {distance:.2f} miles from user {recipient_id}")
        # In production, implement actual push notification

# ============================================
# Heatmap Generator
# ============================================

class HeatmapGenerator:
    """Generate activity heatmaps for premium users"""
    
    def __init__(self, location_matcher: NationwideLocationMatcher):
        self.location_matcher = location_matcher
        self.grid_resolution = 0.01  # ~0.69 miles per grid cell
    
    def get_hotspots(self, latitude: float, longitude: float, 
                     radius_miles: float, limit: int = 10) -> List[Dict]:
        """Get activity hotspots within radius"""
        hotspots = []
        
        # Convert radius to grid cells
        grid_radius = int(radius_miles / 69.0 / self.grid_resolution)
        center_x = int(latitude / self.grid_resolution)
        center_y = int(longitude / self.grid_resolution)
        
        activity_grid = self.location_matcher.activity_grid
        
        for dx in range(-grid_radius, grid_radius + 1):
            for dy in range(-grid_radius, grid_radius + 1):
                grid_x = center_x + dx
                grid_y = center_y + dy
                
                if grid_x in activity_grid and grid_y in activity_grid[grid_x]:
                    activity = activity_grid[grid_x][grid_y]
                    
                    if activity > 5:  # Minimum activity threshold
                        hotspot_lat = grid_x * self.grid_resolution
                        hotspot_lon = grid_y * self.grid_resolution
                        
                        distance = self.location_matcher.haversine_distance(
                            latitude, longitude, hotspot_lat, hotspot_lon
                        )
                        
                        if distance <= radius_miles:
                            # Get approximate address for hotspot
                            city_info = self.location_matcher.location_service.get_city_info(
                                hotspot_lat, hotspot_lon
                            )
                            
                            hotspots.append({
                                'latitude': hotspot_lat,
                                'longitude': hotspot_lon,
                                'activity_level': activity,
                                'distance': distance,
                                'location': city_info.get('full_location', 'Unknown'),
                                'intensity': self._calculate_intensity(activity)
                            })
        
        # Sort by activity level
        hotspots.sort(key=lambda x: x['activity_level'], reverse=True)
        return hotspots[:limit]
    
    def _calculate_intensity(self, activity: int) -> str:
        """Calculate hotspot intensity label"""
        if activity > 100:
            return "very_high"
        elif activity > 50:
            return "high"
        elif activity > 20:
            return "medium"
        elif activity > 10:
            return "low"
        else:
            return "minimal"
    
    def get_city_analytics(self) -> Dict:
        """Get analytics by city"""
        city_stats = defaultdict(lambda: {
            'total_users': 0,
            'active_users': 0,
            'avg_activity': 0
        })
        
        for user_id, location in self.location_matcher.user_locations.items():
            city = location.city
            if city:
                city_stats[city]['total_users'] += 1
                
                # Check if active
                if self.location_matcher._is_user_online(user_id):
                    city_stats[city]['active_users'] += 1
        
        # Convert to regular dict and sort by total users
        city_list = []
        for city, stats in city_stats.items():
            city_list.append({
                'city': city,
                'total_users': stats['total_users'],
                'active_users': stats['active_users'],
                'activity_rate': stats['active_users'] / max(stats['total_users'], 1)
            })
        
        city_list.sort(key=lambda x: x['total_users'], reverse=True)
        
        return {
            'top_cities': city_list[:20],
            'total_cities': len(city_list),
            'most_active': max(city_list, key=lambda x: x['activity_rate']) if city_list else None
        }

# ============================================
# Path Optimizer for Meeting Points
# ============================================

class PathOptimizer:
    """Optimize meeting paths for matched users"""
    
    def __init__(self, location_matcher: NationwideLocationMatcher):
        self.location_matcher = location_matcher
        
        # Venue types for suggestions
        self.venue_types = [
            'Coffee Shop', 'Restaurant', 'Bar', 'Park', 'Museum',
            'Shopping Center', 'Movie Theater', 'Beach', 'Hiking Trail'
        ]
    
    def find_optimal_meeting_point(self, user1_id: int, user2_id: int) -> Dict:
        """Find optimal meeting point between two users"""
        
        if (user1_id not in self.location_matcher.user_locations or
            user2_id not in self.location_matcher.user_locations):
            return {'error': 'User locations not available'}
        
        loc1 = self.location_matcher.user_locations[user1_id]
        loc2 = self.location_matcher.user_locations[user2_id]
        
        # Calculate midpoint
        mid_lat = (loc1.latitude + loc2.latitude) / 2
        mid_lon = (loc1.longitude + loc2.longitude) / 2
        
        # Calculate distances
        distance_user1 = self.location_matcher.haversine_distance(
            loc1.latitude, loc1.longitude, mid_lat, mid_lon
        )
        distance_user2 = self.location_matcher.haversine_distance(
            loc2.latitude, loc2.longitude, mid_lat, mid_lon
        )
        total_distance = self.location_matcher.haversine_distance(
            loc1.latitude, loc1.longitude, loc2.latitude, loc2.longitude
        )
        
        # Get location info for midpoint
        midpoint_info = self.location_matcher.location_service.get_city_info(mid_lat, mid_lon)
        
        # Find nearby venues (simulated)
        venues = self._find_nearby_venues(mid_lat, mid_lon, midpoint_info)
        
        # Determine meeting feasibility
        if total_distance < 2:
            feasibility = "excellent"
            recommendation = "Perfect distance for a quick meetup!"
        elif total_distance < 5:
            feasibility = "good"
            recommendation = "Great distance for a coffee or lunch date"
        elif total_distance < 10:
            feasibility = "moderate"
            recommendation = "Consider meeting for a special occasion"
        elif total_distance < 25:
            feasibility = "challenging"
            recommendation = "Plan ahead for a weekend meetup"
        else:
            feasibility = "difficult"
            recommendation = "Consider video dates until you can meet"
        
        return {
            'midpoint': {
                'latitude': mid_lat,
                'longitude': mid_lon,
                'city': midpoint_info.get('city', 'Unknown'),
                'state': midpoint_info.get('state', 'Unknown')
            },
            'distance_user1': distance_user1,
            'distance_user2': distance_user2,
            'total_distance': total_distance,
            'feasibility': feasibility,
            'recommendation': recommendation,
            'suggested_venues': venues,
            'estimated_travel_time': {
                'user1': self._estimate_travel_time(distance_user1),
                'user2': self._estimate_travel_time(distance_user2)
            }
        }
    
    def _find_nearby_venues(self, latitude: float, longitude: float, 
                          location_info: Dict) -> List[Dict]:
        """Find nearby venues for meeting (simulated)"""
        
        venues = []
        
        # Generate simulated venues based on location
        for i, venue_type in enumerate(self.venue_types[:5]):
            # Add small offset to coordinates for each venue
            venue_lat = latitude + (i - 2) * 0.001
            venue_lon = longitude + (i - 2) * 0.001
            
            venues.append({
                'name': f"{venue_type} at {location_info.get('city', 'Downtown')}",
                'type': venue_type.lower().replace(' ', '_'),
                'latitude': venue_lat,
                'longitude': venue_lon,
                'distance': abs(i - 2) * 0.1,
                'rating': 4.0 + (i % 3) * 0.3,
                'price_level': (i % 3) + 1
            })
        
        return venues
    
    def _estimate_travel_time(self, distance_miles: float) -> Dict:
        """Estimate travel time by different modes"""
        return {
            'walking': f"{int(distance_miles * 20)} minutes" if distance_miles < 2 else "Not recommended",
            'driving': f"{int(distance_miles * 2 + 10)} minutes",
            'transit': f"{int(distance_miles * 3 + 15)} minutes"
        }
    
    def suggest_date_ideas(self, user1_id: int, user2_id: int) -> List[Dict]:
        """Suggest date ideas based on location and distance"""
        
        meeting_point = self.find_optimal_meeting_point(user1_id, user2_id)
        
        if 'error' in meeting_point:
            return []
        
        distance = meeting_point['total_distance']
        ideas = []
        
        if distance < 2:
            ideas.extend([
                {'activity': 'Coffee Date', 'duration': '1-2 hours', 'cost': '$', 
                 'description': 'Perfect for a first meeting'},
                {'activity': 'Walk in the Park', 'duration': '1 hour', 'cost': 'Free',
                 'description': 'Great for casual conversation'},
                {'activity': 'Happy Hour', 'duration': '2-3 hours', 'cost': '$$',
                 'description': 'Relaxed atmosphere after work'}
            ])
        
        if distance < 5:
            ideas.extend([
                {'activity': 'Dinner Date', 'duration': '2-3 hours', 'cost': '$$$',
                 'description': 'Classic date night option'},
                {'activity': 'Mini Golf', 'duration': '2 hours', 'cost': '$$',
                 'description': 'Fun and interactive'},
                {'activity': 'Museum Visit', 'duration': '2-3 hours', 'cost': '$$',
                 'description': 'Cultural and conversation-friendly'}
            ])
        
        if distance < 10:
            ideas.extend([
                {'activity': 'Brunch', 'duration': '2 hours', 'cost': '$$',
                 'description': 'Weekend meetup option'},
                {'activity': 'Beach Day', 'duration': '4-6 hours', 'cost': '$',
                 'description': 'Perfect for summer'},
                {'activity': 'Concert/Show', 'duration': '3-4 hours', 'cost': '$$$',
                 'description': 'Special occasion date'}
            ])
        
        # Always include virtual options
        ideas.append({
            'activity': 'Video Date', 'duration': '1-2 hours', 'cost': 'Free',
            'description': 'Great option before meeting in person'
        })
        
        return ideas

# Export main components
__all__ = [
    'LocationService',
    'NationwideLocationMatcher',
    'HeatmapGenerator',
    'PathOptimizer'
]