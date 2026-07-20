#!/bin/bash
# approve_connection.sh <request_id>
# Simulates clicking "Approve" on the indexd connection approval page — no browser needed.
# The page JS does: Authorization: Basic btoa(":" + password)
# which is standard HTTP Basic auth with empty username and admin password.

REQ_ID="${1:?Usage: $0 <request_id>}"
ADMIN_PASS="sia_api_password_core_sync"
B64=$(printf ':%s' "$ADMIN_PASS" | base64 -w0)

printf '{"approve":true}' > /tmp/ap.json

echo "==> Approving connection request: $REQ_ID"
HTTP_STATUS=$(curl -s -o /tmp/approve_resp.txt -w "%{http_code}" \
  -X POST "http://127.0.0.1:9982/auth/connect/${REQ_ID}" \
  -H "Content-Type: application/json" \
  -H "Authorization: Basic ${B64}" \
  -d @/tmp/ap.json)

echo "HTTP $HTTP_STATUS"
cat /tmp/approve_resp.txt
echo ""

if [ "$HTTP_STATUS" = "204" ]; then
  echo "==> SUCCESS — connection approved"
else
  echo "==> FAILED (HTTP $HTTP_STATUS)"
  exit 1
fi
