# Stellar Micropayment Streaming Contracts

Soroban smart contracts enabling secure micropayment streaming on Stellar, enforcing rate-based transfers, escrow logic, time-based settlement, and trustless execution of continuous payment flows.

## Overview

This repository contains the on-chain enforcement layer for the **stellar-micropay-stream** platform — a real-time micropayment streaming system built on Stellar. Two Soroban contracts handle trustless payment logic:

- **`escrow_contract`** — Locks funds with optional time-locks, enables receiver claims or sender refunds
- **`streaming_contract`** — Rate-based continuous payments with per-second settlement

## Why Stellar for Streaming Payments?

- **Low Latency Finality** — Transactions confirm in ~3–5 seconds via Stellar Consensus Protocol
- **Ultra-Low Fees** — ~$0.00001 per transaction makes micropayments viable
- **Native Streaming Support** — Real-time payment events via Horizon API
- **High Throughput** — Supports both on-chain and off-chain (Layer 2) payment channels

## Architecture

```
User Wallet
    │
    ▼
Frontend (React/Next.js)
    │
    ▼
Backend API (Node.js)
    │
    ▼
Soroban Contracts ◄─── This Repo
    │
    ▼
Stellar Network
```

## Contracts

### 1. Escrow Contract

Trustless escrow with time-lock support for secure fund holding and conditional release.

**Storage:**
```rust
pub struct EscrowState {
    pub sender: Address,
    pub receiver: Address,
    pub token: Address,
    pub amount: i128,
    pub unlock_time: u64,  // Unix timestamp; 0 = no lock
    pub released: bool,
}
```

**Functions:**

| Function | Auth | Description |
|----------|------|-------------|
| `deposit(escrow_id, sender, receiver, token, amount, unlock_time)` | sender | Locks tokens in contract |
| `release(escrow_id, caller)` | caller | Receiver claims anytime; sender after `unlock_time` |
| `refund(escrow_id)` | sender | Sender reclaims after `unlock_time` expires |
| `get_escrow(escrow_id)` | — | View escrow state |

**Time-lock rules:**
- `unlock_time = 0` → No lock; receiver can release immediately
- `unlock_time > 0` → Sender can refund only when `ledger.timestamp >= unlock_time`

### 2. Streaming Contract

Rate-based continuous payment with per-second settlement and automatic balance tracking.

**Storage:**
```rust
pub struct StreamState {
    pub sender: Address,
    pub receiver: Address,
    pub token: Address,
    pub rate_per_sec: i128,   // stroops per second
    pub deposit: i128,        // total locked upfront
    pub transferred: i128,    // cumulative paid to receiver
    pub last_tick: u64,       // ledger timestamp of last settlement
    pub closed: bool,
}
```

**Functions:**

| Function | Auth | Description |
|----------|------|-------------|
| `open_stream(stream_id, sender, receiver, token, deposit, rate_per_sec)` | sender | Locks deposit, starts stream |
| `tick(stream_id)` | anyone | Settles `rate × elapsed` to receiver |
| `close_stream(stream_id, caller)` | sender or receiver | Final settlement + refund leftover |
| `get_stream(stream_id)` | — | View stream state |

**Rate formula:**
```
due = min(rate_per_sec × (now − last_tick), deposit − transferred)
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (no_std) |
| SDK | soroban-sdk 22.0.0 |
| Build target | wasm32-unknown-unknown |
| Test framework | soroban-sdk testutils |

## Getting Started

### Prerequisites

- Rust 1.74+ with `wasm32-unknown-unknown` target
- Stellar CLI (for deployment)
- Funded Stellar testnet account

### Installation

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Add WASM target
rustup target add wasm32-unknown-unknown

# Install Stellar CLI
cargo install --locked stellar-cli --features opt
```

### Build

```bash
# Development build
cargo build

# Release build (optimized WASM)
cargo build --target wasm32-unknown-unknown --release
```

### Test

```bash
cargo test
```

**Test coverage:** 9 tests (4 escrow + 5 streaming) — all passing

#### Escrow Tests
- ✅ `test_deposit_and_release_by_receiver` — Happy path release
- ✅ `test_refund_after_timelock` — Sender reclaims after lock expires
- ✅ `test_refund_before_timelock_panics` — Panics with `"time-lock active"`
- ✅ `test_double_release_panics` — Panics with `"already released"`

#### Streaming Tests
- ✅ `test_open_and_tick` — 50s elapsed at 100 stroops/s → 5000 transferred
- ✅ `test_tick_caps_at_deposit` — Caps payment at remaining deposit
- ✅ `test_close_stream_refunds_leftover` — Returns unused deposit to sender
- ✅ `test_double_close_panics` — Panics with `"already closed"`
- ✅ `test_tick_on_closed_stream_panics` — Panics with `"stream closed"`

## Deployment

### Testnet

```bash
# Build optimized WASM
cargo build --target wasm32-unknown-unknown --release

# Deploy escrow contract
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/escrow_contract.wasm \
  --network testnet \
  --source <your-keypair>

# Deploy streaming contract
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/streaming_contract.wasm \
  --network testnet \
  --source <your-keypair>
```

Save the returned contract IDs for backend integration.

## Use Cases

- 🎬 **Pay-per-second streaming** — Video/audio content without subscriptions
- 🤖 **AI agent payments** — Autonomous API payments per request
- 🎮 **Gaming micro-rewards** — Real-time in-game payments
- 🌍 **IoT micropayments** — Devices paying per data usage
- 📰 **Pay-per-article** — Journalism without paywalls

## Project Structure

```
contracts/
├── escrow_contract/
│   ├── Cargo.toml
│   └── src/lib.rs        # 4 tests
└── streaming_contract/
    ├── Cargo.toml
    └── src/lib.rs        # 5 tests
Cargo.toml                # workspace root
```

## Security

- **Replay protection** — Unique escrow/stream IDs prevent double-spending
- **Time-lock enforcement** — Ledger timestamp validation prevents early refunds
- **Balance caps** — Streaming payments capped at remaining deposit
- **Auth requirements** — All state-changing operations require caller authentication
- **Immutable records** — Released/closed states prevent re-execution

## Integration

These contracts are designed to work with:
- **Backend:** Node.js/Express streaming orchestrator with payment scheduler
- **Frontend:** React/Next.js dashboard with Freighter wallet integration
- **Off-chain channels:** Starlight-style payment channels for high-throughput streaming

See the full platform documentation in `AI.md` and `projectinfo.md`.

## Development Roadmap

- [x] Core escrow logic with time-locks
- [x] Rate-based streaming with per-second settlement
- [x] Comprehensive test coverage
- [ ] Multi-token support
- [ ] Dynamic rate adjustment
- [ ] Dispute resolution mechanism
- [ ] Mainnet deployment

## License

MIT

## Contributing

Contributions welcome! Please open an issue or PR.

## Resources

- [Stellar Documentation](https://developers.stellar.org/)
- [Soroban Smart Contracts](https://soroban.stellar.org/)
- [Stellar Consensus Protocol](https://www.stellar.org/papers/stellar-consensus-protocol)
- [Horizon API](https://developers.stellar.org/api/horizon)
