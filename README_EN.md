# Polymarket HFT Trading Bot

High-performance, standalone trading bot for [Polymarket](https://polymarket.com) built in Rust. Features a multi-strategy engine, real-time liquidity analysis, and Excel-compatible trade logging with P&L tracking.

---

## How It Works

### Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    POLYMARKET BOT                        │
│                                                         │
│  ┌───────────┐    ┌──────────────┐    ┌──────────────┐ │
│  │  Binance   │───▶│  STRATEGIES  │───▶│ Trade Logger │ │
│  │  BTC/USDT  │    │              │    │   (CSV/P&L)  │ │
│  │  5m candles│    │ 1. Momentum  │    └──────┬───────┘ │
│  └───────────┘    │ 2. RSI       │           │         │
│                    │ 3. Mean Rev  │    ┌──────▼───────┐ │
│  ┌───────────┐    │ 4. Liquidity │    │  trades_*.csv │ │
│  │ Polymarket │───▶│              │    │  (per strat)  │ │
│  │ Orderbook  │    └──────┬───────┘    └──────────────┘ │
│  │ /book API  │           │                             │
│  └───────────┘    ┌───────▼───────┐                     │
│                    │   MONITOR     │                     │
│                    │ (every 30s)   │                     │
│                    │               │                     │
│                    │ • Detect      │    ┌──────────────┐ │
│                    │ • Log PENDING │───▶│  REST API     │ │
│                    │ • Resolve 5m  │    │  :8000        │ │
│                    │ • Calc P&L    │    └──────────────┘ │
│                    └───────────────┘                     │
└─────────────────────────────────────────────────────────┘
```

### Execution Flow (every 30 seconds)

1. **Fetch Data** — Gets the last 14 BTC/USDT 5-minute candles from Binance API
2. **Run Strategies** — All 4 strategies analyze the candles independently
3. **Check Liquidity** — If a real BTC Polymarket market exists, fetches the orderbook (`/book`) and runs the liquidity strategy
4. **Generate Signals** — Each strategy either returns `COMPRAR_YES` (buy YES), `COMPRAR_NO` (buy NO), or nothing
5. **Log Entry** — Signal is recorded in the strategy's CSV as `PENDING`
6. **Resolve (5 min later)** — Compares BTC price now vs entry price:
   - If prediction was correct → **WIN** (+$2.99 profit)
   - If prediction was wrong → **LOSS** (-$3.01 loss)
7. **Log Resolution** — Result is written to CSV with P&L, and the total row is updated

---

## Strategies

### 1. Momentum (`trades_momentum.csv`)
Detects sustained directional movement in BTC.

| Parameter | Value |
|-----------|-------|
| **Candles analyzed** | Last 3 (5-min) |
| **Trigger** | 3 consecutive candles in same direction |
| **Threshold** | Accumulated diff > $100 |
| **COMPRAR_YES** | 3 bullish candles (BTC trending UP) |
| **COMPRAR_NO** | 3 bearish candles (BTC trending DOWN) |
| **Confidence** | `diff / threshold` (capped at 100%) |

### 2. RSI — Relative Strength Index (`trades_rsi.csv`)
Classic momentum oscillator measuring speed and magnitude of price changes.

| Parameter | Value |
|-----------|-------|
| **Candles analyzed** | Up to 14 (5-min) |
| **Formula** | `RSI = 100 - (100 / (1 + avg_gain/avg_loss))` |
| **COMPRAR_NO** | RSI > 70 (overbought, expects correction down) |
| **COMPRAR_YES** | RSI < 30 (oversold, expects bounce up) |
| **Confidence** | Distance from threshold (70 or 30) |

### 3. Mean Reversion (`trades_mean_rev.csv`)
Bets that extreme deviations from the average will revert.

| Parameter | Value |
|-----------|-------|
| **Candles analyzed** | All available (up to 14) |
| **Average** | Simple Moving Average of close prices |
| **Threshold** | $150 deviation from average |
| **COMPRAR_NO** | Price > avg + $150 (expects correction down) |
| **COMPRAR_YES** | Price < avg - $150 (expects bounce up) |
| **Confidence** | `deviation / threshold / 2` |

### 4. Liquidity (`trades_liquidity.csv`)
Analyzes the Polymarket orderbook to detect buying/selling pressure.

| Parameter | Value |
|-----------|-------|
| **Data source** | `GET /book?token_id=TOKEN` from Polymarket CLOB |
| **Metrics** | Spread, bid depth, ask depth, liquidity score |
| **Min depth** | $50 total depth required |
| **Min score** | 20/100 liquidity score required |
| **COMPRAR_YES** | Bid/Ask ratio > 1.5 (more buyers) |
| **COMPRAR_NO** | Bid/Ask ratio < 0.67 (more sellers) |
| **Confidence** | `(ratio - 1) / 2` |

#### Liquidity Score Calculation
```
spread_score = (1 - spread_pct/10%) × 50     // Max 50 pts if spread < 1%
depth_score  = min(total_depth/$1000, 1) × 50 // Max 50 pts if depth > $1000
liquidity_score = spread_score + depth_score   // 0-100
```

---

## P&L Tracking

Each trade goes through two phases:

| Phase | CSV Entry | Description |
|-------|-----------|-------------|
| **ENTRY** | `outcome: PENDING` | Signal detected, BTC price recorded |
| **RESOLUTION** | `outcome: WIN/LOSS` | After 5 min, BTC price compared |

**P&L Calculation:**
- **WIN**: `+$3.00 - fees` = +$2.99 (bet $3 at price $0.50, win $6, profit $3 minus 0.2% fee)
- **LOSS**: `-$3.00 - fees` = -$3.01 (lose entire bet plus fees)

Each CSV has a **TOTAL row** at the bottom visible in Excel:
```
--- TOTAL --- | | | | | $18.00 | | W:3 L:2 P:1 | GANANCIA | $5.95 | TOTAL: $5.95
```

---

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/wallet` | Bot's wallet address |
| `GET` | `/balance` | CLOB balance + on-chain USDC balance |
| `POST` | `/trade` | Execute/simulate a manual trade |
| `GET` | `/trades` | All strategies comparison (ranked by P&L) |
| `GET` | `/trades?strategy=momentum` | Specific strategy history + P&L |
| `GET` | `/strategies` | **Ranking** of all strategies by performance |
| `GET` | `/resultado?slug=...` | Query market status via Gamma API |

### Example: Strategy Ranking
```bash
curl http://localhost:8000/strategies | python3 -m json.tool
```
```json
{
  "best_strategy": "momentum",
  "strategies": [
    { "rank": 1, "strategy": "momentum", "wins": 5, "losses": 2, "win_rate": "71.4%", "total_profit_loss": "8.93", "resultado": "GANANCIA" },
    { "rank": 2, "strategy": "rsi", "wins": 3, "losses": 3, "win_rate": "50.0%", "total_profit_loss": "-0.12", "resultado": "PERDIDA" },
    { "rank": 3, "strategy": "liquidity", "wins": 1, "losses": 1, "win_rate": "50.0%", "total_profit_loss": "-0.02", "resultado": "PERDIDA" },
    { "rank": 4, "strategy": "mean_rev", "wins": 0, "losses": 0, "total_profit_loss": "0.00", "resultado": "NEUTRO" },
    { "rank": 5, "strategy": "manual", "wins": 0, "losses": 0, "total_profit_loss": "0.00", "resultado": "NEUTRO" }
  ]
}
```

---

## Project Structure

```
src/
├── main.rs          # App startup, routing, shared state (AppState)
├── auth.rs          # HMAC-SHA256 L2 authentication headers
├── handlers.rs      # HTTP endpoint handlers (/balance, /trades, /strategies)
├── models.rs        # Data structures (Order, TradeRequest, EIP-712 types)
├── monitor.rs       # Background task: runs all strategies every 30s
├── strategies.rs    # 4 strategies: Momentum, RSI, Mean Reversion, Liquidity
├── trade_logger.rs  # Per-strategy CSV logging with P&L totals
└── trading.rs       # Trade execution logic (simulation & real orders)
```

---

## Configuration (.env)

| Variable | Description | Default |
|----------|-------------|---------|
| `POLYMARKET_PRIVATE_KEY` | Ethereum private key | *required* |
| `CLOB_API_KEY` | Polymarket CLOB API key | *required* |
| `CLOB_SECRET` | CLOB API secret (Base64) | *required* |
| `CLOB_PASSPHRASE` | CLOB API passphrase | *required* |
| `POLYGON_RPC_URL` | Polygon RPC endpoint | `https://polygon-rpc.com` |
| `POLY_PROXY_ADDRESS` | Gnosis Safe / proxy address | *optional* |
| `TEST_MODE` | `true` = simulation, `false` = live | `true` |
| `TRADING_FEE_BPS` | Fee in basis points | `20` (0.2%) |
| `MIN_ORDER_AMOUNT` | Min order value (USDC) | `1.0` |
| `MAX_ORDER_AMOUNT` | Max order value (USDC) | `1000.0` |
| `RUST_LOG` | Log verbosity | `info` |

### Wallet Types
| Type | `signature_type` | When |
|------|-------------------|------|
| EOA | `0` | No `POLY_PROXY_ADDRESS` set |
| Gnosis Safe | `2` | `POLY_PROXY_ADDRESS` is set |

---

## Installation

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Build
cd ~/Descargas/polymarketbot
cargo build --release
```

## Usage

```bash
# 1. Configure
cp .env.example .env   # Edit with your credentials

# 2. Run
./target/release/polymarket_bot_hft

# 3. Monitor strategies (wait ~10 min for results)
curl http://localhost:8000/strategies | python3 -m json.tool

# 4. View specific strategy
curl "http://localhost:8000/trades?strategy=momentum" | python3 -m json.tool

# 5. Manual test trade
curl -X POST http://localhost:8000/trade \
  -H "Content-Type: application/json" \
  -d '{"token_id":"123456","price":0.5,"amount":2.2,"side":"BUY"}'
```

## CSV Files

Open any CSV directly in Excel or LibreOffice Calc:

| File | Strategy |
|------|----------|
| `trades_momentum.csv` | BTC Momentum (3 candles) |
| `trades_rsi.csv` | RSI Overbought/Oversold |
| `trades_mean_rev.csv` | Mean Reversion |
| `trades_liquidity.csv` | Orderbook Liquidity |
| `trades_manual.csv` | Manual API trades |

Each CSV has columns: `timestamp, side, token_id, price, amount, total_usdc, fee, mode, result, profit_loss, outcome`

---

## Logs

```
🔍 Monitor MULTI-ESTRATEGIA + LIQUIDEZ iniciado
📈 [MOMENTUM] COMPRAR_YES | BTC:$82345 | 3UP diff:$294 | Conf:100%
📈 [RSI] COMPRAR_NO | BTC:$82345 | RSI:73.2 overbought | Conf:11%
📊 LIQUIDEZ: Bid:$450 Ask:$380 Spread:4.2% Depth:$830 Score:67
📝 [MOMENTUM] PENDING → 5 min
📊 [MOMENTUM] COMPRAR_YES BTC $82345→$82510 WIN P&L:$2.99
```

---

## Future: AI Integration

The strategy system is designed for AI integration. To add an AI-powered strategy:

1. Create a function in `strategies.rs` that receives market data and returns `Option<StrategySignal>`
2. Add it to the `signals` vector in `monitor.rs`
3. The AI strategy will automatically get its own CSV (`trades_ai.csv`), P&L tracking, and ranking comparison
