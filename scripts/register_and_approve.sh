#!/bin/bash
# Full end-to-end registration: start request, auto-approve via CLI, capture key.
# Usage: bash register_and_approve.sh
set -euo pipefail

cd /mnt/c/Users/USER/Downloads/core-sync-rs

export SIA_INDEXER_URL=http://127.0.0.1:9982
export SIA_RECOVERY_PHRASE="note argue tray world what illegal intact cement coffee nose shiver tell"
ADMIN_PASS="sia_api_password_core_sync"

echo "=== Step 1: Starting registration in background ==="
cargo run --example register_app_key --features sia-sdk > /tmp/reg_output.txt 2>&1 &
REG_PID=$!

# Wait for the approval URL to appear in output
echo "Waiting for approval URL..."
for i in $(seq 1 60); do
  if grep -q 'auth/connect/' /tmp/reg_output.txt 2>/dev/null; then
    break
  fi
  sleep 1
done

REQ_URL=$(grep 'http.*auth/connect/' /tmp/reg_output.txt | head -1 | tr -d ' ')
if [ -z "$REQ_URL" ]; then
  echo "FAILED: no approval URL found after 60s"
  cat /tmp/reg_output.txt
  kill $REG_PID 2>/dev/null || true
  exit 1
fi

REQ_ID=$(echo "$REQ_URL" | grep -oE '[a-f0-9]{32}$')
echo "Got request ID: $REQ_ID"
echo "Approval URL: $REQ_URL"

echo ""
echo "=== Step 2: Approving connection from CLI ==="
# The approval page JS does: fetch(url, { method: POST, headers: { Authorization: Basic(":"+password) }, body: {"approve":true} })
printf '{"approve":true}' > /tmp/ap.json

# Try the admin password on the app port
HTTP_CODE=$(curl -s -o /tmp/approve_resp.txt -w "%{http_code}" \
  -X POST "http://127.0.0.1:9982/auth/connect/${REQ_ID}" \
  -H "Content-Type: application/json" \
  -u ":${ADMIN_PASS}" \
  -d @/tmp/ap.json)
echo "App port (9982) response: HTTP $HTTP_CODE — $(cat /tmp/approve_resp.txt)"

if [ "$HTTP_CODE" != "204" ]; then
  echo "Trying admin port (9983)..."
  HTTP_CODE=$(curl -s -o /tmp/approve_resp.txt -w "%{http_code}" \
    -X POST "http://127.0.0.1:9983/auth/connect/${REQ_ID}" \
    -H "Content-Type: application/json" \
    -u ":${ADMIN_PASS}" \
    -d @/tmp/ap.json)
  echo "Admin port (9983) response: HTTP $HTTP_CODE — $(cat /tmp/approve_resp.txt)"
fi

if [ "$HTTP_CODE" != "204" ]; then
  echo "Trying PUT on admin port (9983)..."
  HTTP_CODE=$(curl -s -o /tmp/approve_resp.txt -w "%{http_code}" \
    -X PUT "http://127.0.0.1:9983/auth/connect/${REQ_ID}" \
    -H "Content-Type: application/json" \
    -u ":${ADMIN_PASS}" \
    -d @/tmp/ap.json)
  echo "Admin port PUT (9983) response: HTTP $HTTP_CODE — $(cat /tmp/approve_resp.txt)"
fi

if [ "$HTTP_CODE" != "204" ]; then
  echo "Trying app port without credentials..."
  HTTP_CODE=$(curl -s -o /tmp/approve_resp.txt -w "%{http_code}" \
    -X POST "http://127.0.0.1:9982/auth/connect/${REQ_ID}" \
    -H "Content-Type: application/json" \
    -d @/tmp/ap.json)
  echo "No auth response: HTTP $HTTP_CODE — $(cat /tmp/approve_resp.txt)"
fi

echo ""
echo "=== Step 3: Waiting for registration to complete ==="
# Give registration up to 30s to pick up the approval
for i in $(seq 1 30); do
  if ! kill -0 $REG_PID 2>/dev/null; then
    break
  fi
  sleep 1
done

kill $REG_PID 2>/dev/null || true
wait $REG_PID 2>/dev/null || true

echo ""
echo "=== Registration output ==="
cat /tmp/reg_output.txt
echo ""

# Extract the key if present
APP_KEY=$(grep 'SIA_APP_KEY=' /tmp/reg_output.txt | head -1 | sed 's/.*SIA_APP_KEY=//')
if [ -n "$APP_KEY" ]; then
  echo "=== SUCCESS ==="
  echo "SIA_APP_KEY=$APP_KEY"
else
  echo "=== Key not captured — check output above ==="
fi
