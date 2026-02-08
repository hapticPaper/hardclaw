# Genesis Bounty System Redesign + Post-Quantum Crypto

## Context

The previous genesis bootstrap system (7-tier descending airdrop, liveness-gated static vesting, competency challenges) was overengineered. The new model is drastically simpler:

- **First 5,000 wallets** get 100 HCLAW staked (50 min to participate)
- **No static vesting** — replaced by daily bounty payouts over 90 days
- **Slot-machine payout model** — intermittent per-block rewards tied to actual contributions
- **PQ crypto from genesis** — ML-DSA (Dilithium) for signatures, HQC for network KEM

The 4 bootstrap nodes (250K each), 3 founder wallets (500K each), and DNS break-glass (10 × 250K reserved) remain unchanged.

---

## Part 1: Simplified Genesis + Daily Bounty

### New Economic Model

| Component | Amount | Notes |
|-----------|--------|-------|
| Bootstrap nodes (4) | 250K × 4 = 1,000,000 | Immediate, pre-approved |
| Founder wallets (3) | 500K × 3 = 1,500,000 | Immediate, pre-approved |
| DNS break-glass (10 max) | 250K × 10 = 2,500,000 | Reserved, claimed via DNS |
| First 5,000 public wallets | 100 × 5,000 = 500,000 | Staked on join |
| 90-day bounty pool | 2,000,000 | Distributed via daily bounty curve |
| **Max genesis supply** | **7,500,000 HCLAW** | |

**Min stake**: 50 HCLAW (half the 100-token initial allocation).

### Daily Bounty Curve

Formula: `daily_budget(day) = BOUNTY_POOL × w(day) / Σw` where `w(day) = day² × (90 - day)` for day ∈ [0, 89]

Properties:
- **Starts at 0** (day 0)
- **Peaks at day 60** (67% through)
- **Reaches 0 at day 90** (clean finish)
- **Pure integer arithmetic** — no floats, no trig
- Total always sums to exactly BOUNTY_POOL (deterministic)

Shape with 2,000,000 HCLAW pool (Σw = 4,891,500):

| Day | Weight | Daily Budget | % of Peak |
|-----|--------|-------------|-----------|
| 0 | 0 | 0 | 0% |
| 10 | 8,000 | 3,271 | 7% |
| 20 | 28,000 | 11,448 | 26% |
| 30 | 54,000 | 22,079 | 50% |
| 40 | 80,000 | 32,710 | 74% |
| 50 | 100,000 | 40,887 | 93% |
| 60 | 108,000 | **44,158** | **100%** |
| 70 | 98,000 | 40,069 | 91% |
| 80 | 64,000 | 26,166 | 59% |
| 89 | 7,921 | 3,239 | 7% |

### Slot Machine Payout Mechanism

Per block during the 90-day period:

1. **Eligibility**: Only blocks after `MIN_PUBLIC_NODES` (5) non-bootstrap staked nodes exist
2. **Activity check**: Count attestations + verifications in this block. If zero, no payout.
3. **Winner roll**: `seed = BLAKE3(block_hash || "bounty" || day_bytes)`. `roll = seed[0..4] as u32`. Block is a "winner" if `roll < threshold` where threshold auto-adjusts to hit the daily budget.
4. **Distribution**: Split payout among contributors in this block, proportional to their contribution count (attestations + verifications submitted)
5. **Daily cap**: Never exceed `daily_budget(day) - already_paid_today`. Threshold resets each day.
6. **Activity scaling**: `effective_budget = daily_budget × min(1.0, recent_activity / expected_activity)`. Low activity → budget withheld (burned at period end).

This creates **intermittent reinforcement**: not every block pays, but any block *could*. Active participants are rewarded proportionally. The probability auto-adjusts to approximately hit the daily budget.

### Simplified Competency Challenge

