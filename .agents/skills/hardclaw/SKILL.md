---
description: HardClaw Bootstrap Nodes and Genesis Configuration
---

# HardClaw Bootstrap & Genesis Configuration

## Bootstrap Nodes

### Infrastructure
The network has 4 official bootstrap nodes deployed on GCP:

| Node | GCP Zone | Peer ID |
|------|----------|---------|
| `bootstrap-us` | `us-central1-a` | `12D3KooWMCnTqJNYUv1pHovFzaFounwnQqmBAZ47Ey3jGVpMYbhp` |
| `bootstrap-eu` | `europe-west1-b` | `12D3KooWKyBAAdTyv7XgkZQdhF2R9ubm5pikuzpkLLaaqckfnkS2` |
| `bootstrap-us2` | `us-west1-a` | `12D3KooWJmqBYTbRcmatZwEfswwMyPPM8guDC7xRxhbik78gZiZT` |
| `bootstrap-asia` | `asia-east1-a` | `12D3KooWC8HTSMhSr6tL1PTVUfrSp2Pk4eXDriVgRFSpGwvd8rCU` |

### Critical: Wallet Preservation

**⚠️ IMPORTANT**: Each bootstrap node has a **pre-existing wallet** 

- **Peer IDs are derived from these wallets**
- **NEVER delete or regenerate these wallets**
- **NEVER change the seed phrases**
- When redeploying, the binary must load the existing wallet to maintain the same peer ID

### Deployment

Bootstrap nodes run as systemd services:
```bash
# Service location
/etc/systemd/system/hardclaw.service

# Command
/usr/local/bin/hardclaw node --verifier --no-official-bootstrap --external-addr /ip4/$IP/tcp/9000
```

DNS resolution via `dnsaddr` TXT records at `_dnsaddr.<hostname>.clawpaper.com`.

## Genesis Behavior

### Genesis Configuration

The genesis block bootstraps the network with:

1.  **Pre-approved Verifiers**:
    -   Addresses defined in the genesis `TOML` config (loaded from `genesis.toml`).
    -   **Benefit**: They skip the initial "competency challenge" required for new nodes.
    -   **Stake**: They receive an initial `airdrop_amount` (100 HCLAW) and a vesting schedule, giving them immediate `voting_power` to secure the network.
    -   *Note*: They do NOT have special protocol-level authority beyond their stake.

2.  **Authority Key**:
    -   A specific public key hardcoded in the genesis config.
    -   **Purpose**: Used **only** for the "DNS Break-Glass" mechanism.
    -   **Function**: Signs DNS TXT records to authorize emergency bootstrap nodes if the network stalls. It does *not* sign blocks or override consensus.
Genesis configuration is typically stored in the codebase and includes:
- Initial validator set (public keys)
- Initial token distribution
- Network parameters (block time, etc.)
- Bootstrap node addresses

**Location**: Check `src/genesis.rs` or similar genesis configuration files for the authoritative genesis spec.

## Key Management

### Bootstrap Wallets
- **Source of Truth**: Google Secret Manager (GSM)
- **Local Location**: `~/.hardclaw/seed_phrase.txt` on each node
- **Usage**: Pulled from GSM during deployment/provisioning
- **Critical**: 
  - **MUST BE BACKED UP** in GSM
  - Losing these means losing the bootstrap identity and peer ID
  - Peer ID derivation is deterministic from the seed phrase

### Security
- Seed phrases should be stored in GSM, NOT in git
- Access to bootstrap nodes should be restricted