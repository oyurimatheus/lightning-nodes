#!/usr/bin/env bash
# Reproducible benchmark for the full HTTP endpoint against a running stack
# (start it with `just up`). Measures p50/p95/p99 latency of GET /nodes.
#
# Usage: ./scripts/benchmark.sh [requests] [concurrency]
# Environment: BASE_URL (default http://localhost:3000)
set -euo pipefail

BASE_URL="${BASE_URL:-http://localhost:3000}"
REQUESTS="${1:-2000}"
CONCURRENCY="${2:-50}"

command -v curl >/dev/null || { echo "curl is required" >&2; exit 1; }
command -v sort >/dev/null || { echo "sort is required" >&2; exit 1; }

echo "Benchmarking GET $BASE_URL/nodes (requests=$REQUESTS concurrency=$CONCURRENCY)"

TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT

# Bounded concurrency through xargs; each line is one request's total time (s).
seq 1 "$REQUESTS" | xargs -P "$CONCURRENCY" -I{} sh -c \
  "curl -o /dev/null -s -w '%{time_total}\\n' '$BASE_URL/nodes'" > "$TMP"

sort -n "$TMP" | awk -v n="$REQUESTS" '
  { values[NR] = $1 }
  END {
    if (NR == 0) { print "no samples"; exit 1 }
    p50 = values[int(NR * 0.50 + 0.5)] * 1000
    p95 = values[int(NR * 0.95 + 0.5)] * 1000
    p99 = values[int(NR * 0.99 + 0.5)] * 1000
    printf "samples=%d p50=%.1fms p95=%.1fms p99=%.1fms\n", NR, p50, p95, p99
  }'
