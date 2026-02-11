#!/bin/bash
set -e

# HardClaw Node Setup Script (binary-only)
# Uploaded and executed on the VM by deploy-bootstrap.sh
# Expects /tmp/hardclaw (binary) and /tmp/hardclaw.toml to exist

VM_NAME=$1
PROJECT_ID=$2
IP_ADDR=$3

if [ -z "$VM_NAME" ] || [ -z "$PROJECT_ID" ] || [ -z "$IP_ADDR" ]; then
    echo "Usage: setup-node.sh <VM_NAME> <PROJECT_ID> <IP_ADDR>"
    exit 1
fi

# 1. Install binary
echo "Installing binary..."
sudo mv /tmp/hardclaw /usr/local/bin/hardclaw
sudo chmod +x /usr/local/bin/hardclaw

# 2. Configure node
echo "Configuring node..."
sudo mkdir -p /root/.hardclaw
if [ -f "/tmp/hardclaw.toml" ]; then
    sudo mv /tmp/hardclaw.toml /root/.hardclaw/hardclaw.toml
else
    echo "WARNING: /tmp/hardclaw.toml not found!"
fi

# 3. Fetch seed phrase from GSM
echo "Fetching seed phrase for $VM_NAME from GSM..."
gcloud secrets versions access latest --secret="hardclaw-seed-$VM_NAME" --project="$PROJECT_ID" \
    | sudo tee /root/.hardclaw/seed_phrase.txt > /dev/null
sudo chmod 600 /root/.hardclaw/seed_phrase.txt
sudo chown root:root /root/.hardclaw/seed_phrase.txt

# 4. Firewall (UFW)
echo "Configuring UFW..."
sudo apt-get update -qq && sudo apt-get install -y -qq ufw
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow ssh
sudo ufw limit 9000/tcp comment 'Rate limit P2P port'
sudo ufw --force enable

# 5. Systemd service
echo "Creating systemd service..."
sudo tee /etc/systemd/system/hardclaw.service > /dev/null <<EOF
[Unit]
Description=HardClaw Bootstrap Node
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/hardclaw node --verifier --no-official-bootstrap --genesis /root/.hardclaw/hardclaw.toml --external-addr /ip4/$IP_ADDR/tcp/9000
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

echo "Starting service..."
sudo systemctl daemon-reload
sudo systemctl enable hardclaw
sudo systemctl restart hardclaw

# 6. Clean up any leftover source/toolchain from previous deploys
rm -rf ~/hardclaw ~/.cargo ~/.rustup /tmp/hardclaw-src.tar.gz

# 7. Verify
sleep 3
sudo journalctl -u hardclaw -n 20 --no-pager | grep -E '(Peer ID|P2P Peer ID|Error)'
