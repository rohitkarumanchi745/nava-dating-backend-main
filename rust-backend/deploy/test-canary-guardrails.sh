#!/usr/bin/env bash
# =============================================================================
# Dry-run test harness for canary-shadow-notif.sh guardrails
# =============================================================================
# Exercises every gate (sample size, variant skew, throttle rate, uplift calc)
# using synthetic pipe-delimited data — no DB, Prometheus, or K8s needed.
#
# Usage: ./test-canary-guardrails.sh
# =============================================================================

set -uo pipefail
# NOTE: -e is intentionally omitted — arithmetic (( )) returns exit 1
# when the expression evaluates to 0, which would abort under errexit.

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
PASS=0; FAIL=0

assert_eq() {
    local label="$1" expected="$2" actual="$3"
    if [[ "$expected" == "$actual" ]]; then
        echo -e "  ${GREEN}PASS${NC} $label"
        ((PASS++))
    else
        echo -e "  ${RED}FAIL${NC} $label — expected '$expected', got '$actual'"
        ((FAIL++))
    fi
}

assert_exit() {
    local label="$1" expected_exit="$2"
    shift 2
    local actual_exit=0
    "$@" >/dev/null 2>&1 || actual_exit=$?
    if [[ "$expected_exit" == "$actual_exit" ]]; then
        echo -e "  ${GREEN}PASS${NC} $label (exit=$actual_exit)"
        ((PASS++))
    else
        echo -e "  ${RED}FAIL${NC} $label — expected exit $expected_exit, got $actual_exit"
        ((FAIL++))
    fi
}

# ─── Guardrail constants (must match canary-shadow-notif.sh) ─────────────────
MIN_SAMPLES=200
UPLIFT_THRESHOLD=0.02
MAX_THROTTLE_RATE=0.40
MAX_VARIANT_SKEW=0.80

# =============================================================================
echo -e "\n${YELLOW}=== Test 1: Sample Size Gate ===${NC}\n"
# =============================================================================

# Too few sends — should reject
RESULT_LOW="control|50|8.5000
bandit_a|40|10.2000
bandit_b|30|7.1000"

total_low=$(echo "$RESULT_LOW" | awk -F'|' '{s+=$2} END{print s}')
assert_eq "total_low = 120" "120" "$total_low"

gate_low="pass"
if (( total_low < MIN_SAMPLES )); then gate_low="blocked"; fi
assert_eq "sample_size_gate blocks 120 < 200" "blocked" "$gate_low"

# Enough sends — should pass
RESULT_OK="control|150|8.5000
bandit_a|120|11.2000
bandit_b|80|7.1000"

total_ok=$(echo "$RESULT_OK" | awk -F'|' '{s+=$2} END{print s}')
assert_eq "total_ok = 350" "350" "$total_ok"

gate_ok="pass"
if (( total_ok < MIN_SAMPLES )); then gate_ok="blocked"; fi
assert_eq "sample_size_gate passes 350 >= 200" "pass" "$gate_ok"

# =============================================================================
echo -e "\n${YELLOW}=== Test 2: Variant Skew Gate ===${NC}\n"
# =============================================================================

# High skew — one variant dominates
RESULT_SKEWED="control|900|8.0000
bandit_a|50|12.0000
bandit_b|50|6.0000"

total_skewed=$(echo "$RESULT_SKEWED" | awk -F'|' '{s+=$2} END{print s}')
max_share_skewed=$(echo "$RESULT_SKEWED" | awk -F'|' -v total="$total_skewed" '{
    share = $2 / total
    if (share > max) max = share
} END { printf "%.4f", max }')
assert_eq "max_share_skewed = 0.9000" "0.9000" "$max_share_skewed"

skew_gate="pass"
if (( $(echo "$max_share_skewed > $MAX_VARIANT_SKEW" | bc -l) )); then skew_gate="warned"; fi
assert_eq "skew_gate warns on 0.90 > 0.80" "warned" "$skew_gate"

# Balanced — no skew
RESULT_BALANCED="control|130|8.5000
bandit_a|120|11.2000
bandit_b|100|7.1000"

total_balanced=$(echo "$RESULT_BALANCED" | awk -F'|' '{s+=$2} END{print s}')
max_share_balanced=$(echo "$RESULT_BALANCED" | awk -F'|' -v total="$total_balanced" '{
    share = $2 / total
    if (share > max) max = share
} END { printf "%.4f", max }')
assert_eq "max_share_balanced = 0.3714" "0.3714" "$max_share_balanced"

skew_gate_bal="pass"
if (( $(echo "$max_share_balanced > $MAX_VARIANT_SKEW" | bc -l) )); then skew_gate_bal="warned"; fi
assert_eq "skew_gate passes on 0.37 < 0.80" "pass" "$skew_gate_bal"

