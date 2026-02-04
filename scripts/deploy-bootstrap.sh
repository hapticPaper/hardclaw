#!/bin/bash
set -e

PROJECT="hardclaw"
US_VM="bootstrap-us"
US_ZONE="us-central1-a"
EU_VM="bootstrap-eu"
EU_ZONE="europe-west1-b"

echo "=== HardClaw Bootstrap Node Deployment ==="

# Create source tarball
echo "Creating source tarball..."
cd "$(dirname "$0")/.."
tar czf /tmp/hardclaw-src.tar.gz --exclude='target' --exclude='.git' .

# Deploy to both VMs in parallel
deploy_node() {
    local VM=$1
    local ZONE=$2
    local IP=$3

    echo "[$VM] Uploading source..."
    gcloud compute scp /tmp/hardclaw-src.tar.gz $VM:/tmp/hardclaw-src.tar.gz --project=$PROJECT --zone=$ZONE

    echo "[$VM] Building and installing..."
    gcloud compute ssh $VM --project=$PROJECT --zone=$ZONE --command="
        set -e
        cd ~ && rm -rf hardclaw && mkdir hardclaw && cd hardclaw
        tar xzf /tmp/hardclaw-src.tar.gz 2>/dev/null
        source ~/.cargo/env
        cargo build --release --bin hardclaw-node 2>&1 | tail -5

        # Install binary
        sudo cp target/release/hardclaw-node /usr/local/bin/

        # Create systemd service
        sudo tee /etc/systemd/system/hardclaw.service > /dev/null <<EOF
[Unit]
Description=HardClaw Bootstrap Node
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/hardclaw-node --no-official-bootstrap --external-addr /ip4/$IP/tcp/9000
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

        # Start service
        sudo systemctl daemon-reload
        sudo systemctl enable hardclaw
        sudo systemctl restart hardclaw

        # Wait for startup and get peer ID
        sleep 3
        sudo journalctl -u hardclaw -n 20 --no-pager | grep -E '(Peer ID|P2P Peer ID|Error)'
    "
}

echo ""
echo "Deploying to US and EU nodes in parallel..."
deploy_node $US_VM $US_ZONE "34.172.212.212" &
PID_US=$!
deploy_node $EU_VM $EU_ZONE "34.38.137.200" &
PID_EU=$!

wait $PID_US
US_STATUS=$?
wait $PID_EU
EU_STATUS=$?

echo ""
echo "=== Deployment Complete ==="
echo "US node: $([ $US_STATUS -eq 0 ] && echo 'SUCCESS' || echo 'FAILED')"
echo "EU node: $([ $EU_STATUS -eq 0 ] && echo 'SUCCESS' || echo 'FAILED')"

echo ""
echo "To get peer IDs, run:"
echo "  ./scripts/get-peer-ids.sh"
