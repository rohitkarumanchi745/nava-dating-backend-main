#!/usr/bin/env bash
# =============================================================================
# NAVA — Shadow-Mode Notification Canary Script
# =============================================================================
# Enables shadow-mode bandit for notifications, monitors key metrics over a
# configurable window, evaluates uplift vs. control, and gates promotion
# to full live mode.
#
# Usage:
#   ./canary-shadow-notif.sh [--enable|--monitor|--evaluate|--promote|--rollback]
#
# Prerequisites:
#   - kubectl configured for target cluster
#   - psql access to nava database
#   - curl access to Prometheus & notification-service /metrics
# =============================================================================

set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────────────
NAMESPACE="${NAVA_NAMESPACE:-nava}"
NOTIF_SERVICE="${NOTIF_SERVICE_URL:-http://notification-service.${NAMESPACE}.svc:8007}"
PROMETHEUS="${PROMETHEUS_URL:-http://prometheus.${NAMESPACE}.svc:9090}"
DB_URL="${DATABASE_URL:-postgres://localhost/nava}"
CANARY_DURATION_HOURS="${CANARY_DURATION:-24}"
MIN_SAMPLES=200           # Minimum notification sends before evaluating
UPLIFT_THRESHOLD=0.02     # 2% absolute engagement uplift to promote
MAX_THROTTLE_RATE=0.40    # Abort if throttle rate exceeds 40%
MAX_VARIANT_SKEW=0.80     # Abort if one variant >80% of sends

# ── Colors ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'

log()  { echo -e "${GREEN}[canary]${NC} $*"; }
warn() { echo -e "${YELLOW}[canary]${NC} $*"; }
err()  { echo -e "${RED}[canary]${NC} $*" >&2; }

# ── Step 1: Enable Shadow Mode ──────────────────────────────────────────────
enable_shadow() {
    log "Enabling shadow mode for notification bandit..."

    # Set env var on the notification-service deployment
    kubectl -n "$NAMESPACE" set env deployment/notification-service \
        NOTIF_BANDIT_SHADOW_MODE=true

    # Wait for rollout
    kubectl -n "$NAMESPACE" rollout status deployment/notification-service --timeout=120s

    log "Shadow mode enabled. Bandit selects variants but always sends control copy."
    log "Canary window: ${CANARY_DURATION_HOURS}h — run './canary-shadow-notif.sh --monitor' to check progress."

    # Record start time in a marker file
    date +%s > /tmp/nava-canary-start
    echo "$CANARY_DURATION_HOURS" > /tmp/nava-canary-duration
}