# =============================================================================
echo -e "\n${YELLOW}=== Test 3: Throttle Rate Gate ===${NC}\n"
# =============================================================================

# High throttle — should abort
throttle_high="0.55"
throttle_gate_high="pass"
if (( $(echo "$throttle_high > $MAX_THROTTLE_RATE" | bc -l) )); then throttle_gate_high="abort"; fi
assert_eq "throttle_gate aborts on 0.55 > 0.40" "abort" "$throttle_gate_high"

# Normal throttle
throttle_normal="0.15"
throttle_gate_normal="pass"
if (( $(echo "$throttle_normal > $MAX_THROTTLE_RATE" | bc -l) )); then throttle_gate_normal="abort"; fi
assert_eq "throttle_gate passes on 0.15 < 0.40" "pass" "$throttle_gate_normal"

# Edge case — exactly at threshold
throttle_edge="0.40"
throttle_gate_edge="pass"
if (( $(echo "$throttle_edge > $MAX_THROTTLE_RATE" | bc -l) )); then throttle_gate_edge="abort"; fi
assert_eq "throttle_gate passes on 0.40 == 0.40 (not >)" "pass" "$throttle_gate_edge"

# =============================================================================
echo -e "\n${YELLOW}=== Test 4: Uplift Calculation ===${NC}\n"
# =============================================================================

# Clear uplift — bandit_a at 11.2% vs control at 8.5% → 2.7pp
RESULT_UPLIFT="control|150|8.5000
bandit_a|120|11.2000
bandit_b|80|7.1000"

control_rate=$(echo "$RESULT_UPLIFT" | head -1 | awk -F'|' '{print $3}')
best_variant=$(echo "$RESULT_UPLIFT" | sort -t'|' -k3 -rn | head -1 | awk -F'|' '{print $1}')
best_rate=$(echo "$RESULT_UPLIFT" | sort -t'|' -k3 -rn | head -1 | awk -F'|' '{print $3}')

uplift=$(echo "$best_rate - $control_rate" | bc -l)
threshold_pct=$(echo "$UPLIFT_THRESHOLD * 100" | bc -l)

assert_eq "control_rate = 8.5000" "8.5000" "$control_rate"
assert_eq "best_variant = bandit_a" "bandit_a" "$best_variant"
assert_eq "best_rate = 11.2000" "11.2000" "$best_rate"

# Check uplift = 2.7
uplift_rounded=$(printf "%.1f" "$uplift")
assert_eq "uplift = 2.7pp" "2.7" "$uplift_rounded"

# Threshold check
promote_decision="no"
if (( $(echo "$uplift >= $threshold_pct" | bc -l) )); then promote_decision="yes"; fi
assert_eq "promote decision (2.7 >= 2.0) = yes" "yes" "$promote_decision"

# No uplift — bandit worse than control
RESULT_NO_UPLIFT="control|200|10.0000
bandit_a|150|9.0000
bandit_b|100|8.0000"

control_rate_2=$(echo "$RESULT_NO_UPLIFT" | head -1 | awk -F'|' '{print $3}')
best_rate_2=$(echo "$RESULT_NO_UPLIFT" | sort -t'|' -k3 -rn | head -1 | awk -F'|' '{print $3}')
uplift_2=$(echo "$best_rate_2 - $control_rate_2" | bc -l)

promote_decision_2="no"
if (( $(echo "$uplift_2 >= $threshold_pct" | bc -l) )); then promote_decision_2="yes"; fi
assert_eq "no promote when control is best (uplift=0)" "no" "$promote_decision_2"

# Marginal uplift — below threshold
RESULT_MARGINAL="control|200|10.0000
bandit_a|150|11.5000
bandit_b|100|9.0000"

control_rate_3=$(echo "$RESULT_MARGINAL" | head -1 | awk -F'|' '{print $3}')
best_rate_3=$(echo "$RESULT_MARGINAL" | sort -t'|' -k3 -rn | head -1 | awk -F'|' '{print $3}')
uplift_3=$(echo "$best_rate_3 - $control_rate_3" | bc -l)

promote_decision_3="no"
if (( $(echo "$uplift_3 >= $threshold_pct" | bc -l) )); then promote_decision_3="yes"; fi
assert_eq "marginal uplift 1.5pp < 2.0pp threshold = no promote" "no" "$promote_decision_3"

# =============================================================================
echo -e "\n${YELLOW}=== Test 5: Combined Pipeline (happy path) ===${NC}\n"
# =============================================================================

RESULT_HAPPY="control|200|8.0000
bandit_a|180|12.5000
bandit_b|120|9.0000"

