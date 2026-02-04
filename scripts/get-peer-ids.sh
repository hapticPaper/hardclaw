#!/bin/bash

PROJECT="hardclaw"

echo "=== Bootstrap Node Peer IDs ==="
echo ""

echo "US Node (34.172.212.212):"
gcloud compute ssh bootstrap-us --project=$PROJECT --zone=us-central1-a --command="sudo journalctl -u hardclaw -n 50 --no-pager 2>/dev/null | grep -oP 'P2P Peer ID: \K[^ ]+' | tail -1" 2>/dev/null

echo ""
echo "EU Node (34.38.137.200):"
gcloud compute ssh bootstrap-eu --project=$PROJECT --zone=europe-west1-b --command="sudo journalctl -u hardclaw -n 50 --no-pager 2>/dev/null | grep -oP 'P2P Peer ID: \K[^ ]+' | tail -1" 2>/dev/null

echo ""
echo "Update BOOTSTRAP_NODES in src/network/mod.rs with:"
echo '"/ip4/34.172.212.212/tcp/9000/p2p/<US_PEER_ID>",'
echo '"/ip4/34.38.137.200/tcp/9000/p2p/<EU_PEER_ID>",'