# ── Step 2: Monitor Metrics ─────────────────────────────────────────────────
monitor() {
    log "=== Shadow-Mode Canary Health Check ==="
    echo ""

    # Check canary elapsed time
    if [[ -f /tmp/nava-canary-start ]]; then
        local start
        start=$(cat /tmp/nava-canary-start)
        local now
        now=$(date +%s)
        local elapsed_h=$(( (now - start) / 3600 ))
        local target_h
        target_h=$(cat /tmp/nava-canary-duration 2>/dev/null || echo "$CANARY_DURATION_HOURS")
        log "Elapsed: ${elapsed_h}h / ${target_h}h"
    fi

    echo ""
    log "── Notification Policy Metrics ──"
    curl -s "${NOTIF_SERVICE}/metrics" 2>/dev/null | grep -E "^notif_" || warn "Could not reach notification service"

    echo ""
    log "── FL Aggregation Metrics ──"
    # Query main backend /metrics for FL gauges
    local backend_url="${BACKEND_URL:-http://nava-backend.${NAMESPACE}.svc:8080}"
    curl -s "${backend_url}/metrics" 2>/dev/null | grep -E "^app_fl_" || warn "Could not reach backend /metrics"

    echo ""
    log "── SLO Alert Status (firing) ──"
    # Check Prometheus for any firing notification or FL alerts
    curl -s "${PROMETHEUS}/api/v1/alerts" 2>/dev/null \
        | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    alerts = data.get('data', {}).get('alerts', [])
    notif_fl = [a for a in alerts if a.get('state') == 'firing'
                and any(k in a.get('labels', {}).get('alertname', '')
                        for k in ['Notif', 'Fl', 'Ml'])]
    if notif_fl:
        for a in notif_fl:
            print(f\"  🔥 {a['labels']['alertname']}: {a['annotations'].get('summary', 'N/A')}\")
    else:
        print('  No notification/FL alerts firing.')
except Exception:
    print('  Could not parse Prometheus alerts.')
" 2>/dev/null || warn "Could not reach Prometheus"

    echo ""
    log "── Notification Outcomes (DB) ──"
    psql "$DB_URL" -t -c "
        SELECT
            variant_id,
            COUNT(*) AS sends,
            COUNT(*) FILTER (WHERE opened) AS opens,
            ROUND(100.0 * COUNT(*) FILTER (WHERE opened) / NULLIF(COUNT(*), 0), 2) AS open_rate_pct,
            ROUND(AVG(EXTRACT(EPOCH FROM (opened_at - sent_at)))::numeric, 1) AS avg_open_secs
        FROM notification_outcomes
        WHERE sent_at > NOW() - INTERVAL '${CANARY_DURATION_HOURS} hours'
        GROUP BY variant_id
        ORDER BY sends DESC;
    " 2>/dev/null || warn "Could not query notification_outcomes (DB may not be reachable)"

    echo ""
    log "── FL Stability (DB) ──"
    psql "$DB_URL" -t -c "
        SELECT
            'personalization_head_norm' AS metric,
            ROUND(SQRT(SUM(w * w))::numeric, 6) AS value
        FROM (SELECT unnest(weights) AS w FROM fl_global_head ORDER BY updated_at DESC LIMIT 1) sub
        UNION ALL
        SELECT 'cold_start_buckets', COUNT(DISTINCT intent_bucket)
        FROM fl_cold_start_biases
        WHERE num_contributors > 0;
    " 2>/dev/null || log "(FL tables not yet created — expected before first aggregation round)"
}

# ── Step 3: Evaluate Uplift ─────────────────────────────────────────────────
evaluate() {
    log "=== Evaluating Shadow-Mode Canary ==="
    echo ""

    # Pull variant stats from the outcomes table
    local result
    result=$(psql "$DB_URL" -t -A -F'|' -c "
        SELECT
            variant_id,
            COUNT(*) AS sends,
            ROUND(100.0 * COUNT(*) FILTER (WHERE opened) / NULLIF(COUNT(*), 0), 4) AS open_rate
        FROM notification_outcomes
        WHERE sent_at > NOW() - INTERVAL '${CANARY_DURATION_HOURS} hours'
        GROUP BY variant_id
        ORDER BY sends DESC;
    " 2>/dev/null)

    if [[ -z "$result" ]]; then
        err "No outcome data found. Wait for more sends before evaluating."
        exit 1
    fi

    echo "$result" | while IFS='|' read -r variant sends rate; do
        log "  Variant: ${variant}  Sends: ${sends}  Open rate: ${rate}%"
    done

    # Check total sample size
    local total_sends
    total_sends=$(echo "$result" | awk -F'|' '{s+=$2} END{print s}')
    if (( total_sends < MIN_SAMPLES )); then
        warn "Only ${total_sends} sends (need ${MIN_SAMPLES}). Wait for more data."
        exit 1
    fi

    # Check throttle rate from Prometheus
    local throttle_rate
    throttle_rate=$(curl -s "${PROMETHEUS}/api/v1/query?query=\
(notif_blocked_cap%2Bnotif_blocked_cooldown%2Bnotif_blocked_optout%2Bnotif_deferred_quiet%2Bnotif_deferred_timing)\
/(notif_sent_total%2Bnotif_blocked_cap%2Bnotif_blocked_cooldown%2Bnotif_blocked_optout%2Bnotif_deferred_quiet%2Bnotif_deferred_timing%2B1)" \
        2>/dev/null | python3 -c "
import sys, json
try:
    v = json.load(sys.stdin)['data']['result'][0]['value'][1]
    print(v)
except Exception:
    print('0')
" 2>/dev/null || echo "0")

    log "Throttle rate: ${throttle_rate}"

    # Check variant skew
    local max_share
    max_share=$(echo "$result" | awk -F'|' -v total="$total_sends" '{
        share = $2 / total
        if (share > max) max = share
    } END { printf "%.4f", max }')
    log "Max variant share: ${max_share}"

    # Safety gates
    if (( $(echo "$throttle_rate > $MAX_THROTTLE_RATE" | bc -l 2>/dev/null || echo 0) )); then
        err "ABORT: Throttle rate ${throttle_rate} exceeds ${MAX_THROTTLE_RATE}. Check caps/quiet hours config."
        exit 1
    fi

    if (( $(echo "$max_share > $MAX_VARIANT_SKEW" | bc -l 2>/dev/null || echo 0) )); then
        warn "WARNING: Variant skew ${max_share} exceeds ${MAX_VARIANT_SKEW}. Bandit exploration may be stuck."
    fi

    # Compare best bandit variant vs control (first variant)
    local control_rate best_rate best_variant
    control_rate=$(echo "$result" | head -1 | awk -F'|' '{print $3}')
    best_variant=$(echo "$result" | sort -t'|' -k3 -rn | head -1 | awk -F'|' '{print $1}')
    best_rate=$(echo "$result" | sort -t'|' -k3 -rn | head -1 | awk -F'|' '{print $3}')

    local uplift
    uplift=$(echo "$best_rate - $control_rate" | bc -l 2>/dev/null || echo "0")
    log "Control open rate: ${control_rate}%  Best variant (${best_variant}): ${best_rate}%  Uplift: ${uplift}pp"

    local threshold_pct
    threshold_pct=$(echo "$UPLIFT_THRESHOLD * 100" | bc -l 2>/dev/null || echo "2")
    if (( $(echo "$uplift >= $threshold_pct" | bc -l 2>/dev/null || echo 0) )); then
        log "PASS: Uplift ${uplift}pp >= ${threshold_pct}pp threshold. Safe to promote."
        log "Run: ./canary-shadow-notif.sh --promote"
    else
        warn "Uplift ${uplift}pp < ${threshold_pct}pp. Consider extending canary or reviewing variants."
    fi
}

# ── Step 4: Promote to Live ─────────────────────────────────────────────────
promote() {
    log "Promoting bandit from shadow mode to live..."

    kubectl -n "$NAMESPACE" set env deployment/notification-service \
        NOTIF_BANDIT_SHADOW_MODE=false

    kubectl -n "$NAMESPACE" rollout status deployment/notification-service --timeout=120s

    log "Bandit is now LIVE — variants will be sent to users based on Thompson Sampling."
    log "Monitor with: ./canary-shadow-notif.sh --monitor"

    # Clean up canary markers
    rm -f /tmp/nava-canary-start /tmp/nava-canary-duration
}

# ── Step 5: Rollback ────────────────────────────────────────────────────────
rollback() {
    warn "Rolling back to shadow mode (safe default)..."

    kubectl -n "$NAMESPACE" set env deployment/notification-service \
        NOTIF_BANDIT_SHADOW_MODE=true

    kubectl -n "$NAMESPACE" rollout status deployment/notification-service --timeout=120s

    log "Rolled back to shadow mode. All users receive control variant."
    rm -f /tmp/nava-canary-start /tmp/nava-canary-duration
}

# ── Main ─────────────────────────────────────────────────────────────────────
case "${1:---monitor}" in
    --enable)   enable_shadow ;;
    --monitor)  monitor ;;
    --evaluate) evaluate ;;
    --promote)  promote ;;
    --rollback) rollback ;;
    --help|-h)
        echo "Usage: $0 [--enable|--monitor|--evaluate|--promote|--rollback]"
        echo ""
        echo "  --enable     Enable shadow mode (bandit selects but sends control)"
        echo "  --monitor    Check canary health: metrics, alerts, DB outcomes"
        echo "  --evaluate   Evaluate uplift vs. control with safety gates"
        echo "  --promote    Promote bandit to live (sends winning variant)"
        echo "  --rollback   Roll back to shadow mode"
        ;;
    *)
        err "Unknown command: $1"
        echo "Run $0 --help for usage."
        exit 1
        ;;
esac