total_happy=$(echo "$RESULT_HAPPY" | awk -F'|' '{s+=$2} END{print s}')
max_share_happy=$(echo "$RESULT_HAPPY" | awk -F'|' -v total="$total_happy" '{
    share = $2 / total; if (share > max) max = share
} END { printf "%.4f", max }')
throttle_happy="0.12"
ctrl_happy=$(echo "$RESULT_HAPPY" | head -1 | awk -F'|' '{print $3}')
best_happy=$(echo "$RESULT_HAPPY" | sort -t'|' -k3 -rn | head -1 | awk -F'|' '{print $3}')
uplift_happy=$(echo "$best_happy - $ctrl_happy" | bc -l)

pipeline="PASS"
# Gate 1: sample size
if (( total_happy < MIN_SAMPLES )); then pipeline="FAIL:samples"; fi
# Gate 2: throttle
if (( $(echo "$throttle_happy > $MAX_THROTTLE_RATE" | bc -l) )); then pipeline="FAIL:throttle"; fi
# Gate 3: skew (warn only, don't block)
skew_warn=""
if (( $(echo "$max_share_happy > $MAX_VARIANT_SKEW" | bc -l) )); then skew_warn="(skew warning)"; fi
# Gate 4: uplift
if (( $(echo "$uplift_happy < $threshold_pct" | bc -l) )); then pipeline="FAIL:uplift"; fi

assert_eq "happy path pipeline result" "PASS" "$pipeline"
assert_eq "no skew warning" "" "$skew_warn"

# =============================================================================
echo -e "\n${YELLOW}=== Test 6: Combined Pipeline (multi-gate failure) ===${NC}\n"
# =============================================================================

RESULT_BAD="control|30|10.0000
bandit_a|20|10.5000"

total_bad=$(echo "$RESULT_BAD" | awk -F'|' '{s+=$2} END{print s}')
throttle_bad="0.65"

failures=""
if (( total_bad < MIN_SAMPLES )); then failures="${failures}samples,"; fi
if (( $(echo "$throttle_bad > $MAX_THROTTLE_RATE" | bc -l) )); then failures="${failures}throttle,"; fi

assert_eq "bad pipeline catches sample+throttle" "samples,throttle," "$failures"

# =============================================================================
echo -e "\n${YELLOW}=== Test 7: Edge Cases ===${NC}\n"
# =============================================================================

# Exactly MIN_SAMPLES
RESULT_EXACT="control|100|8.0000
bandit_a|100|10.5000"

total_exact=$(echo "$RESULT_EXACT" | awk -F'|' '{s+=$2} END{print s}')
gate_exact="pass"
if (( total_exact < MIN_SAMPLES )); then gate_exact="blocked"; fi
assert_eq "exactly 200 sends passes" "pass" "$gate_exact"

# Single variant (shadow mode — only control sends)
RESULT_SINGLE="control|500|9.0000"
max_share_single=$(echo "$RESULT_SINGLE" | awk -F'|' -v total=500 '{
    share = $2 / total; if (share > max) max = share
} END { printf "%.4f", max }')
assert_eq "single variant share = 1.0000" "1.0000" "$max_share_single"

single_skew="pass"
if (( $(echo "$max_share_single > $MAX_VARIANT_SKEW" | bc -l) )); then single_skew="warned"; fi
assert_eq "single variant triggers skew warning" "warned" "$single_skew"

# Zero uplift boundary
RESULT_ZERO="control|200|10.0000
bandit_a|200|12.0000"
ctrl_zero=$(echo "$RESULT_ZERO" | head -1 | awk -F'|' '{print $3}')
best_zero=$(echo "$RESULT_ZERO" | sort -t'|' -k3 -rn | head -1 | awk -F'|' '{print $3}')
uplift_zero=$(echo "$best_zero - $ctrl_zero" | bc -l)
uplift_zero_rounded=$(printf "%.1f" "$uplift_zero")
assert_eq "exact 2.0pp uplift = threshold → promote" "2.0" "$uplift_zero_rounded"

promote_exact="no"
if (( $(echo "$uplift_zero >= $threshold_pct" | bc -l) )); then promote_exact="yes"; fi
assert_eq "uplift == threshold passes (>=)" "yes" "$promote_exact"

# =============================================================================
echo -e "\n${YELLOW}=== Test 8: Canary Script --help exits cleanly ===${NC}\n"
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
assert_exit "--help exits 0" 0 bash "$SCRIPT_DIR/canary-shadow-notif.sh" --help
assert_exit "unknown command exits 1" 1 bash "$SCRIPT_DIR/canary-shadow-notif.sh" --bogus

# =============================================================================
# Summary
# =============================================================================
echo -e "\n${YELLOW}═══════════════════════════════════════${NC}"
echo -e "  ${GREEN}Passed: ${PASS}${NC}  ${RED}Failed: ${FAIL}${NC}"
echo -e "${YELLOW}═══════════════════════════════════════${NC}\n"

if (( FAIL > 0 )); then
    echo -e "${RED}GUARDRAIL TESTS FAILED${NC}"
    exit 1
fi

echo -e "${GREEN}All canary guardrail tests passed.${NC}"
