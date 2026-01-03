# Raydium Alerts

Real-time monitoring system for Raydium DEX swap events on Solana blockchain. Supports all three Raydium AMM protocols with flexible filtering and multiple output formats.

## Features

- **Multi-protocol support**: CPMM, CLMM, and AMM V4
- **Flexible filtering**: By market type, token mints, or pool addresses
- **Multiple output formats**: Human-readable text, JSON, or pretty JSON
- **Webhook notifications**: Send alerts to external services (Discord, Slack, etc.)
- **Accurate swap amounts**: Parses nested token transfers to get actual amounts (not just slippage limits)
- **Graceful shutdown**: Ctrl+C for clean exit

## Supported Protocols

| Protocol | Program ID | Description |
|----------|------------|-------------|
| **CPMM** | `CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C` | Constant Product Market Maker |
| **CLMM** | `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK` | Concentrated Liquidity Market Maker |
| **AMM V4** | `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8` | Legacy AMM with Serum integration |

## Quick Start

### 1. Setup Environment

```bash
cp .env.example .env
# Edit .env with your configuration
```

### 2. Build & Run

```bash
cargo build --release -p raydium-alerts
cargo run --release -p raydium-alerts
```

## Configuration

All configuration is done via environment variables. See `.env.example` for a complete template.

### Core Settings

| Variable | Description | Default |
|----------|-------------|---------|
| `RPC_WS_URL` | Solana RPC WebSocket endpoint | `wss://api.mainnet-beta.solana.com/` |
| `OUTPUT_FORMAT` | Output format: `text`, `json`, `json_pretty` | `text` |
| `WEBHOOK_URL` | Webhook URL for notifications (optional) | disabled |
| `RUST_LOG` | Log level | `info` |

### Filters

| Variable | Description | Default |
|----------|-------------|---------|
| `FILTER_MARKETS` | Which markets to monitor | All markets |
| `FILTER_TOKENS` | Token mints to track | All tokens |
| `FILTER_AMMS` | AMM/pool addresses to track | All AMMs |

## Output Formats

### Text Format (default)

Human-readable format with emojis:

```
🔄 SWAP [CPMM]
🔷 SOL 11.9880 ($1491.19)
🪙 MACARON 11500.70
🔎 Maker: 7xKXt...abc
📈 MCap: $615.34K
🔗 https://solscan.io/tx/5abc123...
```

### JSON Format

Compact JSON for log aggregation:

```json
{"event_type":"swap","protocol":"cpmm","signature":"5abc...","pool":"pool123","input_token":{"mint":"So111...","amount_raw":11988000000},"output_token":{"mint":"Mac...","amount_raw":11500700000},"direction":"exact_input","maker":"7xKXt...","slot":12345}
```

### JSON Pretty Format

Pretty-printed JSON for debugging.

## Filter Examples

### Market Filter (`FILTER_MARKETS`)

Control which Raydium protocols to monitor:

```bash
# Only monitor CLMM swaps
FILTER_MARKETS=clmm

# Monitor CPMM and CLMM only
FILTER_MARKETS=cpmm,clmm

# Monitor all markets (default when empty or unset)
FILTER_MARKETS=
```

**Valid values:** `cpmm`, `clmm`, `amm_v4` (also accepts: `ammv4`, `amm-v4`, `v4`)

### Token Filter (`FILTER_TOKENS`)

Track swaps involving specific tokens:

```bash
# Track SOL and USDC swaps only
FILTER_TOKENS=So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
```

**Common tokens:**
- SOL (Wrapped): `So11111111111111111111111111111111111111112`
- USDC: `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`
- USDT: `Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB`

### AMM/Pool Filter (`FILTER_AMMS`)

Track swaps on specific pools only:

```bash
# Only track specific pools
FILTER_AMMS=poolAddress1,poolAddress2
```

### Filter Logic

Filters use **OR logic**:
- A swap is logged if it matches **ANY** of the configured filters
- If both `FILTER_TOKENS` and `FILTER_AMMS` are set, a swap matching either will be logged
- Empty filter = no filtering (track all)

## Example Configurations

### Monitor all SOL swaps on CLMM only

```bash
RPC_WS_URL=wss://your-rpc-endpoint.com
FILTER_MARKETS=clmm
FILTER_TOKENS=So11111111111111111111111111111111111111112
OUTPUT_FORMAT=text
RUST_LOG=info
```

### Monitor all swaps with JSON output

```bash
RPC_WS_URL=wss://your-rpc-endpoint.com
OUTPUT_FORMAT=json
RUST_LOG=info
```

### Send alerts to Discord webhook

```bash
RPC_WS_URL=wss://your-rpc-endpoint.com
FILTER_TOKENS=So11111111111111111111111111111111111111112
OUTPUT_FORMAT=json
WEBHOOK_URL=https://discord.com/api/webhooks/your-webhook-url
RUST_LOG=info
```

### Monitor specific pool across all protocols

```bash
RPC_WS_URL=wss://your-rpc-endpoint.com
FILTER_AMMS=YourPoolAddressHere
OUTPUT_FORMAT=text
RUST_LOG=info
```

## RPC Recommendations

For production use, we recommend using a paid RPC provider:
- [Helius](https://helius.xyz/)
- [QuickNode](https://quicknode.com/)
- [Triton](https://triton.one/)

The public Solana RPC has rate limits and may miss events during high traffic.

## Project Structure

```
alerts/src/
├── main.rs                 # Entry point, pipeline setup, graceful shutdown
├── config.rs               # Environment variable parsing, MarketType enum
├── output/
│   ├── mod.rs              # Output module exports
│   ├── swap_event.rs       # SwapEvent, TokenInfo, formatters
│   ├── token_transfer.rs   # Token transfer parser for actual amounts
│   └── webhook.rs          # Async webhook notifier with retry
└── processors/
    ├── mod.rs              # Processor module exports
    ├── cpmm.rs             # CPMM instruction processor
    ├── clmm.rs             # CLMM instruction processor
    └── amm_v4.rs           # AMM V4 instruction processor
```

## Technical Notes

### Accurate Swap Amounts

Raydium swap instructions contain parameters like `minimum_amount_out` or `max_amount_in` which are **slippage protection values**, not actual swap amounts. This system parses the nested SPL Token Transfer instructions (inner instructions) to extract the **actual transferred amounts**.

### Event Types

- `Swap` - Token swap event
- `AddLiquidity` - Liquidity added to pool
- `RemoveLiquidity` - Liquidity removed from pool
- `CreatePool` - New pool creation

## Development

```bash
# Build
cargo build -p raydium-alerts

# Run tests
cargo test -p raydium-alerts

# Run with debug logging
RUST_LOG=debug cargo run -p raydium-alerts

# Lint
cargo clippy -p raydium-alerts -- -D warnings
```

## TODO

- [ ] **CPMM**: Extract actual swap amounts from nested token transfers (currently shows min/max)
- [ ] **CLMM**: Extract actual swap amounts from nested token transfers (currently shows threshold)
- [ ] **Token Metadata**: Fetch token symbols and decimals from on-chain or API
- [ ] **USD Prices**: Integrate price oracle (Jupiter, Birdeye) for USD values
- [ ] **Market Cap**: Calculate market cap from token supply data
- [ ] **Rate Limiting**: Add configurable rate limits for webhook notifications
- [ ] **Metrics**: Add Prometheus metrics endpoint

## License

MIT