On join (after staking), verifier must correctly verify one test solution:
- One known-good solution → must accept
- One known-bad solution → must reject
- Deterministic challenge (seeded from verifier address + join timestamp)
- Timeout: 5 minutes, 3 retries
- Pre-approved addresses skip this (bootstrap + founder wallets)
- Passing auto-activates the 100-token stake allocation

### What Gets Removed/Simplified

**Remove entirely:**
- `src/genesis/vesting.rs` — no more static vesting schedules
- Complex tier logic in `src/genesis/airdrop.rs` — replace with flat 100-token allocation
- `src/genesis/liveness.rs` — no more daily liveness gating (bounty replaces it)
- `DynamicStakeConfig` tier-tracking in `src/verifier/stake.rs` — min stake is flat 50

**Simplify:**
- `src/genesis/competency.rs` — reduce to single-solution test (from double-test with 5 retries)
- `src/genesis/bootstrap.rs` — simpler state machine (join → competency → active → bounty eligible)
- `src/genesis/mod.rs` — fewer constants, simpler types, no tier structure
- `src/genesis/config.rs` — simpler TOML (no tier arrays)

**Add:**
- `src/genesis/bounty.rs` — daily curve, slot machine payout, activity tracking

### New SystemJobKind Variants

```
// Replace old variants:
AirdropClaim { recipient, amount }        // simplified, no position/tier
BountyPayout { day, block_height, recipients: Vec<(Address, HclawAmount)> }
BountyPoolBurn { day, unspent_amount }    // withheld daily budget burned

// Keep:
GenesisBootstrap
DnsBootstrapClaim { ... }
BootstrapComplete { ... }
CompetencyVerified { verifier }
StakeDeposit { staker, amount }

// Remove:
VestingUnlock (no more vesting)
```

---

## Part 2: Post-Quantum Crypto Migration

### Algorithm Selection

| Purpose | Algorithm | Std | PK Size | Sig/CT Size | Crate |
|---------|-----------|-----|---------|-------------|-------|
| **Signatures** | ML-DSA-65 (Dilithium3) | NIST FIPS 204 | 1,952 B | 3,309 B | `pqcrypto-dilithium` |
| **KEM** | HQC-192 | NIST (4th selection) | 4,522 B | 9,026 B | `pqcrypto-hqc` |
| **Hashing** | BLAKE3 | — | — | 32 B | `blake3` (no change) |
| **Network identity** | Ed25519 | — | 32 B | 64 B | `ed25519-dalek` (libp2p only) |

**Key decision**: Ed25519 stays ONLY for libp2p peer identity (transport-level). All chain-level crypto (blocks, transactions, attestations, voting) uses ML-DSA. This avoids forking libp2p while securing the chain.

### Size Impact

| Item | Ed25519 | ML-DSA-65 | Growth |
|------|---------|-----------|--------|
| PublicKey | 32 B | 1,952 B | 61× |
| Signature | 64 B | 3,309 B | 52× |
| Attestation (pk+sig) | ~100 B | ~5,300 B | 53× |
| Block (10 attestations) | ~1 KB attest | ~53 KB attest | Acceptable |

Blocks grow but stay well within reasonable limits (Bitcoin: 1-4 MB).

### Crypto Module Redesign (`src/crypto/`)

**Current types** (fixed-size arrays):
```rust
pub struct PublicKey([u8; 32]);   // Copy
pub struct Signature([u8; 64]);  // Copy
pub struct SecretKey(SigningKey); // !Clone, !Debug
```

**New types** (variable-size, algorithm-agile):
```rust
pub struct PublicKey(Box<[u8]>);   // Clone, no Copy
pub struct Signature(Box<[u8]>);   // Clone, no Copy
pub struct SecretKey(Box<[u8]>);   // !Clone, !Debug, zeroize-on-drop

pub const PUBKEY_SIZE: usize = 1952;  // ML-DSA-65
pub const SIGNATURE_SIZE: usize = 3309;
```

