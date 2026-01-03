# Raydium Alert System

A comprehensive Solana blockchain monitoring system for Raydium DEX. This monorepo contains real-time swap event alerts and decoder libraries for all Raydium AMM protocols.

## Overview

```
raydium-alert/
├── alerts/                        # Real-time monitoring application
│   └── src/
│       ├── processors/            # Protocol-specific processors
│       └── output/                # Formatters & webhook notifications
└── decoders/                      # Instruction decoder libraries
    ├── raydium-cpmm-decoder/      # CPMM protocol decoder
    ├── raydium-clmm-decoder/      # CLMM protocol decoder
    └── raydium-amm-v4-decoder/    # AMM V4 protocol decoder
```

## Supported Protocols

| Protocol | Program ID | Description |
|----------|------------|-------------|
| **CPMM** | `CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C` | Constant Product Market Maker |
| **CLMM** | `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK` | Concentrated Liquidity Market Maker |
| **AMM V4** | `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8` | Legacy AMM with Serum integration |

## Quick Start

### Prerequisites

- Rust 1.70+ 
- Solana RPC WebSocket endpoint (recommended: paid provider)

### Setup

```bash
# Clone the repository
git clone https://github.com/example/raydium-alert.git
cd raydium-alert

# Setup environment
cp alerts/.env.example alerts/.env
# Edit alerts/.env with your configuration

# Build all packages
cargo build --release

# Run the alert system
cargo run --release -p raydium-alerts
```

## Packages

### `raydium-alerts` - Alert System

Real-time monitoring application with features:

- **Multi-protocol support**: CPMM, CLMM, and AMM V4
- **Flexible filtering**: By market type, token mints, or pool addresses
- **Multiple output formats**: Text, JSON, or pretty JSON
- **Webhook notifications**: Discord, Slack, or custom endpoints
- **Accurate swap amounts**: Parses nested token transfers for actual values
- **Graceful shutdown**: Clean exit with Ctrl+C

[View detailed documentation](./alerts/README.md)

### Decoder Libraries

Carbon-compatible instruction decoders for Raydium protocols:

| Package | Description |
|---------|-------------|
| `carbon-raydium-cpmm-decoder` | Decodes CPMM instructions (swap, deposit, withdraw) |
| `carbon-raydium-clmm-decoder` | Decodes CLMM instructions (swap, liquidity, positions) |
| `carbon-raydium-amm-v4-decoder` | Decodes AMM V4 instructions (swap, initialize, withdraw) |

## Configuration

All configuration via environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `RPC_WS_URL` | Solana RPC WebSocket endpoint | `wss://api.mainnet-beta.solana.com/` |
| `FILTER_MARKETS` | Markets to monitor: `cpmm`, `clmm`, `amm_v4` | All |
| `FILTER_TOKENS` | Token mints to track (comma-separated) | All |
| `FILTER_AMMS` | Pool addresses to track (comma-separated) | All |
| `OUTPUT_FORMAT` | Output: `text`, `json`, `json_pretty` | `text` |
| `WEBHOOK_URL` | Webhook URL for notifications | Disabled |
| `RUST_LOG` | Log level | `info` |

## Example Output

### Text Format
```
SWAP [CPMM]
SOL 11.9880 ($1491.19)
MACARON 11500.70
Maker: 7xKXt...abc
MCap: $615.34K
https://solscan.io/tx/5abc123...
```

### JSON Format
```json
{
  "event_type": "swap",
  "protocol": "cpmm",
  "signature": "5abc...",
  "pool": "pool123",
  "input_token": {"mint": "So111...", "amount_raw": 11988000000},
  "output_token": {"mint": "Mac...", "amount_raw": 11500700000},
  "direction": "exact_input",
  "maker": "7xKXt..."
}
```

## Example Configurations

### Monitor SOL swaps on CLMM only

```bash
FILTER_MARKETS=clmm
FILTER_TOKENS=So11111111111111111111111111111111111111112
OUTPUT_FORMAT=text
```

### Send all swaps to Discord

```bash
OUTPUT_FORMAT=json
WEBHOOK_URL=https://discord.com/api/webhooks/your-webhook-url
```

### Monitor specific pool

```bash
FILTER_AMMS=YourPoolAddressHere
OUTPUT_FORMAT=text
```

## Development

```bash
# Build all packages
cargo build

# Run tests
cargo test --workspace

# Run with debug logging
RUST_LOG=debug cargo run -p raydium-alerts

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
```

## Architecture

The system is built on the [Carbon](https://github.com/sevenlabs-hq/carbon) framework for Solana data processing:

```
Solana RPC (WebSocket)
        │
        ▼
┌──────────────────┐
│  RpcBlockSubscribe │  ← Real-time block subscription
└──────────────────┘
        │
        ▼
┌──────────────────┐
│     Decoders     │  ← Protocol-specific instruction parsing
│  CPMM/CLMM/V4    │
└──────────────────┘
        │
        ▼
┌──────────────────┐
│   Processors     │  ← Filter & transform events
└──────────────────┘
        │
        ▼
┌──────────────────┐
│     Output       │  ← Format & deliver (stdout, webhook)
└──────────────────┘
```

## RPC Recommendations

For production use, we recommend paid RPC providers:

- [Helius](https://helius.xyz/)
- [QuickNode](https://quicknode.com/)
- [Triton](https://triton.one/)

Public Solana RPC has rate limits and may miss events during high traffic.

## Roadmap

- [ ] Extract actual swap amounts from nested token transfers (CPMM/CLMM)
- [ ] Token metadata: symbols and decimals from on-chain/API
- [ ] USD price integration (Jupiter, Birdeye)
- [ ] Market cap calculation
- [ ] Rate limiting for webhooks
- [ ] Prometheus metrics endpoint
- [ ] Historical data backfill

## Tech Stack

- **Runtime**: Rust 2021 Edition
- **Framework**: Carbon 0.12.0
- **Solana SDK**: 3.0
- **Async**: Tokio
- **HTTP**: Reqwest with rustls

## License

MIT
