#!/bin/bash
# Turbine vs eRPC Benchmark Script
# Tests: eth_blockNumber, eth_getBalance, eth_getBlockByNumber
# Measures: latency percentiles, throughput, error rate

set -euo pipefail

TURBINE="https://rpc.observability-server.dealpulley.com/ethereum_sepolia"
ERPC="https://jo440scosc4ogkggkk4o4oog.prod-coolify-rack.dealpulley.com/rpc/evm/11155111"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color
BOLD='\033[1m'

print_header() {
    echo ""
    echo "============================================================"
    echo "  $1"
    echo "============================================================"
}

# --- Test 1: Single request latency (curl-based, 10 requests each) ---
single_request_test() {
    local name="$1"
    local url="$2"
    local body="$3"
    local count="${4:-10}"

    local times=()
    local errors=0

    for i in $(seq 1 "$count"); do
        result=$(curl -s -o /tmp/bench_resp.json -w "%{time_total}" \
            -X POST "$url" \
            -H "Content-Type: application/json" \
            -d "$body" 2>/dev/null || echo "ERROR")

        if [[ "$result" == "ERROR" ]]; then
            ((errors++))
            continue
        fi

        # Check for error in response
        if grep -q '"error"' /tmp/bench_resp.json 2>/dev/null; then
            ((errors++))
            continue
        fi

        times+=("$result")
    done

    if [[ ${#times[@]} -eq 0 ]]; then
        echo "  $name: ALL FAILED ($errors errors)"
        return
    fi

    # Sort times and compute percentiles
    sorted=($(printf '%s\n' "${times[@]}" | sort -n))
    local len=${#sorted[@]}
    local p50_idx=$(( (len * 50 / 100) ))
    local p95_idx=$(( (len * 95 / 100) ))
    local p99_idx=$(( (len - 1) ))

    [[ $p50_idx -ge $len ]] && p50_idx=$((len - 1))
    [[ $p95_idx -ge $len ]] && p95_idx=$((len - 1))

    local min="${sorted[0]}"
    local p50="${sorted[$p50_idx]}"
    local p95="${sorted[$p95_idx]}"
    local p99="${sorted[$p99_idx]}"
    local max="${sorted[$((len - 1))]}"

    printf "  %-10s  min=%-8s p50=%-8s p95=%-8s p99=%-8s max=%-8s errors=%d/%d\n" \
        "$name" "${min}s" "${p50}s" "${p95}s" "${p99}s" "${max}s" "$errors" "$count"
}

# --- Test 2: Concurrent burst test (using hey) ---
burst_test() {
    local name="$1"
    local url="$2"
    local body="$3"
    local concurrency="${4:-50}"
    local total="${5:-200}"

    echo "  $name ($total requests, $concurrency concurrent):"
    hey -n "$total" -c "$concurrency" -m POST \
        -H "Content-Type: application/json" \
        -d "$body" \
        "$url" 2>/dev/null | grep -E "(Requests/sec|Average|Fastest|Slowest|Status code|Latency distribution)" -A 20 | head -20
    echo ""
}

# JSON-RPC payloads
BODY_BLOCK_NUM='{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
BODY_BALANCE='{"jsonrpc":"2.0","method":"eth_getBalance","params":["0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045","latest"],"id":1}'
BODY_GET_BLOCK='{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",false],"id":1}'

echo ""
echo "${BOLD}=======================================================${NC}"
echo "${BOLD}  TURBINE vs eRPC BENCHMARK  $(date '+%Y-%m-%d %H:%M:%S')${NC}"
echo "${BOLD}=======================================================${NC}"
echo ""
echo "Turbine: $TURBINE"
echo "eRPC:    $ERPC"

# =====================================================
# TEST 1: Single request latency (cold start)
# =====================================================
print_header "TEST 1: Cold Start - First 5 Requests"
echo "  ${YELLOW}--- Turbine ---${NC}"
single_request_test "blockNum" "$TURBINE" "$BODY_BLOCK_NUM" 5
single_request_test "balance" "$TURBINE" "$BODY_BALANCE" 5
single_request_test "getBlock" "$TURBINE" "$BODY_GET_BLOCK" 5

echo ""
echo "  ${YELLOW}--- eRPC ---${NC}"
single_request_test "blockNum" "$ERPC" "$BODY_BLOCK_NUM" 5
single_request_test "balance" "$ERPC" "$BODY_BALANCE" 5
single_request_test "getBlock" "$ERPC" "$BODY_GET_BLOCK" 5

# =====================================================
# TEST 2: Warm request latency (after warmup)
# =====================================================
print_header "TEST 2: Warm Requests - 20 Sequential Requests"
# Warmup
for i in $(seq 1 10); do
    curl -s -o /dev/null -X POST "$TURBINE" -H "Content-Type: application/json" -d "$BODY_BLOCK_NUM" &
    curl -s -o /dev/null -X POST "$ERPC" -H "Content-Type: application/json" -d "$BODY_BLOCK_NUM" &
done
wait

echo "  ${YELLOW}--- Turbine ---${NC}"
single_request_test "blockNum" "$TURBINE" "$BODY_BLOCK_NUM" 20
single_request_test "balance" "$TURBINE" "$BODY_BALANCE" 20
single_request_test "getBlock" "$TURBINE" "$BODY_GET_BLOCK" 20

echo ""
echo "  ${YELLOW}--- eRPC ---${NC}"
single_request_test "blockNum" "$ERPC" "$BODY_BLOCK_NUM" 20
single_request_test "balance" "$ERPC" "$BODY_BALANCE" 20
single_request_test "getBlock" "$ERPC" "$BODY_GET_BLOCK" 20

# =====================================================
# TEST 3: Burst test (50 concurrent requests)
# =====================================================
print_header "TEST 3: Burst - 200 requests, 50 concurrent (eth_getBalance)"
echo "  ${YELLOW}--- Turbine ---${NC}"
burst_test "Turbine" "$TURBINE" "$BODY_BALANCE" 50 200

echo "  ${YELLOW}--- eRPC ---${NC}"
burst_test "eRPC" "$ERPC" "$BODY_BALANCE" 50 200

# =====================================================
# TEST 4: Sustained load (10 req/s for 30 seconds)
# =====================================================
print_header "TEST 4: Sustained Load - 10 req/s for 30 seconds (eth_blockNumber)"
echo "  ${YELLOW}--- Turbine ---${NC}"
hey -n 300 -c 10 -q 10 -m POST \
    -H "Content-Type: application/json" \
    -d "$BODY_BLOCK_NUM" \
    "$TURBINE" 2>/dev/null | grep -E "(Requests/sec|Average|Fastest|Slowest|Status code|Latency distribution)" -A 20 | head -20
echo ""

echo "  ${YELLOW}--- eRPC ---${NC}"
hey -n 300 -c 10 -q 10 -m POST \
    -H "Content-Type: application/json" \
    -d "$BODY_BLOCK_NUM" \
    "$ERPC" 2>/dev/null | grep -E "(Requests/sec|Average|Fastest|Slowest|Status code|Latency distribution)" -A 20 | head -20
echo ""

# =====================================================
# TEST 5: Heavy burst (100 concurrent)
# =====================================================
print_header "TEST 5: Heavy Burst - 500 requests, 100 concurrent (eth_getBalance)"
echo "  ${YELLOW}--- Turbine ---${NC}"
burst_test "Turbine" "$TURBINE" "$BODY_BALANCE" 100 500

echo "  ${YELLOW}--- eRPC ---${NC}"
burst_test "eRPC" "$ERPC" "$BODY_BALANCE" 100 500

echo ""
echo "${BOLD}=======================================================${NC}"
echo "${BOLD}  BENCHMARK COMPLETE${NC}"
echo "${BOLD}=======================================================${NC}"