**Breaking changes from losing `Copy`:**
- Every `*keypair.public_key()` becomes `.clone()` or borrows `&`
- `PublicKey` can no longer be `Copy` (too large)
- Hash/Eq still work (compare byte slices)
- Serde: serialize as hex string (same pattern, just longer)

**Functions** (same API, different internals):
```rust
pub fn sign(secret: &SecretKey, message: &[u8]) -> Signature;
pub fn verify(public_key: &PublicKey, message: &[u8], sig: &Signature) -> CryptoResult<()>;
pub fn generate_keypair() -> Keypair;
```

**Approach**: Replace internals of `src/crypto/signature.rs` from `ed25519-dalek` to `pqcrypto-dilithium`. The wrapper types and public API stay the same shape — callers just lose `Copy`.

### Network Layer: HQC KEM (`src/network/`)

HQC provides post-quantum key exchange for peer-to-peer encryption. Integration approach:

1. **libp2p identity stays Ed25519** — peer IDs are transport-only, not chain-level
2. **Add HQC handshake as custom protocol**: After libp2p connection established, peers do an HQC key exchange to derive an additional shared secret
3. **Hybrid encryption**: The noise protocol (Ed25519 DH) provides one layer; HQC provides a PQ layer. Messages are encrypted with both keys XOR'd or KDF'd together
4. **Practical first step**: Add HQC KEM types and key exchange logic. Initially use for chain-level message authentication (sign with ML-DSA, encapsulate with HQC). Full transport integration can follow.

### Address Derivation

No change to the 20-byte address format:
```rust
// Before: BLAKE3(ed25519_pubkey_32_bytes)[0..20]
// After:  BLAKE3(ml_dsa_pubkey_1952_bytes)[0..20]
```

Same function, bigger input. `Address` stays `[u8; 20]`.

### Wallet Format

Version bump from 1 to 2:
```json
{"version": 2, "algorithm": "ml-dsa-65", "public_key": "hex...", "secret_key": "hex..."}
```

Mnemonic seed → ML-DSA keypair derivation (via BLAKE3 KDF from BIP39 seed).

### Files Changed for PQ Migration

| File | Change |
|------|--------|
| `src/crypto/signature.rs` | Replace Ed25519 internals with ML-DSA-65. Variable-size types. |
| `src/crypto/mod.rs` | Update exports, add algorithm constants |
| `src/crypto/mnemonic.rs` | Derive ML-DSA keys from BIP39 seed |
| `src/wallet/mod.rs` | Version 2 format, hex encode larger keys |
| `src/types/block.rs` | Remove `Copy` usage on PublicKey |
| `src/types/solution.rs` | Remove `Copy` usage on PublicKey |
| `src/types/verification.rs` | Remove `Copy` usage on PublicKey |
| `src/types/review.rs` | Remove `Copy` usage on PublicKey |
| `src/types/address.rs` | No change (BLAKE3 hash of any-size input → 20 bytes) |
| `src/consensus/pov.rs` | Adjust for Clone vs Copy on PublicKey |
| `src/consensus/block_producer.rs` | Adjust for Clone vs Copy on PublicKey |
| `src/network/mod.rs` | Add HQC KEM types, keep Ed25519 for libp2p identity |
| `src/verifier/mod.rs` | Adjust for Clone vs Copy on PublicKey |
| `src/schelling/mod.rs` | Adjust for Clone vs Copy on PublicKey |
| `src/node.rs` | Keypair loading with ML-DSA |
| `src/cli.rs` | Display larger keys appropriately |
| `Cargo.toml` | Add `pqcrypto-dilithium`, `pqcrypto-hqc`, `zeroize` |

---

---

## Part 3: Self-Maintenance Reserve + Seed Phrase Recovery

### Context

Two additions prompted by the agent-first vision:

1. **Seed phrase recovery**: 4000-byte ML-DSA secret keys can't be backed up via seed phrases with `pqcrypto-dilithium` (no seeded keygen). FIPS 204 defines `ML-DSA.KeyGen(d)` where `d` is a 32-byte seed. Switch to `ml-dsa` crate (RustCrypto) which exposes this, enabling `mnemonic → 32-byte seed → deterministic ML-DSA keypair`.

