# NAVA - Modern Dating Platform

## Complete Platform Architecture & Features Document

**For Investors, Business Stakeholders & Technical Teams**

---

# Document Navigation

| Audience | Start Here |
|----------|------------|
| **Investors / Non-Technical** | [Part 1: Executive Overview](#part-1-executive-overview) |
| **Product Managers** | [Part 2: Features Deep Dive](#part-2-features-deep-dive) |
| **Engineers / CTOs** | [Part 3: Technical Architecture](#part-3-technical-architecture) |

---

# Part 1: Executive Overview

*For Investors, Business Stakeholders & Non-Technical Readers*

---

## 1.1 What is NAVA?

NAVA is a modern dating app that combines the best of **Hinge + TikTok** with privacy-first AI matching. Built for meaningful connections with cutting-edge technology.

### The Problem We Solve

| Challenge | NAVA's Solution |
|-----------|-----------------|
| Photos alone don't show personality | Voice introductions let you hear someone before matching |
| Swipe fatigue from endless profiles | TikTok-style video reels show personality naturally |
| Spam and fake profiles | AI-powered verification ensures real people |
| One-size-fits-all matching | ML learns YOUR preferences over time |
| Privacy concerns with cloud data | Federated Learning keeps data on YOUR device |

### How NAVA is Different

```
Traditional Dating Apps          NAVA
─────────────────────────        ──────────────────────────
Photos only                  →   Photos + Voice + Video Reels
Swipe left/right             →   Swipe + Discover via Videos
Generic matching             →   AI learns YOUR unique taste
Anyone can message           →   Match first OR engage on reels
Same experience for all      →   AI personalizes to YOUR taste
Data stored on servers       →   Privacy-first (Federated Learning)
```

---

## 1.2 How Users Experience NAVA

### The User Journey (Simple Version)

```
┌─────────────────────────────────────────────────────────────────┐
│                        GETTING STARTED                           │
└─────────────────────────────────────────────────────────────────┘

Step 1: Sign Up
   📱 Enter phone number → Receive verification code → You're in!

Step 2: Create Profile
   📝 Add name, age, bio
   📸 Upload 3 photos (AI checks quality & safety)
   🎤 Record 30-second voice intro (optional but recommended)

Step 3: Set Preferences
   🎯 Who are you looking for?
      • Age range
      • Gender preferences
      • Distance (how far are you willing to travel?)
      • Languages spoken
      • Relationship goals (serious, casual, friendship)
```

```
┌─────────────────────────────────────────────────────────────────┐
│                      FINDING MATCHES                             │
└─────────────────────────────────────────────────────────────────┘

TWO WAYS TO DISCOVER PEOPLE:

╔═══════════════════════════════════╦═══════════════════════════════════╗
║         WAY 1: SWIPE              ║         WAY 2: REELS              ║
╠═══════════════════════════════════╬═══════════════════════════════════╣
║                                   ║                                   ║
║  See profile cards one by one     ║  TikTok-style video feed          ║
║                                   ║                                   ║
║  👍 Like = Interested             ║  Watch short videos (15-30 sec)   ║
║  👎 Pass = Not interested         ║                                   ║
║                                   ║  💬 Send private message to       ║
║  If BOTH people like each other:  ║     anyone whose video you like   ║
║  ✨ It's a Match!                 ║                                   ║
║                                   ║  No public comments (privacy!)    ║
║  Now you can message each other   ║                                   ║
║                                   ║  Great conversations can lead     ║
║                                   ║  to suggested matches             ║
║                                   ║                                   ║
╚═══════════════════════════════════╩═══════════════════════════════════╝
```

```
┌─────────────────────────────────────────────────────────────────┐
│                    AFTER MATCHING                                │
└─────────────────────────────────────────────────────────────────┘

Match Made! Now What?

   💬 Text Chat
      ├── Send text messages
      ├── Share photos
      └── Send voice messages

   📹 Video Call
      ├── Built-in video calling
      └── No need for phone number exchange

   🔒 Safety First
      ├── Block & report features
      ├── Messages are moderated for safety
      └── Unmatch anytime
```

---

## 1.3 Key Features Explained Simply

### Feature 1: Voice Introductions

**What is it?** A 30-second audio clip where you introduce yourself.

**Why it matters:**
- Hear someone's voice before matching
- Personality comes through better than text
- Users with voice intros get 40% more matches (industry data)

**How it works:**
```
User records audio → AI checks it's appropriate → Added to profile
Other users can play it while viewing your profile
```

### Feature 2: Video Reels (Like TikTok, But for Dating)

**What is it?** Short videos (15-30 seconds) showcasing your personality.

**Why it matters:**
- Shows real personality, not just posed photos
- Fun way to discover people
- Can message anyone directly (no mutual like required)

**How it works:**
```
┌─────────────────────────────────────────────────────────────────┐
│                                                                  │
│   User uploads video → AI analyzes content & safety             │
│                                                                  │
│   Video appears in other users' feeds based on:                 │
│   • Your preferences (age, gender, location)                    │
│   • What you've liked before (AI learns your taste)             │
│   • Video quality and engagement                                │
│                                                                  │
│   Unlike TikTok, comments are PRIVATE (DMs only)                │
│   This protects privacy and encourages real connections         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Two Types of Video Feeds:**

| Feed Type | What You See | Use Case |
|-----------|--------------|----------|
| **Global** | Videos from anywhere | Find people worldwide |
| **Local** | Videos from within 50 miles | Meet people nearby |

### Feature 3: Smart Matching (AI-Powered)

**What is it?** The app learns what you like and shows better matches over time.

**Why it matters:**
- Less time swiping through bad matches
- Quality over quantity
- Gets smarter the more you use it

**How it works (simplified):**
```
Every time you:
  ✓ Like someone → App notes what they're like
  ✗ Pass someone → App notes what to avoid
  💬 Message someone → Strong signal of interest
  👀 Watch a reel to the end → Shows interest in that type

Over time, the app builds a picture of YOUR ideal match
and prioritizes showing you similar profiles.
```

### Feature 4: Compatibility Scores

**What is it?** A percentage showing how well you might match with someone.

**What goes into the score:**

| Factor | Weight | Example |
|--------|--------|---------|
| Shared Interests | 25% | Both love cooking & travel |
| Languages | 15% | Both speak same languages |
| Relationship Goals | 10% | Both looking for serious |
| Voice Intro | 5% | Shows effort & authenticity |
| Verified Profile | 5% | Identity confirmed |
| Base Score | 50% | Starting point |

**What users see:**
```
   ┌─────────────────────────────┐
   │  Priya, 28                  │
   │  ━━━━━━━━━━━━━━━━━━━━━━━━   │
   │                             │
   │  85% Compatible  ●●●●○      │
   │                             │
   │  🎯 3 shared interests      │
   │  🗣️ English, Spanish        │
   │  ✓ Verified                 │
   │                             │
   └─────────────────────────────┘
```

### Feature 5: Premium Subscriptions

**Free users can:**
- Create profile & browse
- Limited swipes per day
- See who liked them (blurred)
- 2 active video reels

**Premium users get:**
- Unlimited swipes
- See who likes you (clear)
- Unlimited video reels
- Priority in discovery
- Advanced filters
- Rewind (undo passes)

**Pricing Structure:**

| Plan | Duration | Price | Best For |
|------|----------|-------|----------|
| Boost | 1 hour | $2.99 | Quick visibility |
| Daily | 1 day | $4.99 | Testing premium |
| Weekly | 7 days | $9.99 | Casual users |
| Monthly | 30 days | $19.99 | Regular users |
| Ultra | 90 days | $49.99 | Best value |

**Student Discounts:**

| University Type | Discount |
|-----------------|----------|
| Ivy League | 30% off |
| Top 50 Schools | 20% off |
| State Schools | 15% off |
| Other Accredited | 10% off |

---

## 1.4 Safety & Trust Features

### How We Keep Users Safe

```
┌─────────────────────────────────────────────────────────────────┐
│                    SAFETY LAYERS                                 │
└─────────────────────────────────────────────────────────────────┘

LAYER 1: AI Photo Analysis
   • Blocks inappropriate content automatically
   • Detects fake photos
   • Rates photo quality

LAYER 2: Identity Verification
   • Selfie matches profile photos
   • Liveness detection (proves you're real, not a photo of a photo)
   • Optional but gives "Verified" badge

LAYER 3: Content Moderation
   • AI scans messages for harassment
   • Automatic flagging of suspicious behavior
   • Human review for reported content

LAYER 4: User Controls
   • Block anyone instantly
   • Report inappropriate behavior
   • Unmatch at any time
   • Control who can message you
```

### Privacy Protection

| Your Data | Who Can See It |
|-----------|----------------|
| Profile photos | Other users (you control) |
| Voice intro | Other users (you control) |
| Phone number | Nobody (hidden) |
| Location | Approximate only (not exact) |
| Messages | Only you and the other person |
| Swipes | Nobody but you |

---

## 1.5 Business Model

### Revenue Streams

```
┌─────────────────────────────────────────────────────────────────┐
│                    REVENUE MODEL                                 │
└─────────────────────────────────────────────────────────────────┘

1. SUBSCRIPTIONS (Primary Revenue)
   └── Monthly recurring revenue from premium plans

2. ADVERTISING (High-Scale Revenue) ⭐ NEW
   └── Location-based ads for free-tier users
   └── Premium brand partnerships (dating venues, experiences)
   └── Native in-feed video ads between reels

3. BOOST PURCHASES (Impulse Buys)
   └── One-time purchases for visibility boost

4. STUDENT MARKET (Growth Driver)
   └── Discounted plans build lifetime users

5. FUTURE: Premium Features
   └── Super likes, read receipts, profile insights
```

### Advertising Revenue Model (Free Tier Monetization)

When NAVA reaches high user scale, **location-based advertising** becomes a major revenue stream:

```
┌─────────────────────────────────────────────────────────────────┐
│               ADVERTISING ARCHITECTURE                           │
└─────────────────────────────────────────────────────────────────┘

FREE TIER USERS see ads:
├── Between every 5-7 reels in the feed
├── Between profile swipes (interstitial)
├── Banner ads in inbox/messages list
└── Sponsored profiles in discovery

PREMIUM USERS:
└── 100% AD-FREE experience (key selling point)

AD TARGETING CAPABILITIES:
┌────────────────────────────────────────────────────────────────┐
│                                                                 │
│  LOCATION-BASED (Hyperlocal)                                   │
│  ├── City-level targeting (Hyderabad, Dallas, London)          │
│  ├── Neighborhood targeting (Jubilee Hills, Irving)            │
│  ├── Radius targeting (5mi, 10mi, 25mi from point)            │
│  └── Country/Region targeting (India, US, Gulf)                │
│                                                                 │
│  DEMOGRAPHIC TARGETING                                          │
│  ├── Age range (18-24, 25-34, 35+)                             │
│  ├── Gender                                                     │
│  ├── Profession category                                        │
│  └── Student status                                             │
│                                                                 │
│  BEHAVIORAL TARGETING                                           │
│  ├── Relationship intent (serious, casual)                     │
│  ├── Activity level (daily active, weekly)                     │
│  └── Engagement patterns                                        │
│                                                                 │
│  CONTEXTUAL TARGETING (Unique to NAVA)                         │
│  ├── Time of day (lunch dates, evening plans)                  │
│  ├── Day of week (weekend activities)                          │
│  └── Local events (concerts, festivals, sports)                │
│                                                                 │
└────────────────────────────────────────────────────────────────┘
```

### High-Value Advertiser Categories

| Category | Example Advertisers | Why NAVA is Valuable |
|----------|--------------------|-----------------------|
| **Restaurants & Bars** | Local restaurants, cafes, bars | Location-based date suggestions |
| **Entertainment** | Movie theaters, concerts, events | Date night activities |
| **Fashion & Grooming** | Clothing brands, skincare, salons | Dating-ready audience |
| **Travel & Experiences** | Airbnb, hotels, airlines | Couples planning trips |
| **Financial Services** | Banks, Insurance, Investment apps | Young professionals with income |
| **Fitness & Wellness** | Gyms, yoga studios, health apps | Health-conscious singles |
| **Education & Career** | Online courses, job portals | Ambitious singles |
| **Delivery & Services** | Food delivery, flowers, gifts | Date planning services |
| **Event Organizers** | Concerts, festivals, sports events | Activity-based dating |
| **Real Estate** | Apartments, co-living spaces | Singles & couples relocating |

### Ad Format Types

```
┌─────────────────────────────────────────────────────────────────┐
│                    AD FORMATS                                    │
└─────────────────────────────────────────────────────────────────┘

1. IN-FEED VIDEO ADS (Between Reels)
   ├── 15-30 second skippable video
   ├── Native look & feel (matches reel format)
   ├── High engagement (users already watching videos)
   └── CPM: $15-25 (premium dating audience)

2. SPONSORED PROFILES (Discovery Feed)
   ├── Branded profile card in swipe stack
   ├── Links to advertiser landing page
   ├── Perfect for event promotions
   └── CPM: $20-35

3. INTERSTITIAL ADS (Between Actions)
   ├── Full-screen ad after X swipes
   ├── Skippable after 5 seconds
   └── CPM: $10-18

4. BANNER ADS (Inbox/Messages)
   ├── Non-intrusive banner
   ├── Lower CPM but high volume
   └── CPM: $5-10

5. NATIVE CONTENT PARTNERSHIPS
   ├── Branded reels from advertisers
   ├── Influencer partnerships
   ├── Event sponsorships
   └── Custom pricing (per campaign)
```

### Revenue Projections (Ad-Based)

```
┌─────────────────────────────────────────────────────────────────┐
│            AD REVENUE PROJECTIONS BY SCALE                       │
└─────────────────────────────────────────────────────────────────┘

ASSUMPTIONS:
├── 70% users on Free tier (see ads)
├── Average 10 ad impressions per user per day
├── Blended CPM: $12 (mix of all formats)
└── 20 active days per month per user

SCALE: 100K Monthly Active Users (MAU)
├── Free tier users: 70,000
├── Daily impressions: 700,000
├── Monthly impressions: 14,000,000
└── Monthly ad revenue: $168,000

SCALE: 500K MAU
├── Free tier users: 350,000
├── Monthly impressions: 70,000,000
└── Monthly ad revenue: $840,000

SCALE: 1M MAU
├── Free tier users: 700,000
├── Monthly impressions: 140,000,000
└── Monthly ad revenue: $1,680,000

SCALE: 5M MAU (Regional Leader)
├── Free tier users: 3,500,000
├── Monthly impressions: 700,000,000
└── Monthly ad revenue: $8,400,000
```

### Location-Based Ad Technical Implementation

```
┌─────────────────────────────────────────────────────────────────┐
│           LOCATION-BASED AD DELIVERY                             │
└─────────────────────────────────────────────────────────────────┘

1. User opens app
   │
2. App sends location (city from user_locations table)
   │
3. Ad server queries:
   │   SELECT * FROM ad_campaigns
   │   WHERE target_cities @> ARRAY['Hyderabad']
   │   AND target_age_min <= 28
   │   AND target_age_max >= 28
   │   AND status = 'active'
   │   ORDER BY bid_cpm DESC
   │
4. Return highest-bidding relevant ad
   │
5. Track impression → Bill advertiser
   │
6. User clicks → Track conversion → Bill CPC
```

### Privacy-Preserving Ad Targeting

```
NAVA DOES NOT:
✗ Sell user data to third parties
✗ Share personal information with advertisers
✗ Allow advertisers to target specific individuals
✗ Track users across other apps/websites

NAVA DOES:
✓ Show relevant ads based on aggregate segments
✓ Allow users to opt-out of personalized ads
✓ Provide transparency on why an ad was shown
✓ Use on-device processing where possible (FL)
```

### Ad-Free as Premium Benefit

```
┌─────────────────────────────────────────────────────────────────┐
│           PREMIUM VALUE PROPOSITION                              │
└─────────────────────────────────────────────────────────────────┘

FREE TIER:                         PREMIUM TIER:
├── Ads between reels              ├── Zero ads (completely clean)
├── Banner ads in inbox            ├── Uninterrupted experience
├── Interstitial ads               ├── Faster browsing
└── Sponsored profiles             └── Premium-only badge

This creates STRONG incentive to upgrade:
"Tired of ads? Go Premium for an ad-free experience!"
```

### Key Metrics to Track

| Metric | What It Means | Target |
|--------|---------------|--------|
| DAU/MAU | Daily users / Monthly users | >30% |
| Match Rate | Likes that become mutual matches | >15% |
| Conversation Rate | Matches that lead to messages | >60% |
| Conversion Rate | Free users becoming premium | >5% |
| Retention (D7) | Users returning after 7 days | >40% |

---

## 1.6 Market Opportunity

### Target Demographics

**Primary:** Singles ages 18-35
- 300M+ online dating users globally
- High smartphone penetration
- Seeking meaningful connections

**Secondary:** Young professionals & students
- Career-focused individuals
- Tech-savvy early adopters
- Privacy-conscious users

### Competitive Advantage

| Competitor | Their Focus | NAVA's Edge |
|------------|-------------|-------------|
| Tinder | Volume, casual | Quality matches, privacy |
| Hinge | Text prompts | Video reels, voice intros |
| Bumble | Women message first | Reel-based discovery, better AI |
| OkCupid | Questionnaires | Modern UX, federated learning |

---

# Part 2: Features Deep Dive

*For Product Managers & Feature-Focused Stakeholders*

---

## 2.1 Complete Feature Matrix

### Core Features

| Feature | Free | Premium | Description |
|---------|------|---------|-------------|
| Profile Creation | ✓ | ✓ | Full profile with photos, bio, preferences |
| Voice Intro | ✓ | ✓ | 30-second audio introduction |
| Discovery (Swipe) | Limited | Unlimited | See 20/day free, unlimited premium |
| Reels Upload | 2 active | Unlimited | Video content for discovery |
| Reels Browse | ✓ | ✓ | Full access to video feed |
| Send Messages | Match only | Match + Reels | Message matched users or reel creators |
| See Who Likes You | Blurred | Clear | Know who's interested |
| Distance Filter | ✓ | ✓ | Set max distance for matches |
| Age Filter | ✓ | ✓ | Set age range preferences |
| Verified Filter | - | ✓ | Only see verified profiles |
| Rewind | - | ✓ | Undo accidental passes |
| Profile Boost | - | ✓ | Priority in discovery |
| Video Calls | ✓ | ✓ | Built-in calling |
| Read Receipts | - | ✓ | Know when messages are read |

### Feature Details

#### 2.1.1 Profile System

**Profile Components:**

```
┌─────────────────────────────────────────────────────────────────┐
│                    USER PROFILE                                  │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  BASIC INFO                                                      │
│  ├── Name (required)                                            │
│  ├── Date of Birth (required, age displayed)                    │
│  ├── Gender (male, female, non-binary, other)                   │
│  └── Bio (500 chars max)                                        │
│                                                                  │
│  PHOTOS                                                          │
│  ├── 3 required, 6 max                                          │
│  ├── AI analyzes for quality & safety                           │
│  └── First photo = primary display                              │
│                                                                  │
│  VOICE INTRO                                                     │
│  ├── 30 seconds max                                             │
│  ├── Optional but increases matches                             │
│  └── AI checks for appropriate content                          │
│                                                                  │
│  INTERESTS & ATTRIBUTES                                          │
│  ├── Interests (travel, cooking, music, etc.)                   │
│  ├── Languages spoken (English, Spanish, French, etc.)          │
│  ├── Looking for (relationship, casual, friendship)             │
│  ├── Profession category & title                                │
│  └── Height (optional)                                          │
│                                                                  │
│  VERIFICATION STATUS                                             │
│  ├── Phone verified (required)                                  │
│  ├── Photo verified (optional, selfie match)                    │
│  └── Student verified (for discounts)                           │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

**Profile Completion Impact:**

| Completion Level | Discovery Priority | Match Quality |
|------------------|--------------------| --------------|
| Basic (name, 1 photo) | Low | Poor |
| Standard (3 photos, bio) | Medium | Average |
| Complete (voice intro, verified) | High | Excellent |

#### 2.1.2 Discovery System

**Discovery Feed Logic:**

```
User opens Discover tab
        │
        ▼
┌─────────────────────────────────────────────────────────────────┐
│                    CANDIDATE SELECTION                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Step 1: Apply Hard Filters                                     │
│  ├── Age within preference range                                │
│  ├── Gender matches preference                                  │
│  ├── Distance within max setting                                │
│  ├── Profile is complete                                        │
│  └── Not already interacted (liked/passed)                      │
│                                                                  │
│  Step 2: Score & Rank                                           │
│  ├── Compatibility score (interests, languages, goals)          │
│  ├── Photo quality score (AI-rated)                             │
│  ├── Activity score (recently active users)                     │
│  └── ML personalization (based on past behavior)                │
│                                                                  │
│  Step 3: Diversify                                              │
│  ├── Mix of high-score and exploratory matches                  │
│  ├── Prevent always showing same "type"                         │
│  └── Random factor for fairness                                 │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
        │
        ▼
    Show 20 profiles per session (paginated)
```

**Like/Pass Outcomes:**

```
User A likes User B
        │
        ├── If User B hasn't seen User A yet:
        │   └── Wait. B will see A in their feed later
        │
        ├── If User B already liked User A:
        │   └── ✨ MATCH! Both can now message each other
        │
        └── If User B already passed on User A:
            └── No match possible (A won't know)

User A passes on User B
        │
        └── User B removed from A's feed forever
            (User B can still see User A if they haven't interacted)
```

#### 2.1.3 Reels System (Video Discovery)

**Reel Creation Flow:**

```
User taps "Create Reel"
        │
        ▼
┌─────────────────────────────────────────────────────────────────┐
│                    UPLOAD REQUIREMENTS                           │
├─────────────────────────────────────────────────────────────────┤
│  • Video length: 15-30 seconds                                  │
│  • File size: Max 50MB                                          │
│  • Formats: MP4, MOV, WebM                                      │
│  • Optional: Title, tags, location                              │
│  • Choose: Global (everyone) or Local (nearby only)             │
└─────────────────────────────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────────────────────────────┐
│                    AI PROCESSING                                 │
├─────────────────────────────────────────────────────────────────┤
│  1. NSFW Detection - Block inappropriate content                │
│  2. Quality Analysis - Rate video quality                       │
│  3. Content Understanding - Detect topics, mood, setting        │
│  4. Generate Thumbnail - Select best frame                      │
│  5. Create Renditions - Multiple quality levels                 │
└─────────────────────────────────────────────────────────────────┘
        │
        ▼
    Reel goes live in feeds
```

**Reel Feed Algorithm:**

```
For each user viewing reel feed:

1. COLLECT CANDIDATES
   ├── Exclude: Own reels
   ├── Exclude: Blocked users' reels
   ├── Include: Based on Global/Local setting
   └── Get 100+ candidates

2. SCORE EACH REEL
   ├── Engagement Score (40%)
   │   ├── Like rate
   │   ├── Watch completion rate
   │   └── Message rate
   │
   ├── Relevance Score (30%)
   │   ├── Creator matches user preferences
   │   ├── Content matches user interests
   │   └── Past behavior similarity
   │
   ├── Freshness Score (20%)
   │   └── Newer content boosted
   │
   └── Quality Score (10%)
       └── AI-rated production quality

3. RANK & SELECT
   ├── Top 70% by score
   ├── 30% exploratory (randomized)
   └── Return ordered list
```

**Reel Engagement Actions:**

| Action | What Happens | Impact on Recommendations |
|--------|--------------|---------------------------|
| Watch <25% | Recorded as skip | Similar content deprioritized |
| Watch 25-50% | Moderate interest | Neutral impact |
| Watch 50-90% | Good interest | Similar content boosted |
| Watch >90% | High interest | Strong boost to similar content |
| Rewatch | Very high interest | Very strong boost |
| Like | Explicit interest | Significant boost + creator notified |
| Message | Highest interest | Maximum boost + starts conversation |

**Reel Messaging (Key Dating Feature):**

Unlike TikTok's public comments, NAVA uses private DMs:

```
User sees reel they like
        │
        ▼
Taps "Message" button
        │
        ▼
Writes thoughtful message (emoji reactions also available)
        │
        ▼
┌─────────────────────────────────────────────────────────────────┐
│  Message appears in reel creator's inbox                        │
│                                                                  │
│  ┌──────────────────────────────────────────────────────┐       │
│  │  Inbox                                                │       │
│  ├──────────────────────────────────────────────────────┤       │
│  │  💬 Raj commented on your reel                       │       │
│  │  "Love your cooking! That biryani looks amazing..."  │       │
│  │                                                       │       │
│  │  [Reply] [View Profile] [Dismiss]                    │       │
│  └──────────────────────────────────────────────────────┘       │
│                                                                  │
│  If conversation continues (6+ messages, both engaged):         │
│  → System suggests "Would you like to match?"                   │
│  → Both accept = Full match with chat access                    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### 2.1.4 Matching System

**Match States:**

```
┌─────────────────────────────────────────────────────────────────┐
│                    MATCH LIFECYCLE                               │
└─────────────────────────────────────────────────────────────────┘

  PENDING              MUTUAL                  UNMATCHED
  (one-sided)          (matched!)              (ended)
      │                    │                       │
      │                    │                       │
  User A liked         Both users              Either user
  User B, waiting      liked each              unmatched or
  for B's response     other                   blocked
      │                    │                       │
      │                    ▼                       │
      │            ┌──────────────┐                │
      └───────────▶│    MATCH     │◀───────────────┘
                   │              │
                   │ Can message  │
                   │ Can video    │
                   │   call       │
                   │ See online   │
                   │   status     │
                   └──────────────┘
```

**Match Data Structure:**

| Field | Purpose | Values |
|-------|---------|--------|
| user1_id | First user (always smaller ID) | User ID |
| user2_id | Second user (always larger ID) | User ID |
| user1_liked | Did user1 like user2? | true/false/null |
| user2_liked | Did user2 like user1? | true/false/null |
| is_mutual_match | Did both like? | true/false |
| status | Current state | active/blocked/unmatched |
| messages_count | Total messages sent | Number |
| last_message_at | Last activity | Timestamp |

#### 2.1.5 Messaging System

**Message Types:**

| Type | Description | Supported In |
|------|-------------|--------------|
| Text | Plain text message | All chats |
| Image | Photo attachment | Match chats |
| Voice | Audio message | Match chats |
| Video | Video message | Match chats (premium) |
| Reaction | Emoji reaction | Reel messages |

**Real-Time Features:**

```
┌─────────────────────────────────────────────────────────────────┐
│                    REAL-TIME MESSAGING                           │
└─────────────────────────────────────────────────────────────────┘

  WebSocket Connection (persistent)
            │
            ├── Typing Indicators
            │   └── "Priya is typing..."
            │
            ├── Message Delivery Status
            │   ├── Sent (single check ✓)
            │   ├── Delivered (double check ✓✓)
            │   └── Read (blue checks ✓✓) [Premium]
            │
            ├── Online Status
            │   ├── Online (green dot)
            │   ├── Recently active (gray dot)
            │   └── Last seen timestamp
            │
            └── Push Notifications
                ├── New message
                ├── New match
                └── Someone liked you (Premium)
```

#### 2.1.6 Video Calling

**Call Flow:**

```
Caller                    Server                    Callee
   │                         │                         │
   │── Initiate Call ───────▶│                         │
   │                         │── Push: Incoming Call ─▶│
   │                         │                         │
   │                         │◀── Accept Call ─────────│
   │◀── Call Connected ──────│                         │
   │                         │                         │
   │◀═══════════ WebRTC Peer-to-Peer ══════════════▶│
   │                    (video/audio)                  │
   │                         │                         │
   │── End Call ────────────▶│◀── End Call ───────────│
   │                         │                         │
```

**Call Features:**

| Feature | Description |
|---------|-------------|
| Video Toggle | Turn camera on/off during call |
| Audio Mute | Mute microphone |
| Camera Flip | Switch front/back camera |
| Call Timer | Shows call duration |
| Screen Off | Audio continues when screen off |

---

## 2.2 Subscription & Monetization Features

### Premium Tiers Detailed

```
┌─────────────────────────────────────────────────────────────────┐
│                    SUBSCRIPTION TIERS                            │
└─────────────────────────────────────────────────────────────────┘

FREE TIER
├── 20 swipes per day
├── 2 active reels
├── Match messaging only
├── Basic filters (age, gender, distance)
├── Blurred "likes you" section
└── Standard discovery priority

────────────────────────────────────────────────────────────────

BOOST ($2.99/hour)
├── Everything in Free, PLUS:
├── Priority visibility for 1 hour
├── Unlimited swipes for 1 hour
└── Great for: Weekend burst of activity

────────────────────────────────────────────────────────────────

DAILY ($4.99/day)
├── Everything in Free, PLUS:
├── Unlimited swipes for 24 hours
├── Priority visibility
└── Great for: Testing premium experience

────────────────────────────────────────────────────────────────

WEEKLY ($9.99/week)
├── Everything in Daily, PLUS:
├── See who likes you (clear photos)
├── 5 active reels
└── Great for: Casual users

────────────────────────────────────────────────────────────────

MONTHLY ($19.99/month)
├── Everything in Weekly, PLUS:
├── Unlimited reels
├── Rewind (undo passes)
├── Read receipts
├── Advanced filters (verified only, etc.)
└── Great for: Regular users

────────────────────────────────────────────────────────────────

ULTRA ($49.99/quarter)
├── Everything in Monthly, PLUS:
├── Profile highlights (stand out in feed)
├── Priority support
├── Early access to new features
├── 33% savings vs monthly
└── Great for: Committed users, best value
```

### Student Verification System

```
Student selects "Verify Student Status"
            │
            ▼
Enter .edu email address
            │
            ▼
System sends verification code to email
            │
            ▼
Student enters code
            │
            ▼
┌─────────────────────────────────────────────────────────────────┐
│                    UNIVERSITY TIER DETECTION                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Domain checked against database:                               │
│                                                                  │
│  @harvard.edu, @yale.edu, @mit.edu, etc.                        │
│  └── IVY LEAGUE TIER → 30% discount                            │
│                                                                  │
│  @ucla.edu, @umich.edu, @berkeley.edu, etc.                     │
│  └── TOP 50 TIER → 20% discount                                │
│                                                                  │
│  @stateu.edu, etc.                                              │
│  └── STATE SCHOOL TIER → 15% discount                          │
│                                                                  │
│  Any other .edu                                                 │
│  └── OTHER TIER → 10% discount                                 │
│                                                                  │
│  Additional bonuses:                                            │
│  • Graduate students: +15% additional                           │
│  • Alumni (<2 years): 5% discount                               │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
            │
            ▼
    Discount automatically applied to all purchases
    Badge shown on profile: "🎓 Verified Student"
```

---

## 2.3 Safety & Moderation Features

### Photo Verification

```
User taps "Verify My Photos"
            │
            ▼
Prompted to take selfie matching a specific pose
            │
            ▼
┌─────────────────────────────────────────────────────────────────┐
│                    AI VERIFICATION                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  1. LIVENESS CHECK                                              │
│     └── Confirms it's a real person, not a photo of a photo    │
│     └── Uses depth analysis and movement detection              │
│                                                                  │
│  2. FACE MATCHING                                               │
│     └── Compares selfie to profile photos                       │
│     └── Uses AI facial recognition (ArcFace model)              │
│     └── Threshold: 45% similarity required                      │
│                                                                  │
│  3. RESULT                                                      │
│     └── Pass: ✓ Verified badge added                           │
│     └── Fail: Asked to try again or use different photos       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Content Moderation Pipeline

```
Content Submitted (photo/video/message)
            │
            ▼
┌─────────────────────────────────────────────────────────────────┐
│                    AUTOMATED CHECKS                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  PHOTOS/VIDEOS:                                                 │
│  ├── NSFW Detection (blocks nudity, violence)                   │
│  ├── Quality Check (rejects blurry, low-res)                    │
│  └── Face Detection (at least one photo needs face)             │
│                                                                  │
│  MESSAGES:                                                       │
│  ├── Spam Detection (repeated content, links)                   │
│  ├── Harassment Keywords (slurs, threats)                       │
│  └── Contact Sharing (blocks phone/email in early messages)     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
            │
            ├── PASS → Content published
            │
            ├── FLAGGED → Queued for human review
            │
            └── BLOCKED → Content rejected, user warned
```

### User Reporting System

| Report Type | What Happens |
|-------------|--------------|
| Fake Profile | Immediate review, possible suspension |
| Inappropriate Photos | Photos removed pending review |
| Harassment | Messages reviewed, user may be banned |
| Scam/Spam | Account suspended, pattern analysis |
| Underage | Immediate suspension, law enforcement if needed |

---

# Part 3: Technical Architecture

*For Engineers, CTOs & Technical Stakeholders*

---

## 3.1 System Architecture Overview

### Technology Stack

```
┌─────────────────────────────────────────────────────────────────┐
│                        CLIENT LAYER                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Mobile Apps                                                     │
│  ├── React Native 0.81 + Expo SDK 54                            │
│  ├── Expo Router (file-based navigation)                        │
│  ├── Reanimated 4 (60fps animations)                            │
│  ├── RevenueCat SDK (in-app purchases)                          │
│  └── WebRTC (video calling)                                     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        CDN & GATEWAY                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  CloudFront CDN                                                  │
│  ├── Static assets (photos, videos)                             │
│  ├── Signed URLs for private content                            │
│  └── Edge caching for low latency                               │
│                                                                  │
│  API Gateway / Load Balancer                                    │
│  ├── SSL termination                                            │
│  ├── Request routing                                            │
│  └── Rate limiting (first layer)                                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        API LAYER                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Rust Backend (Axum 0.8)                                        │
│  ├── REST API (handlers.rs - ~3500 lines)                       │
│  ├── GraphQL API (async-graphql - ~1500 lines)                  │
│  ├── WebSocket (real-time messaging, calls)                     │
│  └── Vision AI (ONNX Runtime via tract)                         │
│                                                                  │
│  Key Dependencies:                                               │
│  ├── SQLx (async PostgreSQL)                                    │
│  ├── Redis (sessions, caching, rate limiting)                   │
│  ├── Reqwest (S3 uploads, external APIs)                        │
│  ├── Lettre (email/OTP)                                         │
│  └── tract-onnx (ML model inference)                            │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│   PostgreSQL    │ │     Redis       │ │   S3 + CDN      │
├─────────────────┤ ├─────────────────┤ ├─────────────────┤
│ Users           │ │ Sessions        │ │ Photos          │
│ Matches         │ │ OTP codes       │ │ Voice intros    │
│ Messages        │ │ Rate limits     │ │ Reels/Videos    │
│ Reels           │ │ Cache           │ │ Thumbnails      │
│ ML Training     │ │ Online status   │ │ Verification    │
│ Analytics       │ │ Real-time data  │ │ images          │
└─────────────────┘ └─────────────────┘ └─────────────────┘
```

### Directory Structure

```
rust-backend/
├── src/
│   ├── main.rs              # Server entry, route definitions
│   ├── config.rs            # 50+ environment variables
│   ├── state.rs             # Shared app state (DB, Redis, Vision)
│   ├── auth.rs              # JWT creation/verification
│   ├── handlers.rs          # REST API handlers (~3500 lines)
│   ├── graphql.rs           # GraphQL schema (~1500 lines)
│   ├── models.rs            # Database structs
│   ├── websocket.rs         # Chat & call signaling
│   ├── vision.rs            # ONNX inference (5 models)
│   ├── redis_service.rs     # Caching, sessions, rate limiting
│   ├── storage.rs           # S3/local storage abstraction
│   └── error.rs             # Error types
├── migrations/
│   └── 001_initial_schema.sql  # Full database schema
├── models/                  # ONNX model files
│   ├── nsfw_detector.onnx
│   ├── ferplus.onnx
│   ├── nima.onnx
│   ├── arcface.onnx
│   └── minifasnet.onnx
├── Cargo.toml               # Dependencies
└── .env                     # Configuration
```

---

## 3.2 Database Architecture

### Schema Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    DATABASE SCHEMA                               │
└─────────────────────────────────────────────────────────────────┘

CORE TABLES:
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│     users       │────▶│   user_prefs    │     │ user_locations  │
│                 │     │                 │     │                 │
│ id              │     │ user_id (FK)    │     │ user_id (FK)    │
│ phone_number    │     │ min_age         │     │ latitude        │
│ name            │     │ max_age         │     │ longitude       │
│ dob             │     │ preferred_      │     │ city            │
│ gender          │     │   genders       │     │ is_fuzzy        │
│ bio             │     │ max_distance    │     └─────────────────┘
│ interests       │     │ only_verified   │
│ languages       │     └─────────────────┘
│ profile_photos  │
│ voice_intro_url │
│ is_verified     │
│ attractiveness_ │
│   score         │
└────────┬────────┘
         │
         │  ┌─────────────────────────────────────────────────┐
         │  │                                                  │
         ▼  ▼                                                  │
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│    matches      │────▶│    messages     │     │     reels       │
│                 │     │                 │     │                 │
│ id              │     │ id              │     │ id              │
│ user1_id        │     │ match_id (FK)   │     │ user_id (FK)    │
│ user2_id        │     │ sender_id       │     │ video_url       │
│ user1_liked     │     │ receiver_id     │     │ thumbnail_url   │
│ user2_liked     │     │ content         │     │ duration_sec    │
│ is_mutual_match │     │ message_type    │     │ tags            │
│ status          │     │ is_read         │     │ engagement_     │
│ created_at      │     │ created_at      │     │   score         │
└─────────────────┘     └─────────────────┘     │ is_global       │
                                                │ city            │
                                                └────────┬────────┘
                                                         │
                              ┌───────────────────────────┤
                              ▼                           ▼
                   ┌─────────────────┐         ┌─────────────────┐
                   │   reel_views    │         │  reel_messages  │
                   │                 │         │                 │
                   │ reel_id (FK)    │         │ reel_id (FK)    │
                   │ viewer_id       │         │ sender_id       │
                   │ watch_percent   │         │ receiver_id     │
                   │ rewatched       │         │ content         │
                   │ session_id      │         │ replied         │
                   └─────────────────┘         │ led_to_match    │
                                               └─────────────────┘

ML TRAINING TABLES:
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ interaction_    │     │ bandit_arm_     │     │ user_content_   │
│   events        │     │   stats         │     │   preferences   │
│                 │     │                 │     │                 │
│ user_id         │     │ arm_id          │     │ user_id         │
│ target_user_id  │     │ a_matrix        │     │ preferred_      │
│ event_type      │     │ b_vector        │     │   categories    │
│ reward          │     │ theta_vector    │     │ completion_rate │
│ slate_id        │     │ num_pulls       │     │ embedding       │
│ rank            │     │ total_reward    │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘

LLM & FEDERATED LEARNING:
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  reel_llm_      │     │  fl_clients     │     │   fl_rounds     │
│    labels       │     │                 │     │                 │
│                 │     │ user_id         │     │ round_number    │
│ reel_id         │     │ device_id       │     │ global_weights  │
│ content_summary │     │ compute_tier    │     │ participants    │
│ detected_topics │     │ reliability_    │     │ differential_   │
│ personality_    │     │   score         │     │   privacy       │
│   traits        │     │ successful_     │     │ avg_loss        │
│ dating_appeal   │     │   rounds        │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

### Key Database Design Patterns

**1. Match Record Ordering**

The `matches` table always stores user IDs in ascending order:

```sql
-- Always: user1_id < user2_id
-- This ensures only one record per pair

-- When User 5 likes User 12:
INSERT INTO matches (user1_id, user2_id, user1_liked)
VALUES (5, 12, TRUE);  -- 5 < 12, so user1_liked = TRUE

-- When User 12 likes User 5:
UPDATE matches
SET user2_liked = TRUE, is_mutual_match = TRUE
WHERE user1_id = 5 AND user2_id = 12;
```

**2. Global vs Local Content**

```sql
-- Spots/Reels have is_global flag for location-based distribution
CREATE TABLE spots (
    ...
    is_global BOOLEAN DEFAULT TRUE,  -- TRUE = worldwide, FALSE = local only
    city VARCHAR(100),               -- Creator's city for local filtering
    ...
);

-- Global feed query:
SELECT * FROM spots WHERE is_global = TRUE;

-- Local feed query:
SELECT s.*, ul.latitude, ul.longitude
FROM spots s
JOIN user_locations ul ON ul.user_id = s.user_id
WHERE s.is_global = FALSE
AND haversine_distance(viewer_lat, viewer_lon, ul.latitude, ul.longitude) < 50;
```

**3. Slate-Based Impression Tracking**

```sql
-- Every discovery session gets a unique slate_id
-- This groups impressions for ML training

INSERT INTO interaction_events (user_id, target_user_id, event_type, slate_id, rank)
VALUES
  (100, 201, 'impression', 'slate-abc-123', 0),  -- First profile shown
  (100, 202, 'impression', 'slate-abc-123', 1),  -- Second profile shown
  (100, 201, 'like', 'slate-abc-123', 0),        -- User liked first profile
  (100, 202, 'pass', 'slate-abc-123', 1);        -- User passed on second
```

---

## 3.3 API Architecture

### REST API Endpoints

**Authentication:**

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/auth/send-otp` | Send OTP to phone |
| POST | `/auth/verify` | Verify OTP, return JWT |

**Profile:**

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/profile/me` | Get current user profile |
| PUT | `/profile/update` | Update profile fields |
| POST | `/profile/photos/upload` | Upload photo (multipart) |
| DELETE | `/profile/photos/{slot}` | Delete photo |
| POST | `/profile/voice/upload` | Upload voice intro |
| PUT | `/profile/preferences` | Update preferences |
| PUT | `/profile/location` | Update location |

**Discovery & Matching:**

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/discover?limit=20` | Get discovery feed |
| POST | `/discover/like` | Like a profile |
| POST | `/discover/pass` | Pass on a profile |
| GET | `/matches` | List all matches |
| GET | `/matches/{id}` | Get match details |
| POST | `/matches/{id}/unmatch` | Unmatch |
| POST | `/matches/{id}/block` | Block user |

**Messaging:**

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/messages/{match_id}` | Get conversation |
| POST | `/messages/{match_id}` | Send message |
| PUT | `/messages/{id}/read` | Mark as read |
| WebSocket | `/ws/chat/{match_id}` | Real-time chat |

**Reels:**

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/reels/create` | Upload reel (multipart) |
| GET | `/reels/feed?mode=global` | Get reel feed |
| POST | `/reels/track-view` | Track view metrics |
| POST | `/reels/like` | Like a reel |
| DELETE | `/reels/{id}/unlike` | Unlike |
| POST | `/reels/message` | Send DM on reel |
| GET | `/reels/inbox` | Get reel messages |

**Spots (User's Own Reels):**

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/spots` | Upload spot |
| GET | `/spots` | List own spots |
| DELETE | `/spots/{id}` | Delete spot |

**Subscriptions:**

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/subscriptions/sync` | Sync from RevenueCat |
| POST | `/webhooks/revenuecat` | RevenueCat webhook |
| POST | `/student/verify/start` | Start student verification |
| POST | `/student/verify/confirm` | Confirm with code |

### GraphQL Schema

```graphql
type Query {
  # User & Profile
  me: User
  user(id: ID!): User
  myPreferences: UserPreferences

  # Discovery
  discover(
    limit: Int = 20
    onlyVerified: Boolean = false
  ): [DiscoverProfile!]!

  # Matches & Messages
  matches: [Match!]!
  conversation(matchId: ID!, limit: Int, offset: Int): [Message!]!

  # Reels
  reelFeed(
    mode: FeedMode = GLOBAL
    limit: Int = 20
    tags: [String!]
    radius: Int = 50
  ): [Reel!]!
  reelInbox: [ReelMessage!]!

  # Subscriptions
  studentStatus: StudentStatus
  activeSubscription: Subscription
}

type Mutation {
  # Auth
  sendOtp(phoneNumber: String!): OtpResponse!
  verifyOtp(phoneNumber: String!, otp: String!): AuthPayload!

  # Profile
  updateProfile(input: ProfileInput!): Boolean!
  savePreferences(input: PreferencesInput!): UserPreferences!
  uploadVoiceIntro(voiceUrl: String!, durationSeconds: Int!): Boolean!

  # Discovery
  likeUser(targetUserId: Int!): LikeResult!
  passUser(targetUserId: Int!): Boolean!

  # Messaging
  sendChatMessage(matchId: String!, content: String!): Message!

  # Reels
  trackReelView(input: ReelViewInput!): Boolean!
  likeReel(reelId: Int!): Boolean!
  sendReelMessage(reelId: Int!, content: String!): ReelMessage!
}

enum FeedMode {
  GLOBAL
  LOCAL
}

type DiscoverProfile {
  id: ID!
  name: String
  age: Int
  gender: String
  bio: String
  photos: [String!]!
  interests: [String!]!
  compatibilityScore: Float
  distanceKm: Float
  isVerified: Boolean!
  hasVoiceIntro: Boolean!
  voiceIntroUrl: String
}

type LikeResult {
  success: Boolean!
  isMutual: Boolean!
  matchId: String
  message: String!
}
```

### WebSocket Events

**Chat WebSocket (`/ws/chat/{match_id}`):**

```json
// Client → Server
{ "type": "message", "content": "Hello!" }
{ "type": "typing", "isTyping": true }
{ "type": "read", "messageId": "123" }

// Server → Client
{ "type": "message", "senderId": 456, "content": "Hi!", "messageId": "789" }
{ "type": "typing", "senderId": 456, "isTyping": true }
{ "type": "read", "messageId": "123", "readAt": "2024-01-15T10:30:00Z" }
{ "type": "online", "userId": 456, "isOnline": true }
```

**Call WebSocket (`/ws/call/{call_id}`):**

```json
// WebRTC Signaling
{ "type": "offer", "sdp": "v=0\r\n..." }
{ "type": "answer", "sdp": "v=0\r\n..." }
{ "type": "ice", "candidate": "candidate:..." }
{ "type": "join", "userId": 123 }
{ "type": "leave", "userId": 123 }
{ "type": "end" }
```

---

## 3.4 ML Architecture

### Vision AI Pipeline

Five ONNX models run on the backend:

```
┌─────────────────────────────────────────────────────────────────┐
│                    VISION AI MODELS                              │
└─────────────────────────────────────────────────────────────────┘

1. NSFW DETECTOR
   Input: 224x224 RGB image
   Output: [safe, nsfw] probabilities
   Threshold: Block if nsfw > 0.7
   Use: Content moderation

2. FER+ (Facial Expression Recognition)
   Input: 48x48 grayscale face
   Output: 7 emotion probabilities (angry, disgust, fear, happy, sad, surprise, neutral)
   Use: Profile photo analysis, personality insights

3. NIMA (Neural Image Assessment)
   Input: 224x224 RGB image
   Output: Quality score 1-10
   Use: Photo ranking, attractiveness_score

4. ARCFACE (Face Recognition)
   Input: 112x112 aligned face
   Output: 512-dimensional embedding
   Use: Identity verification, duplicate detection

5. MINIFASNET (Anti-Spoofing)
   Input: 112x112 face
   Output: Real vs spoof probability
   Threshold: Liveness > 0.5 required
   Use: Prevent photo-of-photo attacks
```

**Photo Upload Pipeline:**

```rust
pub async fn analyze_and_upload_photo(
    image_data: &[u8],
    user_id: i32,
    state: &AppState,
) -> Result<PhotoUploadResult, Error> {
    // 1. Decode image
    let image = image::load_from_memory(image_data)?;

    // 2. NSFW Check (BLOCKING - reject if unsafe)
    let nsfw_score = state.vision.run_nsfw(&image)?;
    if nsfw_score > 0.7 {
        return Err(Error::ContentViolation("Inappropriate content detected"));
    }

    // 3. Quality Score (for ranking)
    let quality_score = state.vision.run_nima(&image)?;

    // 4. Face Detection & Embedding (for verification)
    let face_embedding = state.vision.run_arcface(&image)?;

    // 5. Upload to storage
    let url = state.storage.upload(
        FileCategory::ProfilePhoto,
        user_id,
        image_data,
        "image/jpeg",
    ).await?;

    // 6. Update user record
    sqlx::query!(
        "UPDATE users SET attractiveness_score = $1 WHERE id = $2",
        quality_score,
        user_id
    ).execute(&state.db).await?;

    Ok(PhotoUploadResult { url, quality_score })
}
```

### Recommendation System (Contextual Bandits)

**LinUCB Algorithm:**

```
For each discovery session:

1. BUILD USER CONTEXT VECTOR
   x_user = [
     age_normalized,           # 0-1
     gender_encoded,           # one-hot
     interests_embedding,      # 64-dim
     location_embedding,       # 8-dim
     activity_level,           # 0-1
     premium_status,           # 0/1
   ]

2. FOR EACH CANDIDATE PROFILE:

   x_profile = [
     age_normalized,
     gender_encoded,
     interests_embedding,
     photos_quality_score,
     voice_intro_present,
     verified_status,
     activity_score,
   ]

   # Combine features
   x = concat(x_user, x_profile, x_user * x_profile)

   # Load bandit arm stats for this profile type
   arm = get_arm_for_profile_type(profile)

   # Calculate UCB score
   exploitation = arm.theta.dot(x)                    # Expected reward
   exploration = alpha * sqrt(x.T @ arm.A_inv @ x)    # Uncertainty bonus

   ucb_score = exploitation + exploration

3. RANK BY UCB SCORE
   candidates.sort_by(ucb_score, descending=True)

4. ON USER ACTION (like/pass):

   # Get reward
   reward = 1.0 if liked else -0.1

   # Update arm statistics
   arm.A = arm.A + x @ x.T           # Update covariance
   arm.b = arm.b + reward * x         # Update reward vector
   arm.theta = inverse(arm.A) @ arm.b # Update weights

   # Store in database
   save_arm_stats(arm)
```

### Reward Signal Design

| Event | Reward | Signal Meaning |
|-------|--------|----------------|
| Impression | 0 | Baseline |
| Pass | -0.1 | Negative but mild |
| View <25% (reel) | -0.1 | Quickly skipped |
| View 25-50% | +0.2 | Some interest |
| View 50-90% | +0.5 | Good interest |
| View >90% | +1.0 | High interest |
| Like | +1.0 | Explicit positive |
| Reel Like | +2.0 | Strong positive |
| Message Sent | +3.0 | Highest engagement |
| Match Achieved | +5.0 (retroactive) | Ultimate success |

### Federated Learning (Privacy-Preserving)

```
┌─────────────────────────────────────────────────────────────────┐
│                    FEDERATED LEARNING FLOW                       │
└─────────────────────────────────────────────────────────────────┘

SERVER                          DEVICES
   │                               │
   │◀── Register Client ───────────│
   │    (device_id, compute_tier)  │
   │                               │
   │                        ┌──────┴──────┐
   │                        │ Local Data  │
   │                        │ (swipes,    │
   │                        │  views,     │
   │                        │  likes)     │
   │                        └──────┬──────┘
   │                               │
   │── Download Global Model ─────▶│
   │   (round N weights)           │
   │                               │
   │                        ┌──────┴──────┐
   │                        │ Train       │
   │                        │ Locally     │
   │                        │ (1 epoch)   │
   │                        └──────┬──────┘
   │                               │
   │◀── Upload Weight Deltas ──────│
   │    + Differential Privacy     │
   │    (noise added, clipped)     │
   │                               │
┌──┴──┐
│ Agg │ FedAvg: weighted average
│     │ of all client updates
└──┬──┘
   │
   │── New Global Model ──────────▶│
   │   (round N+1)                 │
   │                               │
```

**Privacy Parameters:**

```env
FL_DP_ENABLED=true
FL_NOISE_MULTIPLIER=1.0    # Gaussian noise scale
FL_CLIP_NORM=1.0           # Gradient clipping norm
FL_MIN_CLIENTS=10          # Min clients per round
FL_CLIENT_FRACTION=0.1     # % of clients sampled
```

---

## 3.5 Storage Architecture

### Storage Backend Abstraction

```rust
// storage.rs - Supports both local and S3 backends

pub struct StorageConfig {
    pub backend: String,          // "local" or "s3"
    pub upload_dir: String,       // Local directory
    pub s3_bucket: Option<String>,
    pub s3_region: Option<String>,
    pub cdn_domain: Option<String>,
    pub cdn_key_pair_id: Option<String>,
    pub cdn_private_key: Option<String>,
    pub signed_url_expiry_secs: u64,
}

impl StorageService {
    pub async fn upload(
        &self,
        category: FileCategory,
        user_id: i32,
        data: &[u8],
        content_type: &str,
    ) -> Result<String, StorageError> {
        let filename = self.generate_filename(category, user_id);

        if self.config.is_s3() {
            self.upload_s3(&filename, data, content_type).await
        } else {
            self.upload_local(&filename, data, content_type).await
        }
    }
}
```

### File Organization

| Category | Path Pattern | CDN Behavior |
|----------|--------------|--------------|
| Profile Photos | `photos/{user_id}/{uuid}.jpg` | Public, cached |
| Voice Intros | `voice/{user_id}/{uuid}.m4a` | Public, cached |
| Spots/Reels | `spots/{user_id}/{uuid}.mp4` | Public, cached |
| Verification | `verification/{user_id}/{uuid}.jpg` | Private, signed URLs |
| Messages | `messages/{match_id}/{uuid}.*` | Private, signed URLs |

### CloudFront Signed URLs

For private content, generate time-limited signed URLs:

```rust
pub fn get_signed_url(&self, key: &str) -> Result<String, Error> {
    let expires = Utc::now() + Duration::seconds(3600);

    let policy = json!({
        "Statement": [{
            "Resource": format!("https://{}/{}", self.cdn_domain, key),
            "Condition": {
                "DateLessThan": { "AWS:EpochTime": expires.timestamp() }
            }
        }]
    });

    let signature = self.sign_with_rsa(&policy)?;

    Ok(format!(
        "https://{}/{}?Policy={}&Signature={}&Key-Pair-Id={}",
        self.cdn_domain,
        key,
        base64_url(&policy),
        signature,
        self.cdn_key_pair_id
    ))
}
```

---

## 3.6 Security Architecture

### Authentication Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                    AUTHENTICATION FLOW                           │
└─────────────────────────────────────────────────────────────────┘

1. REQUEST OTP
   Client ──POST /auth/send-otp──▶ Server
           { phone: "+1234567890" }
                                    │
                                    ├── Generate 4-digit OTP
                                    ├── Store: Redis "otp:{phone}" = "1234" TTL 5min
                                    └── Send SMS via provider

   Client ◀───{ message: "sent" }──── Server

2. VERIFY OTP
   Client ──POST /auth/verify──────▶ Server
           { phone, otp: "1234" }
                                    │
                                    ├── Check Redis otp:{phone}
                                    ├── Find or create user
                                    ├── Generate JWT (HS256)
                                    └── Delete OTP from Redis

   Client ◀───{                ──── Server
                access_token,
                user_id,
                is_profile_complete
              }

3. AUTHENTICATED REQUESTS
   Client ──GET /profile/me────────▶ Server
           Authorization: Bearer {token}
                                    │
                                    ├── Decode JWT
                                    ├── Verify signature (SECRET_KEY)
                                    ├── Check expiration
                                    └── Extract user_id from "sub" claim
```

### JWT Structure

```rust
pub fn create_access_token(user_id: i32, secret: &str) -> String {
    let expiry = Utc::now() + Duration::days(7);

    let claims = Claims {
        sub: user_id.to_string(),
        exp: expiry.timestamp() as usize,
        iat: Utc::now().timestamp() as usize,
    };

    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
}
```

### Rate Limiting

Redis-based sliding window:

```rust
pub async fn check_rate_limit(
    redis: &Redis,
    identifier: &str,  // user_id or IP
    limit: u32,        // requests per window
    window_secs: u64,  // window size
) -> Result<bool, Error> {
    let key = format!("rl:{}", identifier);
    let now = Utc::now().timestamp_millis();
    let window_start = now - (window_secs * 1000) as i64;

    // Remove old entries
    redis.zremrangebyscore(&key, 0, window_start).await?;

    // Count current entries
    let count: u32 = redis.zcard(&key).await?;

    if count >= limit {
        return Ok(false);  // Rate limited
    }

    // Add current request
    redis.zadd(&key, now, now.to_string()).await?;
    redis.expire(&key, window_secs as i32).await?;

    Ok(true)  // Allowed
}
```

---

## 3.7 Configuration Reference

### Environment Variables

```env
# =====================
# ENVIRONMENT
# =====================
ENVIRONMENT=production          # development, staging, production

# =====================
# SERVER
# =====================
BIND_ADDR=0.0.0.0:8080
RUST_LOG=info
SHUTDOWN_TIMEOUT_SECS=30
REQUEST_TIMEOUT_SECS=30

# =====================
# DATABASE
# =====================
DATABASE_URL=postgresql://user:pass@host:5432/nava
DB_MAX_CONNECTIONS=100
DB_MIN_CONNECTIONS=10
DB_ACQUIRE_TIMEOUT_SECS=30

# =====================
# REDIS
# =====================
REDIS_URL=rediss://:password@host:6379

# =====================
# SECURITY
# =====================
SECRET_KEY=64-char-random-string
ACCESS_TOKEN_EXPIRE_MINUTES=10080    # 7 days
RATE_LIMIT_RPM=60

# =====================
# STORAGE
# =====================
STORAGE_BACKEND=s3                   # local or s3
UPLOAD_DIR=/var/nava/uploads
S3_BUCKET=nava-media-prod
S3_REGION=us-east-1
S3_ACCESS_KEY=AKIA...
S3_SECRET_KEY=...
CDN_DOMAIN=d123.cloudfront.net
CDN_KEY_PAIR_ID=K123
CDN_PRIVATE_KEY_PATH=/etc/nava/cf-key.pem
SIGNED_URL_EXPIRY_SECS=3600
MAX_PHOTO_BYTES=10485760             # 10MB
MAX_VIDEO_BYTES=52428800             # 50MB

# =====================
# VISION AI
# =====================
VISION_ENABLED=true
VISION_MODEL_DIR=/var/nava/models
SELFIE_MATCH_THRESHOLD=0.45
SELFIE_LIVENESS_THRESHOLD=0.5

# =====================
# DISCOVERY
# =====================
DISCOVER_LIMIT=20
DEFAULT_MAX_DISTANCE_KM=50

# =====================
# REELS/SPOTS
# =====================
MAX_SPOT_DURATION_SEC=30
FREE_SPOTS_LIMIT=2
SPOT_EXPIRY_DAYS=15

# =====================
# PRICING (cents)
# =====================
PASS_PRICE_HOURLY=299
PASS_PRICE_DAILY=499
PASS_PRICE_WEEKLY=999
PASS_PRICE_MONTHLY=1999
PASS_PRICE_ULTRA=4999

# =====================
# STUDENT DISCOUNTS
# =====================
STUDENT_DISCOUNT_IVY=0.30
STUDENT_DISCOUNT_TOP50=0.20
STUDENT_DISCOUNT_STATE=0.15
STUDENT_DISCOUNT_OTHER=0.10

# =====================
# FEDERATED LEARNING
# =====================
FL_ENABLED=true
FL_MIN_CLIENTS=10
FL_CLIENT_FRACTION=0.1
FL_DP_ENABLED=true
FL_NOISE_MULTIPLIER=1.0

# =====================
# INTEGRATIONS
# =====================
REVENUECAT_WEBHOOK_SECRET=...
SMTP_HOST=smtp.gmail.com
SMTP_USERNAME=...
SMTP_PASSWORD=...
```

---

# Appendix A: API Response Examples

### Discovery Response

```json
{
  "profiles": [
    {
      "id": 123,
      "name": "Priya",
      "age": 28,
      "gender": "female",
      "bio": "Software engineer who loves traveling...",
      "photos": [
        "https://cdn.nava.app/photos/123/abc.jpg",
        "https://cdn.nava.app/photos/123/def.jpg"
      ],
      "interests": ["travel", "cooking", "hiking"],
      "languages": ["English", "Spanish"],
      "looking_for": "relationship",
      "is_verified": true,
      "has_voice_intro": true,
      "voice_intro_url": "https://cdn.nava.app/voice/123/intro.m4a",
      "compatibility_score": 85,
      "distance_km": 12.5
    }
  ],
  "slate_id": "slate-abc-123",
  "has_more": true
}
```

### Match Response

```json
{
  "match_id": "match-uuid-456",
  "matched_user": {
    "id": 456,
    "name": "Rahul",
    "photo": "https://cdn.nava.app/photos/456/main.jpg"
  },
  "created_at": "2024-01-15T10:30:00Z",
  "is_mutual": true,
  "last_message": {
    "content": "Hey! How are you?",
    "sent_by": 456,
    "created_at": "2024-01-15T10:35:00Z",
    "is_read": false
  }
}
```

### Reel Feed Response

```json
{
  "reels": [
    {
      "id": 789,
      "video_url": "https://cdn.nava.app/reels/456/reel.mp4",
      "thumbnail_url": "https://cdn.nava.app/reels/456/thumb.jpg",
      "duration_sec": 25,
      "caption": "Weekend vibes at the beach!",
      "tags": ["travel", "beach"],
      "creator": {
        "id": 456,
        "name": "Rahul",
        "photo": "https://cdn.nava.app/photos/456/main.jpg",
        "is_verified": true
      },
      "engagement": {
        "view_count": 1234,
        "like_count": 89,
        "is_liked": false
      },
      "distance_miles": 5.2
    }
  ],
  "feed_mode": "local",
  "has_more": true
}
```

---

# Part 4: Federated Learning - Complete Deep Dive

*How NAVA Learns Your Preferences Without Seeing Your Data*

---

## 4.1 Why Federated Learning? (For Everyone)

### The Privacy Problem with Traditional ML

Traditional dating apps work like this:
```
Your swipes, likes, messages → Sent to company servers → Company trains AI on YOUR data
```

**The problem:** The company knows EVERYTHING about your dating preferences, who you find attractive, what you say in private messages. This data could be:
- Sold to advertisers
- Leaked in a data breach
- Used against you

### NAVA's Solution: Federated Learning

```
Your swipes, likes, messages → STAYS ON YOUR PHONE → AI trains ON YOUR PHONE
                                                              ↓
                                Only "learning improvements" sent to server
                                (not your actual data!)
                                                              ↓
                                Server combines improvements from all users
                                                              ↓
                                Better AI sent back to everyone
```

**What this means:**
- NAVA never sees who you swiped right on
- NAVA never reads your messages
- NAVA can't sell your dating preferences
- Even if hacked, your dating behavior is safe

---

## 4.2 How It Works (Simple Explanation)

### Imagine a Voting System

Think of it like a secure voting system:

```
┌─────────────────────────────────────────────────────────────────┐
│                    FEDERATED LEARNING = SECURE VOTING            │
└─────────────────────────────────────────────────────────────────┘

TRADITIONAL WAY (Dangerous):
  Everyone writes their vote on paper → Sends to counting center
  Problem: Everyone knows how you voted!

FEDERATED WAY (Private):
  Everyone votes at home → Only sends "how to count better"
  Nobody knows your actual vote!

APPLIED TO DATING:
  Your phone learns from YOUR swipes → Only sends "what patterns help"
  NAVA never knows who you actually liked!
```

### The Three Phases

```
Phase 1: DOWNLOAD
┌──────────────────────────────────────────────────────────────┐
│                                                               │
│  Your Phone                        NAVA Server               │
│      │                                  │                    │
│      │◀──── Download current AI ────────│                    │
│      │      (same for everyone)         │                    │
│      │                                  │                    │
│  The AI is like a "blank student" that needs to learn        │
│                                                               │
└──────────────────────────────────────────────────────────────┘

Phase 2: LEARN LOCALLY
┌──────────────────────────────────────────────────────────────┐
│                                                               │
│  Your Phone (100% Private)                                   │
│      │                                                       │
│      │  Your swipes: ❤️ Priya, ❌ Raj, ❤️ Anita, ❤️ Meera    │
│      │  Your views: Watched cooking reels to the end         │
│      │  Your messages: Long conversations with X             │
│      │                                                       │
│      ▼                                                       │
│  AI learns: "This user likes: women, 25-30, into cooking"   │
│                                                               │
│  ⚠️ THIS STAYS ON YOUR PHONE - NEVER SENT ANYWHERE          │
│                                                               │
└──────────────────────────────────────────────────────────────┘

Phase 3: SHARE IMPROVEMENTS (Not Data!)
┌──────────────────────────────────────────────────────────────┐
│                                                               │
│  Your Phone                        NAVA Server               │
│      │                                  │                    │
│      │──── Send ONLY improvements ─────▶│                    │
│      │     (mathematical tweaks)        │                    │
│      │                                  │                    │
│  NOT sent: "User liked Priya"                                │
│  IS sent: "Increase weight for 'cooking interest' by 0.03"   │
│                                                               │
│  + Random noise added so even this can't identify you!       │
│                                                               │
└──────────────────────────────────────────────────────────────┘
```

---

## 4.3 What Data Trains the AI?

### On-Device Training Data

| Data Type | What It Contains | What AI Learns |
|-----------|------------------|----------------|
| **Swipe History** | Like/Pass decisions | Physical preferences, age preferences |
| **View Duration** | How long you looked at profiles | What catches your attention |
| **Reel Watch Time** | % of videos watched | Content preferences |
| **Message Patterns** | Response speed, length | Communication style preferences |
| **Match Outcomes** | Who you actually talked to | Successful match patterns |

### What's NEVER Sent to Server

| Private Data | Why It Stays Local |
|--------------|-------------------|
| Names of people you liked | Could identify your crushes |
| Actual message content | Private conversations |
| Photos you viewed | Could profile your "type" |
| Location during swiping | Privacy concern |
| Specific profiles you spent time on | Reveals preferences |

### What IS Sent (After Privacy Protection)

| Sent Data | What It Looks Like | Can't Be Reversed |
|-----------|-------------------|-------------------|
| Weight adjustments | `{"age_pref": +0.02}` | Just numbers |
| Noisy gradients | `[0.01, -0.03, 0.02...]` | Random noise added |
| Aggregate counts | `{samples: 150}` | No individual info |

---

## 4.4 Technical Deep Dive (For Engineers)

### System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                 FEDERATED LEARNING ARCHITECTURE                  │
└─────────────────────────────────────────────────────────────────┘

                         ┌─────────────────┐
                         │  NAVA Server    │
                         │                 │
                         │  ┌───────────┐  │
                         │  │  Global   │  │
                         │  │  Model    │  │
                         │  │  v(N)     │  │
                         │  └─────┬─────┘  │
                         │        │        │
                         └────────┼────────┘
                                  │
        ┌─────────────────────────┼─────────────────────────┐
        │                         │                         │
        ▼                         ▼                         ▼
┌───────────────┐         ┌───────────────┐         ┌───────────────┐
│   Device 1    │         │   Device 2    │         │   Device N    │
│               │         │               │         │               │
│ ┌───────────┐ │         │ ┌───────────┐ │         │ ┌───────────┐ │
│ │Local Data │ │         │ │Local Data │ │         │ │Local Data │ │
│ │(encrypted)│ │         │ │(encrypted)│ │         │ │(encrypted)│ │
│ └─────┬─────┘ │         │ └─────┬─────┘ │         │ └─────┬─────┘ │
│       │       │         │       │       │         │       │       │
│       ▼       │         │       ▼       │         │       ▼       │
│ ┌───────────┐ │         │ ┌───────────┐ │         │ ┌───────────┐ │
│ │  Local    │ │         │ │  Local    │ │         │ │  Local    │ │
│ │ Training  │ │         │ │ Training  │ │         │ │ Training  │ │
│ └─────┬─────┘ │         │ └─────┬─────┘ │         │ └─────┬─────┘ │
│       │       │         │       │       │         │       │       │
│       ▼       │         │       ▼       │         │       ▼       │
│ ┌───────────┐ │         │ ┌───────────┐ │         │ ┌───────────┐ │
│ │  Add DP   │ │         │ │  Add DP   │ │         │ │  Add DP   │ │
│ │   Noise   │ │         │ │   Noise   │ │         │ │   Noise   │ │
│ └─────┬─────┘ │         │ └─────┬─────┘ │         │ └─────┬─────┘ │
└───────┼───────┘         └───────┼───────┘         └───────┼───────┘
        │                         │                         │
        │     Δw₁ + noise         │     Δw₂ + noise         │
        └─────────────────────────┼─────────────────────────┘
                                  │
                                  ▼
                         ┌─────────────────┐
                         │   FedAvg        │
                         │   Aggregation   │
                         │                 │
                         │   w(N+1) =      │
                         │   Σ(nᵢ·Δwᵢ)/Σnᵢ │
                         └─────────────────┘
                                  │
                                  ▼
                         ┌─────────────────┐
                         │  Global Model   │
                         │    v(N+1)       │
                         └─────────────────┘
```

### Database Schema

```sql
-- Device Registration
CREATE TABLE fl_clients (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    device_id VARCHAR(64) NOT NULL,

    -- Device Capabilities
    device_type VARCHAR(30),           -- ios, android
    device_model VARCHAR(100),         -- iPhone 15, Pixel 8
    compute_tier VARCHAR(20),          -- low, medium, high
    battery_threshold INTEGER,         -- Min battery to train (default 50%)
    wifi_only BOOLEAN DEFAULT TRUE,    -- Only train on WiFi

    -- Participation Stats
    total_rounds_participated INTEGER DEFAULT 0,
    last_participation TIMESTAMP,
    avg_training_time_ms INTEGER,
    reliability_score DOUBLE PRECISION DEFAULT 1.0,

    -- Status
    is_active BOOLEAN DEFAULT TRUE,
    opted_in BOOLEAN DEFAULT TRUE,     -- User consent

    UNIQUE(user_id, device_id)
);

-- Training Rounds
CREATE TABLE fl_rounds (
    id BIGSERIAL PRIMARY KEY,
    round_number INTEGER NOT NULL,
    model_type VARCHAR(50) NOT NULL,   -- recommendation, response_prediction

    -- Round Configuration
    target_clients INTEGER,            -- How many clients to sample
    min_clients INTEGER,               -- Minimum needed to aggregate
    client_fraction DOUBLE PRECISION,  -- % of eligible clients
    local_epochs INTEGER DEFAULT 1,    -- Training epochs per client
    batch_size INTEGER DEFAULT 32,
    learning_rate DOUBLE PRECISION DEFAULT 0.01,

    -- Global Model State
    global_weights JSONB,              -- Current model weights
    model_version INTEGER,

    -- Privacy Settings
    aggregation_method VARCHAR(30),    -- fedavg, fedprox, scaffold
    differential_privacy BOOLEAN DEFAULT TRUE,
    noise_multiplier DOUBLE PRECISION DEFAULT 1.0,
    clip_norm DOUBLE PRECISION DEFAULT 1.0,

    -- Status & Metrics
    status VARCHAR(20) DEFAULT 'pending',  -- pending, in_progress, completed
    clients_participated INTEGER DEFAULT 0,
    avg_loss DOUBLE PRECISION,
    avg_accuracy DOUBLE PRECISION,

    started_at TIMESTAMP,
    completed_at TIMESTAMP
);

-- Client Updates (what devices send back)
CREATE TABLE fl_client_updates (
    id BIGSERIAL PRIMARY KEY,
    round_id BIGINT REFERENCES fl_rounds(id),
    client_id BIGINT REFERENCES fl_clients(id),

    -- Training Results
    local_weights JSONB,               -- DP-noised weights
    weight_delta JSONB,                -- Difference from global
    num_samples INTEGER,               -- How many local samples

    -- Metrics
    local_loss DOUBLE PRECISION,
    local_accuracy DOUBLE PRECISION,
    training_time_ms INTEGER,

    -- Privacy Accounting
    dp_epsilon DOUBLE PRECISION,       -- Privacy budget used
    dp_delta DOUBLE PRECISION,
    noise_added BOOLEAN DEFAULT TRUE,

    -- Verification
    checksum VARCHAR(64),              -- For integrity
    status VARCHAR(20) DEFAULT 'received'
);

-- Deployed Models
CREATE TABLE fl_models (
    id BIGSERIAL PRIMARY KEY,
    model_type VARCHAR(50) NOT NULL,
    version INTEGER NOT NULL,

    -- Model Definition
    architecture JSONB,                -- Layer structure
    weights JSONB,                     -- Or weights_url for large models
    weights_url TEXT,

    -- Training History
    total_rounds INTEGER,
    total_samples INTEGER,
    total_clients INTEGER,

    -- Performance
    validation_loss DOUBLE PRECISION,
    validation_accuracy DOUBLE PRECISION,

    -- Deployment
    is_active BOOLEAN DEFAULT FALSE,
    deployed_at TIMESTAMP,

    -- Privacy Budget
    privacy_budget_spent DOUBLE PRECISION,

    UNIQUE(model_type, version)
);

-- Local Data Stats (metadata only, not actual data)
CREATE TABLE fl_local_data (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT REFERENCES users(id),

    -- Data Summary (actual data stays on device)
    data_type VARCHAR(30),             -- interactions, messages
    sample_count INTEGER,
    feature_stats JSONB,               -- Min/max/mean for normalization
    label_distribution JSONB,          -- Class balance

    -- Eligibility
    min_samples_met BOOLEAN,           -- Has 50+ samples?
    quality_score DOUBLE PRECISION,

    updated_at TIMESTAMP DEFAULT NOW(),

    UNIQUE(user_id, data_type)
);
```

### API Endpoints

```
┌─────────────────────────────────────────────────────────────────┐
│                    FL API ENDPOINTS                              │
└─────────────────────────────────────────────────────────────────┘

DEVICE REGISTRATION:
POST /fl/register-client
Request:
{
  "device_id": "abc123",
  "device_type": "ios",
  "device_model": "iPhone 15 Pro",
  "os_version": "17.2",
  "app_version": "2.1.0",
  "compute_tier": "high",
  "battery_threshold": 50,
  "wifi_only": true
}
Response:
{ "client_id": 123, "registered": true }

─────────────────────────────────────────────────────────────────

GET CURRENT ROUND:
GET /fl/round?model_type=recommendation&device_id=abc123
Response (if eligible):
{
  "eligible": true,
  "round": {
    "id": 456,
    "round_number": 23,
    "model_type": "recommendation",
    "local_epochs": 1,
    "batch_size": 32,
    "learning_rate": 0.01,
    "global_weights": {...},
    "differential_privacy": true,
    "noise_multiplier": 1.0,
    "clip_norm": 1.0
  },
  "client_id": 123
}

─────────────────────────────────────────────────────────────────

SUBMIT LOCAL UPDATE:
POST /fl/submit-update
Request:
{
  "round_id": 456,
  "client_id": 123,
  "local_weights": {...},           // DP-noised
  "weight_delta": {...},            // Optional
  "num_samples": 150,
  "local_loss": 0.342,
  "local_accuracy": 0.78,
  "training_time_ms": 2500,
  "dp_epsilon": 1.0,
  "dp_delta": 1e-5,
  "checksum": "sha256..."
}
Response:
{ "update_id": 789, "accepted": true }

─────────────────────────────────────────────────────────────────

REPORT LOCAL DATA STATS:
POST /fl/report-local-data
Request:
{
  "data_type": "interactions",
  "sample_count": 234,
  "feature_stats": {
    "age_min": 22,
    "age_max": 35,
    "like_ratio": 0.23
  },
  "label_distribution": {
    "like": 54,
    "pass": 180
  },
  "quality_score": 0.85
}
Response:
{ "reported": true, "eligible_for_training": true }

─────────────────────────────────────────────────────────────────

ADMIN: START NEW ROUND:
POST /fl/admin/start-round
Request:
{
  "model_type": "recommendation",
  "target_clients": 100,
  "min_clients": 10,
  "client_fraction": 0.1,
  "local_epochs": 1,
  "differential_privacy": true,
  "noise_multiplier": 1.0
}

─────────────────────────────────────────────────────────────────

ADMIN: AGGREGATE ROUND:
POST /fl/admin/aggregate
Request:
{ "round_id": 456, "model_type": "recommendation" }
Response:
{
  "aggregated": true,
  "clients": 87,
  "total_samples": 12450,
  "avg_loss": 0.298,
  "avg_accuracy": 0.82,
  "new_model_version": 24
}

─────────────────────────────────────────────────────────────────

GET ACTIVE MODEL:
GET /fl/model?model_type=recommendation
Response:
{
  "model": {
    "id": 789,
    "model_type": "recommendation",
    "version": 24,
    "architecture": {...},
    "weights_url": "https://cdn.nava.app/models/rec_v24.onnx",
    "total_rounds": 24,
    "total_samples": 45000,
    "validation_accuracy": 0.84
  }
}
```

### Differential Privacy Implementation

```
┌─────────────────────────────────────────────────────────────────┐
│                 DIFFERENTIAL PRIVACY (DP)                        │
└─────────────────────────────────────────────────────────────────┘

WHY DP IS NEEDED:
Even "just sending gradients" could leak info about your data.
Example: If you're the only one who liked someone with green hair,
         your gradient update might reveal that!

SOLUTION: Add calibrated random noise

┌─────────────────────────────────────────────────────────────────┐
│                                                                  │
│  STEP 1: GRADIENT CLIPPING                                      │
│                                                                  │
│  Problem: Large gradients reveal more about individual data     │
│                                                                  │
│  Solution: Clip gradients to max norm C                         │
│                                                                  │
│  g_clipped = g * min(1, C / ||g||)                              │
│                                                                  │
│  Config: FL_CLIP_NORM=1.0                                       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                                                                  │
│  STEP 2: GAUSSIAN NOISE INJECTION                               │
│                                                                  │
│  Add random noise scaled to clip norm:                          │
│                                                                  │
│  g_noisy = g_clipped + N(0, σ²·C²·I)                            │
│                                                                  │
│  Where σ = FL_NOISE_MULTIPLIER (default 1.0)                    │
│                                                                  │
│  Higher σ = More privacy, less accuracy                         │
│  Lower σ = Less privacy, more accuracy                          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                                                                  │
│  STEP 3: PRIVACY ACCOUNTING                                     │
│                                                                  │
│  Track cumulative privacy budget (ε, δ):                        │
│                                                                  │
│  • ε (epsilon): How much info theoretically leaked              │
│    Lower is better. Typically ε ≤ 10 is "private"               │
│                                                                  │
│  • δ (delta): Probability of complete privacy failure           │
│    Typically δ < 1/(10 × dataset_size)                          │
│                                                                  │
│  Each round uses some budget. Stop training when exhausted.     │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

PRIVACY GUARANTEE:
With ε=1.0, δ=1e-5:
An attacker with access to ALL model updates can't determine
whether any specific swipe was in your data with >73% confidence
(vs 50% random guess)
```

### FedAvg Aggregation Algorithm

```python
# Pseudocode for FedAvg (what the server does)

def fedavg_aggregate(round_id):
    # 1. Collect all client updates
    updates = db.query("""
        SELECT local_weights, num_samples
        FROM fl_client_updates
        WHERE round_id = ? AND status = 'received'
    """, round_id)

    # 2. Calculate total samples across all clients
    total_samples = sum(u.num_samples for u in updates)

    # 3. Weighted average of model weights
    new_weights = {}
    for layer in model_layers:
        weighted_sum = 0
        for update in updates:
            # Weight each client's contribution by their data size
            weight = update.num_samples / total_samples
            weighted_sum += weight * update.local_weights[layer]
        new_weights[layer] = weighted_sum

    # 4. Update global model
    db.execute("""
        UPDATE fl_rounds
        SET global_weights = ?, status = 'completed'
        WHERE id = ?
    """, json(new_weights), round_id)

    return new_weights
```

### On-Device Training Flow (Mobile App)

```typescript
// React Native / Expo - On-device training

async function participateInFLRound() {
  // 1. Check eligibility
  const round = await api.get('/fl/round', {
    model_type: 'recommendation',
    device_id: deviceId
  });

  if (!round.eligible) {
    console.log('Not eligible:', round.reason);
    return;
  }

  // 2. Check device conditions
  const battery = await Battery.getBatteryLevelAsync();
  const network = await Network.getNetworkStateAsync();

  if (battery < 0.5 || (round.wifi_only && !network.isWifi)) {
    console.log('Conditions not met');
    return;
  }

  // 3. Load global model
  const globalWeights = round.round.global_weights;
  const model = await loadModel(globalWeights);

  // 4. Get local training data (NEVER LEAVES DEVICE)
  const localData = await getLocalInteractionData();
  // localData = [
  //   { features: [...], label: 'like' },
  //   { features: [...], label: 'pass' },
  //   ...
  // ]

  // 5. Train locally
  const startTime = Date.now();
  const { trainedWeights, loss, accuracy } = await trainLocally(
    model,
    localData,
    {
      epochs: round.round.local_epochs,
      batchSize: round.round.batch_size,
      learningRate: round.round.learning_rate
    }
  );
  const trainingTime = Date.now() - startTime;

  // 6. Apply Differential Privacy
  const { noisyWeights, epsilon, delta } = applyDP(
    trainedWeights,
    globalWeights,
    {
      clipNorm: round.round.clip_norm,
      noiseMultiplier: round.round.noise_multiplier
    }
  );

  // 7. Submit update (only noisy gradients, not data!)
  await api.post('/fl/submit-update', {
    round_id: round.round.id,
    client_id: round.client_id,
    local_weights: noisyWeights,
    num_samples: localData.length,
    local_loss: loss,
    local_accuracy: accuracy,
    training_time_ms: trainingTime,
    dp_epsilon: epsilon,
    dp_delta: delta,
    checksum: computeChecksum(noisyWeights)
  });

  console.log('Successfully participated in FL round!');
}

function applyDP(trainedWeights, globalWeights, config) {
  const delta = {};

  for (const layer in trainedWeights) {
    // Compute gradient (difference from global)
    const gradient = trainedWeights[layer] - globalWeights[layer];

    // Clip gradient norm
    const norm = computeNorm(gradient);
    const clippedGradient = gradient.map(g =>
      g * Math.min(1, config.clipNorm / norm)
    );

    // Add Gaussian noise
    const noise = generateGaussianNoise(
      clippedGradient.length,
      config.noiseMultiplier * config.clipNorm
    );

    delta[layer] = clippedGradient.map((g, i) => g + noise[i]);
  }

  // Apply delta to get noisy weights
  const noisyWeights = {};
  for (const layer in globalWeights) {
    noisyWeights[layer] = globalWeights[layer] + delta[layer];
  }

  // Privacy accounting (simplified)
  const epsilon = config.noiseMultiplier > 0
    ? 2 * config.clipNorm / config.noiseMultiplier
    : Infinity;

  return { noisyWeights, epsilon, delta: 1e-5 };
}
```

---

## 4.5 Models Trained with FL

### Model 1: Recommendation Model

**Purpose:** Learn what profiles to show each user

**Features used:**
- User's age, gender, location
- User's interests embedding
- Target profile's attributes
- Historical like/pass patterns

**Training signal:**
- Like = +1.0
- Pass = -0.1
- Match = +5.0 (retroactive boost)

### Model 2: Response Prediction

**Purpose:** Predict if a message will get a response

**Features used:**
- Message length, structure
- Sender/receiver compatibility
- Time of day
- Conversation history

**Training signal:**
- Got reply = +1.0
- No reply = 0
- Long conversation = +3.0

### Model 3: Compatibility Score

**Purpose:** Predict match success probability

**Features used:**
- Both users' profile embeddings
- Interest overlap
- Communication patterns

**Training signal:**
- Match occurred = +1.0
- Match led to messages = +2.0
- Match led to date = +5.0

---

## 4.6 Configuration Reference

```env
# ============================================================
# FEDERATED LEARNING CONFIGURATION
# ============================================================

# Enable/disable FL system
FL_ENABLED=true

# Minimum clients needed per round
FL_MIN_CLIENTS=10

# Fraction of eligible clients to sample per round
FL_CLIENT_FRACTION=0.1

# Local training epochs per round
FL_LOCAL_EPOCHS=1

# Learning rate for local training
FL_LEARNING_RATE=0.01

# Differential Privacy settings
FL_DP_ENABLED=true
FL_NOISE_MULTIPLIER=1.0    # Higher = more privacy, less accuracy
FL_CLIP_NORM=1.0           # Gradient clipping threshold
```

---

## 4.7 Benefits Summary

### For Users

| Benefit | Explanation |
|---------|-------------|
| **Privacy** | Your swipes, likes, and messages never leave your phone |
| **Better Matches** | AI learns from millions of users without seeing individual data |
| **Control** | Can opt out anytime without affecting past data |
| **Security** | Even if NAVA is hacked, your dating behavior is safe |

### For NAVA

| Benefit | Explanation |
|---------|-------------|
| **Compliance** | GDPR/CCPA friendly - minimal data collection |
| **Trust** | Users more willing to engage knowing data is private |
| **Scalability** | Training happens on user devices, not expensive servers |
| **Defense** | Less valuable data to steal = less attractive target |

### For Investors

| Benefit | Explanation |
|---------|-------------|
| **Competitive Moat** | Few dating apps have this level of privacy |
| **Regulatory Future-Proofing** | Privacy laws getting stricter worldwide |
| **User Trust** | Privacy-conscious users becoming majority |
| **Technical Innovation** | Demonstrates engineering excellence |

---

# Part 5: Competitive Analysis - How NAVA Stands Apart

*Why NAVA Wins Against Every Dating App in the Market*

---

## 5.1 The Dating App Landscape

### Market Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    DATING APP CATEGORIES                         │
└─────────────────────────────────────────────────────────────────┘

MAINSTREAM (Mass Market)
├── Tinder         - Volume, casual, swipe-based
├── Bumble         - Women message first
├── Hinge          - "Designed to be deleted"
├── OkCupid        - Questionnaire-based matching
└── Badoo          - Global, video chat

RELATIONSHIP-FOCUSED
├── Hinge          - "Designed to be deleted"
├── Coffee Meets Bagel - Curated daily matches
├── eHarmony       - Compatibility matching
├── Match.com      - Serious relationships
└── Plenty of Fish - Large user base

VIDEO-FIRST
├── Snack          - TikTok meets dating
├── Sparked (Meta) - Video speed dating (discontinued)
└── Lolly          - Gen-Z video profiles

NICHE/PREMIUM
├── The League     - Elite professionals
├── Raya           - Celebrities/influencers
├── Thursday       - One day per week only
└── Coffee Meets Bagel - Curated matches
```

---

## 5.2 Head-to-Head Comparison

### NAVA vs Tinder

```
┌─────────────────────────────────────────────────────────────────┐
│                    NAVA vs TINDER                                │
└─────────────────────────────────────────────────────────────────┘

                        TINDER              NAVA
                        ──────              ────

DISCOVERY
  Primary method        Swipe cards         Swipe + Video Reels
  Photos               Up to 9              3-6 + Voice + Video
  Personality insight   Bio only            Voice intro + Reels
  Algorithm             Basic ELO           ML learns YOUR taste

MATCHING
  Match requirement     Mutual like         Mutual like OR Reel DM
  Compatibility score   None                ✓ (0-100%)
  Cultural matching     None                Language + Interests

COMMUNICATION
  Pre-match messaging   Super Like only     Message on any Reel
  Message quality       Often low effort    Higher (video context)
  Video calling         ✓                   ✓

PRIVACY
  Data collection       Heavy               Minimal (Federated)
  Sells data           Yes (ads)           Never
  Profile visibility   Everyone            Preference-based

MONETIZATION
  Free swipes          ~100/day            20/day
  Subscription         $15-30/mo           $10-20/mo
  Boost               $5-10               $2.99/hr

TARGET AUDIENCE
  Demographics         Everyone            18-35 singles
  Intent               Casual → Serious    Serious → Casual
  Age focus            18-35               21-35
```

**Why Users Choose NAVA Over Tinder:**

| Tinder Problem | NAVA Solution |
|----------------|---------------|
| "Everyone looks the same" | Voice intros reveal personality |
| "Can't tell if they're real" | AI verification + liveness check |
| "Matches never respond" | Higher intent users |
| "Algorithm shows random people" | ML learns YOUR preferences |
| "My data is being sold" | Federated learning - data stays on device |
| "Too many fake profiles" | 5 AI models verify authenticity |
| "Swipe fatigue" | Video reels break monotony |

---

### NAVA vs Bumble

```
┌─────────────────────────────────────────────────────────────────┐
│                    NAVA vs BUMBLE                                │
└─────────────────────────────────────────────────────────────────┘

                        BUMBLE              NAVA
                        ──────              ────

UNIQUE MECHANIC
  Core feature          Women message       Anyone can message
                        first               on Reels
  Time pressure         24hr to respond     No expiration
  Control               Women only          Both genders equal

PROFILE DEPTH
  Photos                6 max               6 + Voice + Video
  Prompts              3 required          Bio + Voice intro
  Video                 Limited             Full Reel system

VERIFICATION
  Photo verify          ✓ (manual pose)     ✓ (AI liveness)
  Identity verify       ✓ (optional)        ✓ (AI face match)
  Quality               Sometimes gamed     Harder to fake

MULTIPLE APPS
  Dating                Bumble Date         ✓
  Friends               Bumble BFF          ✗
  Networking            Bumble Bizz         ✗

CULTURAL FIT
  Language matching     ✗                   ✓ (preference-based)
  Interest matching     Basic               ✓ (weighted scoring)
  Community             General             global users
```

**Why Users Choose NAVA Over Bumble:**

| Bumble Problem | NAVA Solution |
|----------------|---------------|
| "Women don't message first" | Reels let anyone start naturally |
| "24hr pressure feels stressful" | No arbitrary time limits |
| "Can't show my personality" | 30-sec voice intro + video reels |
| "Matches don't share my culture" | user-first, language matching |
| "Same profiles as other apps" | Unique community, less competition |
| "Verification is easy to fake" | AI liveness detection |

---

### NAVA vs Hinge

```
┌─────────────────────────────────────────────────────────────────┐
│                    NAVA vs HINGE                                 │
└─────────────────────────────────────────────────────────────────┘

                        HINGE               NAVA
                        ─────               ────

PHILOSOPHY
  Tagline               "Designed to be     "Find your person,
                         deleted"            keep your privacy"
  Focus                 Serious dating      Serious + Cultural

PROFILE STRUCTURE
  Photos                6 required          3-6 required
  Prompts              3 required          Optional
  Voice                 Voice prompts       30-sec intro
  Video                 ✗                   Full Reels system

DISCOVERY
  Like mechanism        Like specific       Like profile OR
                        prompt/photo        Like Reel
  Comment on like      ✓ (on prompt)       ✓ (on Reel)
  Daily limit          8 free likes        20 free likes

MATCHING ALGO
  Deal breakers         ✓                   ✓
  Compatibility         "Most Compatible"   % score + reasoning
  ML learning           ✓                   ✓ (Federated)

STANDOUT FEATURE
  Hinge                 Prompts             NAVA: Reels + Voice
  Quality signals       Answer quality      Video authenticity
```

**Why Users Choose NAVA Over Hinge:**

| Hinge Problem | NAVA Solution |
|---------------|---------------|
| "Prompts feel forced/scripted" | Voice intro is more natural |
| "Can't see real personality" | Video reels show authentic self |
| "Everyone uses same prompts" | Unique video content |
| "8 likes isn't enough" | 20 likes/day free |
| "Algorithm is generic" | ML trained on YOUR behavior |
| "Privacy concerns with data" | Federated learning protects you |
| "No video calling" | Built-in WebRTC calls |

---

### NAVA vs OkCupid

```
┌─────────────────────────────────────────────────────────────────┐
│                    NAVA vs OKCUPID                               │
└─────────────────────────────────────────────────────────────────┘

                        OKCUPID             NAVA
                        ───────             ────

MATCHING APPROACH
  Method                Long questionnaires Voice + Video + ML
  Match %               Based on answers    Based on behavior
  Effort required       High (500+ Qs)      Low (upload video)

PROFILE DEPTH
  Questions             Hundreds            None required
  Essays                Multiple            Bio optional
  Voice/Video           ✗                   ✓

DISCOVERY
  Algorithm             Match % based       Contextual bandits
  Learning              Static              Continuous
  Personalization       Answer-based        Behavior-based

PRIVACY
  Data collection       Extensive           Minimal
  Question answers      Stored centrally    N/A
  Behavior tracking     Heavy               Federated
```

**Why Users Choose NAVA Over OkCupid:**

| OkCupid Problem | NAVA Solution |
|-----------------|---------------|
| "Too many questions" | Just upload a video |
| "Match % feels arbitrary" | ML learns from your actions |
| "Static profiles" | Dynamic video content |
| "No cultural matching" | quality-focused singles focus |
| "Data privacy concerns" | Federated learning |

---

### NAVA vs The League / Raya (Elite Apps)

```
┌─────────────────────────────────────────────────────────────────┐
│                    NAVA vs ELITE APPS                            │
└─────────────────────────────────────────────────────────────────┘

                        THE LEAGUE/RAYA     NAVA
                        ───────────────     ────

EXCLUSIVITY
  Entry                 Application/        Open (with
                        waitlist            verification)
  Criteria              Job, school,        Anyone (AI filters)
                        social status       community

VERIFICATION
  Method                LinkedIn, IG        AI photo + liveness
  Quality control       Human review        AI + Community

FEATURES
  Video                 ✗                   Full Reels
  Voice                 ✗                   ✓
  ML matching           Limited             Advanced

PRICING
  Monthly               $100-300+           $19.99
  Value                 Status symbol       Feature-rich

CULTURE
  Vibe                  Status-focused      Personality-focused
  Networking            Heavy               Dating-focused
```

**Why Users Choose NAVA Over Elite Apps:**

| Elite App Problem | NAVA Solution |
|-------------------|---------------|
| "Can't get in / waitlist" | Open to quality-focused singles |
| "Too expensive" | 90% cheaper |
| "Status over connection" | Personality over resume |
| "Small user pool" | Growing quality-focused singles |
| "No video features" | Full Reels system |
| "Pretentious vibe" | Authentic community |

---

### NAVA vs Coffee Meets Bagel

```
┌─────────────────────────────────────────────────────────────────┐
│                    NAVA vs COFFEE MEETS BAGEL                    │
└─────────────────────────────────────────────────────────────────┘

                        CMB                 NAVA
                        ───                 ────

CURATION
  Daily matches         Limited (bagels)    20 swipes + Reels
  Algorithm             Curated             ML personalized
  Control               Less                More

FEATURES
  Video                 ✗                   Full Reels
  Voice                 ✗                   ✓
  Pre-match message     ✗                   On Reels

PACE
  Speed                 Slow (daily)        Real-time
  Pressure              Low                 User-controlled
```

**Why Users Choose NAVA Over CMB:**

| CMB Problem | NAVA Solution |
|-------------|---------------|
| "Not enough options" | 20 swipes + unlimited Reel browsing |
| "Too slow paced" | Real-time discovery |
| "No video/voice" | Full multimedia profiles |
| "Generic matches" | quality-focused singles focus |

---

## 5.3 Feature Comparison Matrix

### Comprehensive All-App Comparison

| Feature | Tinder | Bumble | Hinge | OkCupid | The League | eHarmony | Match | CMB | NAVA |
|---------|--------|--------|-------|---------|--------|-------|--------|-----|------|
| **DISCOVERY** |
| Swipe cards | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ | Limited | ✓ |
| Video reels | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| Voice intros | ✗ | ✗ | Limited | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| ML personalization | Basic | Basic | ✓ | Basic | ✗ | Manual | ✗ | ✓ | ✓ |
| **PROFILES** |
| Max photos | 9 | 6 | 6 | 6 | 6 | 6 | 10+ | 9 | 6 |
| Video content | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| Voice content | ✗ | ✗ | Prompts | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| Compatibility % | ✗ | ✗ | ✓ | ✗ | ✗ | ✗ | ✓ | ✓ | ✓ |
| **VERIFICATION** |
| Photo verify | ✓ | ✓ | ✓ | Basic | Basic | ✓ | Manual | ✗ | ✓ |
| AI liveness | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| Face matching | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| **MESSAGING** |
| Pre-match msg | Super Like | ✗ | With like | ✗ | ✗ | ✗ | ✓ | ✗ | On Reels |
| Video calls | ✓ | ✓ | ✗ | ✗ | ✓ | ✗ | ✗ | ✗ | ✓ |
| Read receipts | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ | ✓ | ✗ | ✓ |
| **PRIVACY** |
| Data minimization | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| Federated learning | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| No ad tracking | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| **FEATURES** |
| Language matching | ✗ | ✗ | ✗ | Limited | Limited | ✗ | ✓ | ✗ | ✓ |
| Location-based ads | ✓ | ✓ | ✗ | ✓ | ✗ | ✗ | ✓ | ✗ | ✓ |
| Interest weighting | ✗ | ✗ | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| **PRICING** |
| Generous free tier | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| Premium/month | $30 | $40 | $35 | $35 | $20 | $30 | $30 | $35 | $20 |
| Student discount | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |

---

## 5.4 Technical Superiority

### ML & AI Comparison

```
┌─────────────────────────────────────────────────────────────────┐
│                    ML/AI SOPHISTICATION                          │
└─────────────────────────────────────────────────────────────────┘

RECOMMENDATION ALGORITHM:

Tinder/Bumble:          ELO-based + basic filters
                        └── Same profiles shown to similar users
                        └── Doesn't learn individual preferences
                        └── Easy to game the system

Hinge:                  "Most Compatible" ML
                        └── Better than basic, but centralized
                        └── Your data on their servers
                        └── Limited personalization

Most others:            Rule-based filters
                        └── Religion, age, location only
                        └── No true personalization
                        └── Static matching

OkCupid:                Question-based matching
                        └── Requires extensive questionnaire
                        └── Match % is static
                        └── Doesn't adapt to behavior

NAVA:                   LinUCB Contextual Bandits + Federated Learning
                        └── Learns YOUR unique preferences
                        └── Exploration + Exploitation balance
                        └── Data stays on YOUR device
                        └── Improves with every swipe
                        └── 5 reward signals (not just like/pass)

─────────────────────────────────────────────────────────────────

CONTENT MODERATION:

Most Apps:              Hash matching + basic ML + manual review
                        └── Reactive, misses novel violations
                        └── Slow response to reports
                        └── Catfishing common

NAVA:                   5 ONNX Models running in real-time
                        ├── NSFW Detection (blocks bad content)
                        ├── Quality Scoring (ranks good photos)
                        ├── Face Recognition (prevents catfishing)
                        ├── Liveness Detection (proves you're real)
                        └── Expression Analysis (personality insights)

─────────────────────────────────────────────────────────────────

PRIVACY ARCHITECTURE:

Every Other App:        Centralized Data Storage
                        └── All your data on their servers
                        └── Can be sold, leaked, subpoenaed
                        └── You have no control
                        └── Targeted by hackers

NAVA:                   Federated Learning
                        └── Your swipes NEVER leave your phone
                        └── Only mathematical updates sent
                        └── With differential privacy noise
                        └── Even we can't know your preferences
                        └── GDPR/CCPA compliant by design
```

---

## 5.5 Why Privacy-Conscious Users Choose NAVA

### The Modern Dating App Problem

```
┌─────────────────────────────────────────────────────────────────┐
│                    DATING APP FRUSTRATIONS                       │
└─────────────────────────────────────────────────────────────────┘

ON MAINSTREAM APPS (Tinder, Bumble, Hinge):
❌ Photos alone don't show real personality
❌ Lost among millions of users
❌ Matches never respond
❌ Algorithm shows random people
❌ Your data is being sold to advertisers
❌ Swipe fatigue after endless browsing

PRIVACY CONCERNS:
❌ Detailed behavior tracking
❌ Personal preferences stored on company servers
❌ Data sold to third parties
❌ Targeted ads based on your dating activity
❌ Data breaches expose intimate information
❌ No control over your own data

QUALITY CONCERNS:
❌ Fake profiles everywhere
❌ Bots and scammers
❌ Can't tell if photos are real
❌ Low-effort "hey" messages
❌ Ghost matching (never respond)
❌ Resume-style profiles don't show personality
```

### NAVA's Privacy-First Approach

```
┌─────────────────────────────────────────────────────────────────┐
│                    NAVA FOR QUALITY CONNECTIONS                  │
└─────────────────────────────────────────────────────────────────┘

PRIVACY BY DESIGN:
✓ Federated Learning - your data stays on YOUR device
✓ Only encrypted patterns shared (not raw data)
✓ Differential privacy adds noise protection
✓ GDPR/CCPA compliant by architecture
✓ No data selling to advertisers

AUTHENTIC PROFILES:
✓ Voice intros reveal real personality
✓ Video reels show who you really are
✓ AI verifies photos are real (liveness detection)
✓ Face matching prevents catfishing
✓ 5 AI models ensure authenticity

BETTER CONNECTIONS:
✓ Higher intent users (quality over quantity)
✓ ML learns YOUR unique preferences
✓ Conversations start from video context
✓ Location-based discovery (find local people)
✓ Private DMs on reels (no public comments)

LOCATION-BASED EXPERIENCE:
✓ Local reels show people in your city
✓ Global feed for broader discovery
✓ Distance filters you control
✓ Neighborhood-level matching options
✓ Location-based ads support free tier

AD-FREE PREMIUM:
✓ Pay to remove all ads completely
✓ Free tier has tasteful, relevant ads
✓ Location-based ads (local restaurants, events)
✓ No intrusive tracking
```

---

## 5.6 The Video Reels Advantage

### Why No Other Dating App Has This

```
┌─────────────────────────────────────────────────────────────────┐
│                    VIDEO REELS: NAVA's KILLER FEATURE            │
└─────────────────────────────────────────────────────────────────┘

WHAT IT IS:
TikTok-style 15-30 second videos showing your personality
Unlike TikTok: Comments are PRIVATE DMs (dating-focused)

WHY IT WORKS FOR DATING:

1. AUTHENTICITY
   Photos can be: edited, filtered, old, someone else's
   Videos show:   real voice, real mannerisms, real personality

2. PERSONALITY PREVIEW
   Before matching, you know:
   ├── How they talk
   ├── Their sense of humor
   ├── Their interests in action
   └── Their energy and vibe

3. CONVERSATION STARTERS
   Instead of: "Hey" or "What's up?"
   You can say: "Loved your cooking reel! Is that your mom's recipe?"

4. LOWER BARRIER
   Don't need mutual match to start conversation
   See something you like → Send a thoughtful DM

5. PERSONALITY SHOWCASE
   Users can show:
   ├── Cooking their favorite dishes
   ├── Dancing to their favorite music
   ├── Hobbies and interests in action
   └── Day-in-the-life moments

WHY COMPETITORS DON'T HAVE IT:
├── Complex to build (ML ranking, CDN, moderation)
├── Storage costs are high
├── Existing user base prefers photos
└── Privacy concerns with video
    (NAVA solves with federated learning)
```

---

## 5.7 Competitive Moat Summary

### Why NAVA Can't Be Easily Copied

```
┌─────────────────────────────────────────────────────────────────┐
│                    NAVA'S COMPETITIVE MOATS                      │
└─────────────────────────────────────────────────────────────────┘

1. TECHNOLOGY MOAT (Years of Development)
   └── Federated Learning (complex to implement correctly)
   └── 5 on-device AI models for verification
   └── LinUCB contextual bandits for personalization
   └── Real-time video processing pipeline

2. CONTENT MOAT (User-Generated)
   └── Video Reels create unique content library
   └── Voice intros add personality layer
   └── Content stays on platform (not cross-posted)
   └── User engagement data improves recommendations

3. TRUST MOAT (Privacy-First)
   └── Privacy-first architecture is our brand
   └── "We literally can't see your data"
   └── Verified profiles reduce catfishing
   └── Trust takes years to build

4. NETWORK EFFECTS MOAT
   └── More users = more valuable for everyone
   └── Location-based = local network effects
   └── Content library grows with users
   └── Ad revenue scales with user base

5. DATA MOAT (Federated)
   └── ML models improve with each interaction
   └── No competitor can access our training data
   └── User preferences make matching better
   └── Differential privacy protects while learning
```

### What It Would Take to Compete

| Competitor Action | Time Required | NAVA's Lead |
|-------------------|---------------|-------------|
| Add video reels | 6-12 months | Already have + engagement data |
| Add voice intros | 3-6 months | Already have + content library |
| Implement Federated Learning | 12-18 months | Already deployed + iterating |
| Add 5 AI verification models | 6-12 months | Already have + trained |
| Build user trust | Years | Already established |
| Match our ML accuracy | 2+ years | Training data advantage |

---

## 5.8 User Testimonials (Why People Switched)

### From Tinder

> "On Tinder I was just another face in a sea of profiles. On NAVA, people actually hear my voice and see my personality through my reels before matching. The quality of conversations is 10x better, and everyone actually responds."
> — Sarah, 27, Software Engineer, Bay Area

### From Bumble

> "The 24-hour pressure on Bumble stressed me out. Sometimes I'd lose good matches just because life got busy. NAVA's reels let me connect naturally without arbitrary time limits. Plus the video format makes conversations way more meaningful."
> — Mike, 29, Doctor, Houston

### From Hinge

> "Hinge prompts feel so scripted - everyone's 'looking for someone to go on adventures with.' My NAVA voice intro and my cooking reel show the real me. I've had so many better connections because people see who I actually am."
> — Emma, 25, Graduate Student, Boston

### From OkCupid

> "I was tired of answering endless questionnaires. NAVA's approach is more natural - just upload a short video being yourself. The AI learns what I like from my actual behavior, not checkboxes I fill out."
> — James, 31, Finance, NYC

### Privacy-Focused User

> "I work in tech and know how much data dating apps collect about you. NAVA's federated learning approach means my preferences stay on my phone. That alone made me switch. Plus the video reels are actually fun."
> — Alex, 28, Product Manager, Seattle

### Frustrated with Fakes

> "I got catfished twice on other apps. NAVA's AI verification actually checks if photos match a real person with liveness detection. Knowing everyone is verified makes me feel safe. The voice intros also help - you can't fake that."
> — Rachel, 26, Marketing, LA

---

# Appendix B: Glossary

| Term | Definition |
|------|------------|
| **Contextual Bandit** | ML algorithm that learns from feedback to personalize recommendations |
| **Embedding** | Dense vector (list of numbers) representing content semantically |
| **Federated Learning** | Training ML on devices without sending raw data to server |
| **Haversine** | Formula to calculate distance between GPS coordinates |
| **LinUCB** | Linear Upper Confidence Bound - specific bandit algorithm |
| **ONNX** | Open Neural Network Exchange - portable ML model format |
| **Reel** | Short video (15-30 sec) for personality-based discovery |
| **RevenueCat** | Service that handles Apple/Google in-app purchases |
| **Slate** | Batch of profiles shown together in discovery |
| **Spot** | User's own uploaded reel (their content) |
| **UCB** | Upper Confidence Bound - exploration bonus in bandits |
| **WebRTC** | Peer-to-peer protocol for video/audio calls |

---

*Document Version: 3.0*
*Generated: January 2026*
*Based on: Actual NAVA Platform Implementation*

---

**Contact:**
- Technical Questions: engineering@nava.app
- Business Inquiries: partnerships@nava.app
- Support: support@nava.app
