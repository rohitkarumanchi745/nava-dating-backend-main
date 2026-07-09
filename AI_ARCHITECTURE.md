# NAVA — On-Device AI, Personalization & Agentic Matching

This document describes the AI/ML work added across the Nava backend and iOS
client: **what** changed, **why** we made each decision, and **how** it fits our
existing architecture. The guiding principle throughout: reuse what we already
have (Redis, federated learning, the RL/bandit ranker, our governed API), keep
the user-facing path fast and offline-capable, and add nothing we can't run
cheaply at scale.

---

## 1. The vision

A hybrid AI dating experience:

- **Chat suggestions that work offline**, generated on the phone.
- **Personalization per user** — suggestions that sound like *them*.
- **Agentic matching** — instant matches from learned preferences, without
  manual swiping, driven by natural-language intent.

Everything is designed as **hybrid**: server when online (higher quality),
on-device when offline (privacy + zero token cost), with graceful fallback.

---

## 2. Backend changes

### 2.1 Cross-instance real-time fanout (`src/realtime.rs`)
**What:** chat, call, and app-event WebSocket traffic now fans out across pods
over Redis Pub/Sub, with call sessions shared in Redis so a callee on a different
pod can join.
**Why:** our WebSocket rooms lived in per-process memory — two users on different
pods couldn't see each other's messages. This is the change that actually makes
the "10k+ users" claim real.
**How it fits:** reuses the Redis we already run. Degrades gracefully to
single-pod behavior when Redis is absent — no behavior change without it.

### 2.2 Server-side Apple receipt verification (`services/payments/apple.rs`)
**What:** `/api/payments/verify-apple` now verifies the receipt against Apple
before granting a product.
**Why:** it previously trusted a client-supplied transaction id — anyone could
grant themselves premium. This closes a revenue-fraud hole.
**How it fits:** slots into the existing `PaymentService`; fails closed in
production, keeps working in dev.

### 2.3 Webhook signature hardening
**What:** Razorpay/Stripe/RevenueCat signature checks use constant-time
comparison; Stripe webhooks now reject events outside a 5-minute window.
**Why:** the old `expected == signature` leaks timing, and there was no replay
protection. Small, high-confidence security fixes.

### 2.4 FedLoRA: per-user chat personalization
**What:** migration `032_lora_adapters.sql`, `handlers/lora.rs`, and the training
worker `scripts/fedlora_trainer.py`. The device submits privacy-safe signal and
downloads a small per-user LoRA adapter; the worker trains it and registers a
new version.
**Why:** we want chat suggestions that match each user's voice. A **per-user
LoRA adapter** on a shared base is the cost-efficient way to do that — *not* a
full model per user (millions of models is infeasible).
**How it fits:** reuses our existing federated-learning + differential-privacy
setup. Training is offline/GPU and stops when done; the adapter is applied
on-device at inference. When an adapter is ready, we notify the device over the
same durable outbox + cross-pod fanout from 2.1.

### 2.5 Agentic auto-matcher (`services/matchmaker.rs`, migration `033_auto_match.sql`)
**What:** reciprocal preference scoring — we score A→B *and* B→A and combine them
(geometric mean, so one-sided interest stays low). High-scoring pairs become
proposals or, above a higher bar with safety gates, instant matches. Accept/
decline feeds the model.
**Why:** "instant match without swiping." The prediction is a classic ML problem
we already solve.
**How it fits:** **reuses `MlService::rank_candidates`** (RL + LinUCB + geo +
affinity, already learned from swipes) in both directions — no new model.
Accept/decline calls the existing `record_swipe_weighted`, closing the RL loop.
**Safety:** disabled by default (`AUTO_MATCH_ENABLED=false`); proposals are the
default, instant auto-create is gated behind a high score + both-verified.

### 2.6 Prompt-driven matchmaker agent (all-Rust)
**What:** `/agent/matchmaker/prompt` — a natural-language intent
("verified grad students who love hiking, late 20s") is parsed into structured
filters *in Rust* (`parse_intent`), then run through the reciprocal scorer via a
**governed tool** (`agent_query`).
**Why:** we want conversational matching, but we do **not** want an LLM inside
the data layer.
**How it fits — the key security decision:** the agent sits **above our existing
API boundary**, not in the data layer. It only ever produces *structured,
user-scoped filters* — never raw SQL — so every query inherits our existing
auth, row-scoping, and rate limits. The "learning from engagement" is the
existing Rust RL bandit; no Python and no per-request LLM.

---

## 3. iOS changes (`Packages/NavAI`)

A new local Swift package for on-device AI, split so the pure logic is testable
without the native engine:

- **On-device chat suggestions** — BitNet b1.58-2B-4T via a thin `bitnet.cpp`
  C FFI bridge (`native/nava_bitnet.*`), a Swift actor engine, a model download
  manager, and a hybrid `SuggestionEngine` (server → on-device → templates).
- **Retrieval personalization** — the user's own past messages are embedded and
  retrieved to match their voice in the prompt.
- **Harrier embeddings** (`microsoft/harrier-oss-v1-0.6b`) — runs on-device
  through the same bridge. Confirmed specs: 1024-dim, **last-token** pooling,
  L2-normalized, decoder-only (Qwen3), **asymmetric** (queries need a one-line
  instruction; documents don't).
- **Per-user LoRA adapter** — applied on top of the shared BitNet at inference
  (the device half of §2.4).

---

## 4. Why these decisions (summary of the trade-offs)

| Decision | Why |
|---|---|
| **Hybrid** (server + on-device), not pure server | "Offline" requires on-device; server adds quality when online. |
| **Per-user LoRA + retrieval**, not a full LLM per user | A model per user is infeasible; adapters + retrieval are cheap and scale. |
| **Matching = bandit/RL in Rust**, not an LLM | Match prediction is a classic ML problem; an LLM would be slower, costlier, and no better. |
| **Agent above the API boundary** with governed tools, not an LLM in the data layer | An LLM with DB access over user prompts is a PII/injection risk; governed tools reuse existing authz. |
| **All-Rust + ONNX serving**; Python only for offline training | Serving stays in-stack and fast; the only thing needing Python/GPU is offline LLM/LoRA training, which then exports weights for Rust to serve. |
| **On-device inference** | Offline, private, and zero per-token cost (nothing to bill). |

We evaluated **TOON** (token-oriented notation) and did **not** adopt it: our
user-facing model runs on-device (zero token cost) and our prompts aren't
JSON tables, so it offers no architectural benefit here. It's a drop-in only if
we later feed a record table to a *cloud* LLM.

---

## 5. How it improves the existing architecture

Everything is **additive and reuses existing systems**:

- Real-time fanout → the Redis we already run; unlocks horizontal scaling.
- FedLoRA → our existing federated-learning + DP pipeline.
- Auto-matcher & agent → the existing `MlService` (LinUCB/RL/embeddings), the
  `matches`/`swipes` tables, and the existing swipe-reward loop.
- The agent → our existing governed API and its authz.

No existing behavior is removed; every new capability degrades gracefully when
its dependency (Redis, a configured model, a trained adapter) is absent.