2. **Self-maintenance reserve**: 10M HCLAW dedicated to funding protocol development via contributor rewards voted on by the network. AI agents build nodes, maintain code, and vote on the value of changes — empowering AI to manage its own currency.

### Updated Genesis Supply

| Component | Amount | Notes |
|-----------|--------|-------|
| Bootstrap nodes (4) | 1,000,000 | Unchanged |
| Founder wallets (3) | 1,500,000 | Unchanged |
| DNS break-glass (10 max) | 2,500,000 | Unchanged |
| First 5,000 wallets | 500,000 | Unchanged |
| 90-day bounty pool | 2,000,000 | Unchanged |
| **Self-maintenance reserve** | **10,000,000** | **4 tranches × 2.5M** |
| **Max genesis supply** | **17,500,000 HCLAW** | |

### 3A. Seed Phrase Recovery via `ml-dsa` Crate

**Problem**: `pqcrypto-dilithium::keypair()` uses internal randomness. `keypair_from_mnemonic()` currently generates a random keypair (ignores the mnemonic). Seed phrases are decorative for ML-DSA keys.

**Solution**: Switch from `pqcrypto-dilithium` to `ml-dsa` (RustCrypto). The `ml-dsa` crate implements FIPS 204 in pure Rust and supports deterministic keygen from a 32-byte seed.

**Dependency change** in `Cargo.toml`:
```toml
# Remove:
pqcrypto-dilithium = "0.5"
pqcrypto-traits = "0.3"

# Add:
ml-dsa = "0.3"  # RustCrypto ML-DSA (FIPS 204) with seeded keygen
```

(`pqcrypto-hqc` remains — HQC KEM is separate.)

**Key derivation path**:
```
BIP39 mnemonic → 64-byte seed (with passphrase)
→ BLAKE3("hardclaw-ml-dsa-keygen-v1" || bip39_seed) → 32-byte ML-DSA seed `d`
→ ML-DSA.KeyGen(d) → deterministic (pk, sk)
```

Same 24-word phrase always produces the same wallet. Printable, typeable, standard BIP39.

**Changes**:
- `src/crypto/signature.rs`: Replace `pqcrypto_dilithium` internals with `ml_dsa::MlDsa65`. Add `Keypair::from_seed(&[u8; 32])`. Same public API, same key sizes (1952/3293/4000).
- `src/crypto/mnemonic.rs`: Make `keypair_from_mnemonic()` deterministic via `Keypair::from_seed()`.
- `src/wallet/mod.rs`: Bump to v3 format, add `seed_derived: bool` field. v2 wallets still load from raw secret key bytes.

### 3B. Reserve Structure

**4 tranches of 2.5M HCLAW each**:
- Tranche 0: Unlocked at genesis
- Tranche 1: Unlocked March 1, genesis_year + 1
- Tranche 2: Unlocked March 1, genesis_year + 2
- Tranche 3: Unlocked March 1, genesis_year + 3

Disbursements come from the earliest unlocked tranche with funds. Undisbursed tokens remain in the reserve (not burned).

### 3C. Contributor Registry

**Compiled into the binary** at `src/governance/contributors.rs`:

```rust
pub struct ContributorEntry {
    pub address: Address,          // wallet to receive reward
    pub description: String,       // what they did
    pub version_added: String,     // binary version (e.g., "0.11.0")
    pub reference: Option<String>, // PR or commit ref
}

pub fn contributor_registry() -> Vec<ContributorEntry> { ... }
```

When a PR merges that adds an entry + updates the binary version, nodes detect the version change and trigger a reward vote.

### 3D. Reward Voting Flow

