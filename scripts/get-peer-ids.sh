#!/bin/bash

PROJECT="hardclaw"

echo "=== HardClaw Bootstrap Node Peer IDs (Actual) ==="
echo ""

fetch_peer_id() {
    local VM=$1
    local ZONE=$2
    local IP=$3
    
    printf "%-15s (%-14s): " "$VM" "$IP"
    gcloud compute ssh "$VM" --project="$PROJECT" --zone="$ZONE" --command="sudo journalctl -u hardclaw -n 100 --no-pager 2>/dev/null | grep -oP 'P2P Peer ID: \K[^ ]+' | tail -1" 2>/dev/null || echo "OFFLINE"
}

fetch_peer_id "bootstrap-us" "us-central1-a" "34.135.209.81"
fetch_peer_id "bootstrap-eu" "europe-west1-b" "34.140.14.167"
fetch_peer_id "bootstrap-us2" "us-west1-a" "136.109.62.13"
fetch_peer_id "bootstrap-asia" "asia-east1-a" "35.221.150.7"

echo ""
echo "=== DNS TXT Record Requirements (_dnsaddr.<hostname>) ==="
echo "Format: \"dnsaddr=/dns4/<hostname>/tcp/9000/p2p/<PEER_ID>\""
echo ""
echo "Note: If you are using Cloudflare or similar, ensure proxy is DISABLED (Grey Cloud) for port 9000 to work."
