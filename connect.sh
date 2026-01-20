#!/bin/bash
# Connect to Mac via Cloudflare Tunnel
# Usage: ./connect.sh [username]

set -e

# Configuration
HOSTNAME="tcp.andreisuslov.com"
LOCAL_PORT="${CF_LOCAL_PORT:-2222}"
USERNAME="${1:-ansuslov}"

# Check for required environment variables
if [ -z "$CF_ACCESS_CLIENT_ID" ] || [ -z "$CF_ACCESS_CLIENT_SECRET" ]; then
    echo "Error: Missing environment variables"
    echo ""
    echo "Set these before running:"
    echo "  export CF_ACCESS_CLIENT_ID='your-client-id'"
    echo "  export CF_ACCESS_CLIENT_SECRET='your-client-secret'"
    echo ""
    echo "Or create a .env file and source it:"
    echo "  source .env && ./connect.sh"
    exit 1
fi

# Check if cloudflared is installed
if ! command -v cloudflared &> /dev/null; then
    echo "Error: cloudflared not found"
    echo ""
    echo "Install it:"
    echo "  macOS:  brew install cloudflared"
    echo "  Linux:  curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 -o cloudflared && chmod +x cloudflared"
    exit 1
fi

# Kill any existing tunnel on this port
pkill -f "cloudflared access tcp.*--url localhost:$LOCAL_PORT" 2>/dev/null || true

echo "Starting Cloudflare tunnel to $HOSTNAME..."
cloudflared access tcp \
    --hostname "$HOSTNAME" \
    --url "localhost:$LOCAL_PORT" \
    --id "$CF_ACCESS_CLIENT_ID" \
    --secret "$CF_ACCESS_CLIENT_SECRET" &

TUNNEL_PID=$!

# Wait for tunnel to be ready
sleep 2

# Check if tunnel is running
if ! kill -0 $TUNNEL_PID 2>/dev/null; then
    echo "Error: Tunnel failed to start"
    exit 1
fi

echo "Tunnel running (PID: $TUNNEL_PID)"
echo ""
echo "Connecting via SSH..."
echo "---"

# Connect via SSH
ssh -o StrictHostKeyChecking=no -p "$LOCAL_PORT" "$USERNAME@localhost"

# Cleanup
echo ""
echo "Closing tunnel..."
kill $TUNNEL_PID 2>/dev/null || true