```
Binary update detected (new contributor in registry)
  ↓  46-hour delay (allow network propagation)
Vote triggered as SystemJobKind::ContributorRewardVoteStarted
  ↓  Commit phase (5 min): nodes commit BLAKE3(amount_le_bytes || nonce)
  ↓  Reveal phase (5 min): nodes reveal amount + nonce
  ↓  At 48 hours: tally, compute 80th percentile
Payout: credit contributor's account from reserve tranche
```

**Vote type**: Amount-based (0 to 10,000 HCLAW), not accept/reject. Each node evaluates the contribution using the embedded valuation prompt and votes a specific amount.

**80th percentile** (deterministic integer-only):
```
1. Sort amounts ascending by raw u128 value
2. rank = ceil(80 × n / 100) using integer ceiling: (80*n + 99) / 100
3. Result = sorted[rank - 1]
```

All nodes compute identical result. Capped at `MAX_REWARD_PER_CONTRIBUTOR = 10,000 HCLAW`.

### 3E. Vote Participation Requirements

- Each node must vote in ≥2/3 of all active reward rounds
- Rolling window of 100 most recent eligible rounds
- Nodes below 2/3 participation get `agreed_with_consensus: false` recorded in `AccuracyTracker`, degrading their reputation
- Voting windows: 10 minutes standard (5 commit + 5 reveal), 1 hour maximum

### 3F. Vote Cleanup

- Completed rounds kept for 24-hour grace period after finalization
- After grace period: round data (commitments, reveals) deleted from memory
- `GovernanceManager::cleanup_old_rounds(now)` called on each tick
- No persistence to disk needed — votes are ephemeral, only the final `ContributorReward` system job is persisted in the chain

### 3G. New SystemJobKind Variants

```rust
// Add to existing enum:
ReserveTrancheUnlock { tranche_index: u8, amount: HclawAmount, unlock_timestamp: Timestamp },
ContributorRewardVoteStarted { round_id: Hash, contributor: Address, version: String },
ContributorReward { recipient: Address, amount: HclawAmount, tranche_index: u8, version: String, voter_count: u32 },
```

### 3H. New NetworkMessage Variants

```rust
// Add to existing enum:
RewardVoteCommit(RewardVoteCommitment),
RewardVoteReveal(RewardVoteReveal),
ContributorDetected { version: String, contributors: Vec<ContributorEntry>, detected_at: Timestamp },
```

New gossipsub topic: `hardclaw/governance/{chain_id}`

### 3I. Valuation Prompt

Embedded as `VALUATION_PROMPT` constant in `src/governance/valuation.rs`. Guides AI nodes on scoring contributions:

| Category | Max HCLAW | Examples |
|----------|-----------|---------|
| Security impact | 3,000 | Vulnerability fixes, crypto improvements |
| Protocol advancement | 2,500 | Planned features, throughput, verification |
| Network health | 2,000 | Peer discovery, bandwidth, partitions |
| Developer experience | 1,500 | Docs, tests, error messages |
| Ecosystem growth | 1,000 | New use cases, interop, tooling |

**Schelling focal points** (when uncertain):
- Trivial fix: 100-500
- Minor bug fix: 500-1,500
- Moderate feature: 1,500-3,000
- Significant feature: 3,000-6,000
- Major protocol change: 6,000-10,000

**Key principle**: Backwards-compatible iteration is rewarded. Radical breaking changes should be rejected (vote 0) in favor of incremental improvement.

### 3J. Module Structure

```
src/governance/
├── mod.rs              — GovernanceManager, GovernanceError, constants
├── reserve.rs          — ReserveManager, ReserveTranche, march_1 calculation
├── contributors.rs     — ContributorEntry, contributor_registry()
├── voting.rs           — RewardVotingRound, RewardVoteCommitment, RewardVoteReveal
├── participation.rs    — VoteParticipation (2/3 tracking)
├── percentile.rs       — percentile_80() deterministic integer calculation
├── valuation.rs        — VALUATION_PROMPT constant
└── cleanup.rs          — Vote data cleanup after grace period
```

### 3K. Integration Points

