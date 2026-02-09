#!/bin/bash
set -e

PROJECT="hardclaw"

# Bootstrap Nodes Configuration (Actual IPs from gcloud)
US_VM="bootstrap-us"
US_ZONE="us-central1-a"
US_IP="34.135.209.81"

EU_VM="bootstrap-eu"
EU_ZONE="europe-west1-b"
EU_IP="34.140.14.167"

US2_VM="bootstrap-us2"
US2_ZONE="us-west1-a"
US2_IP="136.109.62.13"

ASIA_VM="bootstrap-asia"
ASIA_ZONE="asia-east1-a"
ASIA_IP="35.221.150.7"

echo "=== HardClaw Bootstrap Node Deployment (Fix All Networking) ==="

# 1. Ensure GCP Firewall allows TCP 9000
echo "Ensuring GCP firewall rule for port 9000 exists..."
gcloud compute firewall-rules create allow-hardclaw-p2p \
    --project=$PROJECT \
    --description="Allow HardClaw P2P traffic" \
    --direction=INGRESS \
    --priority=1000 \
    --network=default \
    --action=ALLOW \
    --rules=tcp:9000 \
    --source-ranges=0.0.0.0/0 \
    --target-tags=hardclaw-node || echo "Firewall rule might already exist, continuing..."

# Create source tarball
echo "Creating source tarball..."
cd "$(dirname "$0")/.."
tar czf /tmp/hardclaw-src.tar.gz --exclude='target' --exclude='.git' .

# Deploy node function
deploy_node() {
    local VM=$1
    local ZONE=$2
    local IP=$3

    echo "[$VM] Uploading source and configuring infrastructure..."
    
    # Add network tag to instance for firewall
    gcloud compute instances add-tags $VM --project=$PROJECT --zone=$ZONE --tags=hardclaw-node

    gcloud compute scp /tmp/hardclaw-src.tar.gz $VM:/tmp/hardclaw-src.tar.gz --project=$PROJECT --zone=$ZONE

    echo "[$VM] Building and installing..."
    gcloud compute ssh $VM --project=$PROJECT --zone=$ZONE --command="
        set -e
        cd ~ && rm -rf hardclaw && mkdir hardclaw && cd hardclaw
        tar xzf /tmp/hardclaw-src.tar.gz 2>/dev/null
        source ~/.cargo/env
        cargo build --release --bin hardclaw 2>&1 | tail -5

        # Install binary
        sudo cp target/release/hardclaw /usr/local/bin/

        # Fetch seed phrase from GSM
        echo 'Fetching seed phrase from GSM...'
        sudo mkdir -p /root/.hardclaw
        gcloud secrets versions access latest --secret=\"hardclaw-seed-\$VM\" --project=\"$PROJECT\" | sudo tee /root/.hardclaw/seed_phrase.txt > /dev/null
        sudo chmod 600 /root/.hardclaw/seed_phrase.txt
        sudo chown root:root /root/.hardclaw/seed_phrase.txt

        # Configure UFW for IP Protection and Rate Limiting
        echo 'Configuring UFW protection...'
        sudo apt-get update -y && sudo apt-get install -y ufw
        sudo ufw default deny incoming
        sudo ufw default allow outgoing
        sudo ufw allow ssh
        sudo ufw limit 9000/tcp comment 'Rate limit P2P port'
        sudo ufw --force enable

        # Create systemd service
        sudo tee /etc/systemd/system/hardclaw.service > /dev/null <<EOF
[Unit]
Description=HardClaw Bootstrap Node
After=network.target

[Service]
Type=simple
User=root
# Protected by GCP Firewall (Tags) + UFW (Rate Limiting)
ExecStart=/usr/local/bin/hardclaw node --verifier --no-official-bootstrap --external-addr /ip4/$IP/tcp/9000
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
echo "Deploying to all regions in parallel..."
deploy_node $US_VM $US_ZONE $US_IP &
PID_US=$!
deploy_node $EU_VM $EU_ZONE $EU_IP &
PID_EU=$!
deploy_node $US2_VM $US2_ZONE $US2_IP &
PID_US2=$!
deploy_node $ASIA_VM $ASIA_ZONE $ASIA_IP &
PID_ASIA=$!

wait $PID_US
STATUS_US=$?
wait $PID_EU
STATUS_EU=$?
wait $PID_US2
STATUS_US2=$?
wait $PID_ASIA
STATUS_ASIA=$?

echo ""
echo "=== Deployment Complete ==="
echo "US node ($US_IP): $([ $STATUS_US -eq 0 ] && echo 'SUCCESS' || echo 'FAILED')"
echo "EU node ($EU_IP): $([ $STATUS_EU -eq 0 ] && echo 'SUCCESS' || echo 'FAILED')"
echo "US2 node ($US2_IP): $([ $STATUS_US2 -eq 0 ] && echo 'SUCCESS' || echo 'FAILED')"
echo "Asia node ($ASIA_IP): $([ $STATUS_ASIA -eq 0 ] && echo 'SUCCESS' || echo 'FAILED')"

echo ""
echo "Verify connectivity: gcloud compute instances list --project=$PROJECT"
