#!/usr/bin/env bash
# What the anonymous usage reports say (crates/core/src/telemetry.rs, stored by
# worker/src/index.js in the Analytics Engine dataset `cctop_telemetry`).
#
#   tools/usage.sh [days]     # default 7
#
# The token needs "Account Analytics: Read" on the account the Worker runs in;
# wrangler's own login does not carry that scope. It is read from
# CLOUDFLARE_API_TOKEN, or else from ~/.config/cctop/usage-token, a file kept
# private (chmod 600) so the token stays out of shell history. Counts are weighted by
# `_sample_interval`, since Analytics Engine samples at volume, and the
# `0.0.0-test` reports sent by hand while checking the Worker are left out.
#
# Row layout, as the Worker writes it: index1 is the install's random id,
# blob1-3 are version, os and arch, blob4 is the kind ("report", "feature",
# "agents" or "crash"), then blob5 is the feature or agent name, or the crash
# location with blob6 its message, and double1 is the count.
set -euo pipefail

token_file="${XDG_CONFIG_HOME:-$HOME/.config}/cctop/usage-token"
[ -n "${CLOUDFLARE_API_TOKEN:-}" ] || CLOUDFLARE_API_TOKEN=$(cat "$token_file" 2>/dev/null || true)
: "${CLOUDFLARE_API_TOKEN:?needs a token with Account Analytics: Read, in the environment or $token_file}"
: "${CLOUDFLARE_ACCOUNT_ID:=e566c3eea9bb7ba66b02108291bf1646}"
days="${1:-7}"
case "$days" in '' | *[!0-9]*) echo "days must be a number" >&2 && exit 2 ;; esac

sql() {
    curl --fail-with-body --silent --show-error \
        -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
        --data-binary "$1 FORMAT JSON" \
        "https://api.cloudflare.com/client/v4/accounts/$CLOUDFLARE_ACCOUNT_ID/analytics_engine/sql" |
        # Analytics Engine has no TSV-with-header format; JSON carries the names.
        jq -r '[.meta[].name], (.data[] | [.[]]) | @tsv' | column -t -s $'\t'
}

real="blob1 != '0.0.0-test'"
window="timestamp > NOW() - INTERVAL '$days' DAY"

echo "== installs reporting, per day (last $days days)"
sql "SELECT toStartOfDay(timestamp) AS day, COUNT(DISTINCT index1) AS installs
     FROM cctop_telemetry WHERE blob4 = 'report' AND $real AND $window
     GROUP BY day ORDER BY day"

echo; echo "== distinct installs over the whole window"
sql "SELECT COUNT(DISTINCT index1) AS installs
     FROM cctop_telemetry WHERE blob4 = 'report' AND $real AND $window"

echo; echo "== versions"
sql "SELECT blob1 AS version, COUNT(DISTINCT index1) AS installs
     FROM cctop_telemetry WHERE blob4 = 'report' AND $real AND $window
     GROUP BY version ORDER BY installs DESC"

echo; echo "== os / arch"
sql "SELECT blob2 AS os, blob3 AS arch, COUNT(DISTINCT index1) AS installs
     FROM cctop_telemetry WHERE blob4 = 'report' AND $real AND $window
     GROUP BY os, arch ORDER BY installs DESC"

echo; echo "== features: how many installs use each, and how often"
sql "SELECT blob5 AS feature, COUNT(DISTINCT index1) AS installs,
            SUM(double1 * _sample_interval) AS runs
     FROM cctop_telemetry WHERE blob4 = 'feature' AND $real AND $window
     GROUP BY feature ORDER BY installs DESC"

echo; echo "== agents watched: installs seeing each kind, and the most at once"
sql "SELECT blob5 AS agent, COUNT(DISTINCT index1) AS installs, MAX(double1) AS most_at_once
     FROM cctop_telemetry WHERE blob4 = 'agents' AND $real AND $window
     GROUP BY agent ORDER BY installs DESC"

echo; echo "== crashes"
sql "SELECT blob1 AS version, blob5 AS location, blob6 AS message,
            SUM(_sample_interval) AS crashes, COUNT(DISTINCT index1) AS installs
     FROM cctop_telemetry WHERE blob4 = 'crash' AND $real AND $window
     GROUP BY version, location, message ORDER BY crashes DESC LIMIT 20"