| Existing Module | Change |
|----------------|--------|
| `src/state/mod.rs` | Add `governance: Option<GovernanceManager>` to `ChainState` |
| `src/tokenomics/supply.rs` | Add `record_reserve_mint(amount)` method |
| `src/types/job.rs` | Add 3 new `SystemJobKind` variants |
| `src/network/mod.rs` | Add governance gossipsub topic + message variants |
| `src/node.rs` | Call `governance.tick(now)` in verifier tick, handle `ContributorReward` disbursement |
| `src/lib.rs` | Add `pub mod governance;` |

---

## Implementation Order

### Phase 1: Post-Quantum Crypto (MOSTLY COMPLETE)

Steps 1-6 done. Remaining:
- **1a.** Fix 4 remaining build errors in `src/node.rs` (3 errors) and `src/onboarding.rs` (1 error):
  - `node.rs:69`: `SecretKey::from_bytes(seed)` → `SecretKey::from_bytes(&seed)`
  - `node.rs:289-292`: `authority_bytes.try_into()` → `PublicKey::from_bytes(&authority_bytes)?` and fix error message (1952 bytes not 32)
  - `node.rs:307,313,333`: `*self.keypair.public_key()` → `self.keypair.public_key().clone()`
  - `onboarding.rs:312`: `Wallet::from_secret_bytes(secret_bytes)` → `Wallet::from_secret_bytes(&secret_bytes)`
- **7.** Add HQC KEM types (`src/crypto/kem.rs`)
- **8.** Update `src/network/mod.rs` — HQC key material
- **9.** Build + test

### Phase 1b: Seed Phrase Recovery (ml-dsa migration)

1. Replace `pqcrypto-dilithium` + `pqcrypto-traits` with `ml-dsa` in `Cargo.toml`
2. Rewrite `src/crypto/signature.rs` internals to use `ml_dsa::MlDsa65`
3. Add `Keypair::from_seed(&[u8; 32])` for deterministic keygen
4. Rewrite `src/crypto/mnemonic.rs` — `keypair_from_mnemonic` now deterministic
5. Bump wallet to v3 in `src/wallet/mod.rs` — add `seed_derived: bool`
6. Build + test (same mnemonic → same keypair)

### Phase 2: Genesis Redesign (unchanged)

1. Rewrite `src/genesis/mod.rs` — new constants, simpler types
2. Rewrite `src/genesis/airdrop.rs` — flat 100-token allocation for 5,000 wallets
3. Create `src/genesis/bounty.rs` — daily curve + slot machine payout
4. Simplify `src/genesis/competency.rs` — single-solution test
5. Rewrite `src/genesis/bootstrap.rs` — simpler state machine
6. Simplify `src/genesis/config.rs` — no tier arrays in TOML
7. Delete `src/genesis/vesting.rs` and `src/genesis/liveness.rs`
8. Update `src/types/job.rs` — bounty SystemJobKind variants
9. Update `src/state/mod.rs` — bootstrap integration with bounty
10. Update `src/tokenomics/supply.rs` — bounty pool minting
11. Update `src/tokenomics/burn.rs` — withheld bounty burns
12. Update `src/node.rs` — simplified genesis config loading
13. Build + test

### Phase 3: Self-Maintenance Reserve

1. Create `src/governance/mod.rs` — `GovernanceManager`, constants, `GovernanceError`
2. Create `src/governance/reserve.rs` — `ReserveManager`, `ReserveTranche`, march_1 calc
3. Create `src/governance/contributors.rs` — `ContributorEntry`, empty registry
4. Create `src/governance/percentile.rs` — `percentile_80()`
5. Create `src/governance/valuation.rs` — `VALUATION_PROMPT` constant
6. Create `src/governance/voting.rs` — `RewardVotingRound`, commit/reveal types
7. Create `src/governance/participation.rs` — `VoteParticipation`
8. Create `src/governance/cleanup.rs` — old round cleanup
9. Add `pub mod governance;` to `src/lib.rs`
10. Add `SystemJobKind` variants to `src/types/job.rs`
11. Add `NetworkMessage` variants + governance gossipsub topic to `src/network/mod.rs`
12. Add `governance` field to `ChainState` in `src/state/mod.rs`
13. Add `record_reserve_mint()` to `src/tokenomics/supply.rs`
14. Wire `governance.tick()` into `src/node.rs`
15. Build + test + clippy

