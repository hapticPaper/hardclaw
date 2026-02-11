#!/bin/bash
set -e

PROJECT="hardclaw"
REPO="hapticPaper/hardclaw"

# Bootstrap Nodes Configuration (static IPs from gcloud)
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

echo "=== HardClaw Bootstrap Node Deployment (Binary-Only) ==="

# Resolve binary: local path or latest GitHub release
BINARY_PATH="${1:-}"
if [ -n "$BINARY_PATH" ] && [ -f "$BINARY_PATH" ]; then
    echo "Using local binary: $BINARY_PATH"
else
    echo "Downloading latest linux-x86_64 binary from GitHub releases..."
    BINARY_PATH="/tmp/hardclaw-release-binary"
    gh release download --repo "$REPO" --pattern "hardclaw-linux-x86_64.tar.gz" --output /tmp/hardclaw-linux-x86_64.tar.gz --clobber
    tar -xzf /tmp/hardclaw-linux-x86_64.tar.gz -C /tmp
    mv /tmp/hardclaw "$BINARY_PATH"
    rm -f /tmp/hardclaw-linux-x86_64.tar.gz
    echo "Downloaded release binary to $BINARY_PATH"
fi

chmod +x "$BINARY_PATH"

cd "$(dirname "$0")/.."

# Ensure GCP firewall allows TCP 9000
echo "Ensuring GCP firewall rule for port 9000..."
gcloud compute firewall-rules create allow-hardclaw-p2p \
    --project=$PROJECT \
    --description="Allow HardClaw P2P traffic" \
    --direction=INGRESS \
    --priority=1000 \
    --network=default \
    --action=ALLOW \
    --rules=tcp:9000 \
    --source-ranges=0.0.0.0/0 \
    --target-tags=hardclaw-node 2>/dev/null || true

# Deploy node function — binary only, no source code
deploy_node() {
    local VM=$1
    local ZONE=$2
    local IP=$3

    echo "[$VM] Uploading binary, config, and setup script..."

    gcloud compute instances add-tags $VM --project=$PROJECT --zone=$ZONE --tags=hardclaw-node

    gcloud compute scp "$BINARY_PATH" $VM:/tmp/hardclaw --project=$PROJECT --zone=$ZONE
    gcloud compute scp hardclaw.toml $VM:/tmp/hardclaw.toml --project=$PROJECT --zone=$ZONE
    gcloud compute scp scripts/setup-node.sh $VM:/tmp/setup-node.sh --project=$PROJECT --zone=$ZONE

    echo "[$VM] Executing setup script..."
    gcloud compute ssh $VM --project=$PROJECT --zone=$ZONE --command="bash /tmp/setup-node.sh $VM $PROJECT $IP"
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
echo "Verify: gcloud compute instances list --project=$PROJECT"
