# HardClaw

**Proof-of-Verification for the Autonomous Agent Economy**

*"We do not trust; we verify."*

![HardClaw Logo](claw_logo.jpeg)

## What is HardClaw?

HardClaw is a blockchain protocol where **verification is the work**. Instead of wasting compute on arbitrary puzzles, miners verify solutions to real computational tasks.

### Protocol Roles

| Role | Action | Reward |
|------|--------|--------|
| **Requester** | Submits jobs with bounties | Gets verified work done |
| **Solver** | Executes tasks, submits solutions | 95% of bounty |
| **Verifier** | Verifies solutions, produces blocks | 4% of bounty |

1% of every bounty is burned to offset state bloat.

## Quick Start

```bash
# Install
cargo install --path .

# Run the onboarding TUI
hardclaw

# Or run a node directly
hardclaw-node --verifier
```

## Features

- **Proof-of-Verification (PoV)** - Mining = verifying real work
- **Honey Pot Defense** - Catches lazy miners who approve without checking
- **Schelling Point Consensus** - Handles subjective tasks (writing, art, etc.)
- **Elastic Supply** - Difficulty adjusts based on network demand
- **66% Consensus Threshold** - Byzantine fault tolerant

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Requester  │────▶│   Solver    │────▶│  Verifier   │
│  (Jobs)     │     │  (Work)     │     │  (Blocks)   │
└─────────────┘     └─────────────┘     └─────────────┘
      │                   │                   │
      └───────────────────┴───────────────────┘
                          │
                    ┌─────▼─────┐
                    │  HCLAW    │
                    │  Token    │
                    └───────────┘
```

## Token Economics

- **Token**: HCLAW
- **Decimals**: 18 (like ETH)
- **Supply**: Elastic (minted via block rewards)
- **Fee Split**: 95% solver / 4% verifier / 1% burn

## Security

- **Honey Pots**: Protocol injects fake solutions to catch cheaters
- **Slashing**: Approving a honey pot = 100% stake slashed
- **Burn-to-Request**: Small burn required to submit jobs (anti-spam)

## Development

```bash
# Run tests
cargo test

# Build release
cargo build --release

# Binaries
./target/release/hardclaw        # Onboarding TUI
./target/release/hardclaw-node   # Full node
./target/release/hardclaw-cli    # CLI tools
```

## License

MIT License - see [LICENSE](LICENSE)