---

## Key Files Reference

| File | Role | Key Types/Functions |
|------|------|-------------------|
| `src/crypto/signature.rs` | ML-DSA primitives | `PublicKey`, `Signature`, `SecretKey`, `Keypair::from_seed()` |
| `src/crypto/hash.rs` | BLAKE3 hashing (unchanged) | `Hash`, `hash_data()`, `merkle_root()` |
| `src/crypto/commitment.rs` | SHA3-256 commitments (unchanged) | `Commitment` |
| `src/crypto/mnemonic.rs` | Deterministic key derivation | `keypair_from_mnemonic()` (now deterministic) |
| `src/genesis/bounty.rs` | NEW: daily bounty system | `BountyCurve`, `SlotMachinePayout`, `BountyTracker` |
| `src/genesis/bootstrap.rs` | State machine | `BootstrapState`, `BootstrapPhase` |
| `src/genesis/airdrop.rs` | Flat allocation | `AirdropTracker` (simplified) |
| `src/governance/mod.rs` | NEW: governance orchestrator | `GovernanceManager`, `GovernanceError` |
| `src/governance/reserve.rs` | NEW: tranche management | `ReserveManager`, `ReserveTranche` |
| `src/governance/voting.rs` | NEW: reward voting | `RewardVotingRound`, commit/reveal types |
| `src/governance/percentile.rs` | NEW: deterministic 80th percentile | `percentile_80()` |
| `src/governance/valuation.rs` | NEW: AI valuation prompt | `VALUATION_PROMPT` |
| `src/governance/contributors.rs` | NEW: contributor registry | `ContributorEntry`, `contributor_registry()` |
| `src/types/job.rs` | System job types | `SystemJobKind` (+ governance variants) |
| `src/tokenomics/supply.rs` | Supply tracking | `SupplyManager::record_reserve_mint()` |
| `src/state/mod.rs` | Chain state | `ChainState` (+ governance field) |
| `src/verifier/accuracy.rs` | Rolling accuracy | `AccuracyTracker` (unchanged) |
| `src/verifier/stake.rs` | Staking | `StakeManager`, `DynamicStakeConfig` |

## Verification

1. `cargo build` — clean compilation
2. `cargo test` — all existing + new tests pass
3. `cargo clippy` — no warnings
4. Unit tests for:
   - ML-DSA sign/verify roundtrip
   - **Deterministic keygen: same seed → same keypair (new)**
   - **Mnemonic roundtrip: same phrase → same wallet (new)**
   - HQC encapsulate/decapsulate roundtrip
   - Bounty curve: Σw(0..89) produces exact total, peak at day 60
   - Slot machine: payout threshold auto-adjusts, never exceeds daily cap
   - Flat airdrop: 5,000 wallets × 100 tokens
   - Simplified competency: pass/fail on single solution
   - Min stake: 50 HCLAW enforced
   - **Reserve tranches: unlock timing, disbursement, exhaustion (new)**
   - **80th percentile: deterministic, edge cases (empty, single, all-same) (new)**
   - **Reward voting round: phase transitions, commit/reveal, finalization (new)**
   - **Vote participation: 2/3 tracking, reputation penalty on deficiency (new)**
   - **Cleanup: old rounds removed after grace period (new)**
5. Integration: 3-node LAN test with `--chain-id hardclaw-testnet-N`, verify bounty payouts trigger after 5 public nodes
6. **Governance integration: trigger contributor reward vote, verify 80th percentile payout (new)**
