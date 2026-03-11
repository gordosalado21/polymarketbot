# Polymarket HFT Trading Bot

Bot de trading autónomo de alto rendimiento para [Polymarket](https://polymarket.com), construido en Rust. Motor multi-estrategia, análisis de liquidez en tiempo real, y registro de trades en Excel con tracking de ganancias/pérdidas.

---

## Cómo Funciona

### Arquitectura General

```
┌─────────────────────────────────────────────────────────┐
│                    POLYMARKET BOT                        │
│                                                         │
│  ┌───────────┐    ┌──────────────┐    ┌──────────────┐ │
│  │  Binance   │───▶│ ESTRATEGIAS  │───▶│ Trade Logger │ │
│  │  BTC/USDT  │    │              │    │  (CSV / P&L) │ │
│  │  velas 5m  │    │ 1. Momentum  │    └──────┬───────┘ │
│  └───────────┘    │ 2. RSI       │           │         │
│                    │ 3. Mean Rev  │    ┌──────▼───────┐ │
│  ┌───────────┐    │ 4. Liquidez  │    │ trades_*.csv │ │
│  │ Polymarket │───▶│              │    │ (por estrat.) │ │
│  │ Orderbook  │    └──────┬───────┘    └──────────────┘ │
│  │  /book API │           │                             │
│  └───────────┘    ┌───────▼───────┐                     │
│                    │   MONITOR     │                     │
│                    │ (cada 30s)    │                     │
│                    │               │                     │
│                    │ • Detectar    │    ┌──────────────┐ │
│                    │ • Log PENDING │───▶│   API REST   │ │
│                    │ • Resolver 5m │    │   :8000      │ │
│                    │ • Calcular P&L│    └──────────────┘ │
│                    └───────────────┘                     │
└─────────────────────────────────────────────────────────┘
```

### Flujo de Ejecución (cada 30 segundos)

1. **Obtener datos** — Lee las últimas 14 velas de 5 minutos de BTC/USDT de Binance
2. **Ejecutar estrategias** — Las 4 estrategias analizan las velas de forma independiente
3. **Verificar liquidez** — Si existe un mercado BTC real en Polymarket, consulta el orderbook (`/book`) y ejecuta la estrategia de liquidez
4. **Generar señales** — Cada estrategia devuelve `COMPRAR_YES` (comprar YES), `COMPRAR_NO` (comprar NO), o nada
5. **Registrar entrada** — La señal se graba en el CSV de la estrategia como `PENDING`
6. **Resolver (5 min después)** — Compara el precio actual de BTC con el de entrada:
   - Si la predicción fue correcta → **WIN** (+$2.99 de ganancia)
   - Si la predicción fue incorrecta → **LOSS** (-$3.01 de pérdida)
7. **Registrar resultado** — Se escribe el resultado con P&L en el CSV y se actualiza la fila TOTAL

---

## Estrategias

### 1. Momentum (`trades_momentum.csv`)
Detecta movimiento direccional sostenido en BTC.

| Parámetro | Valor |
|-----------|-------|
| **Velas analizadas** | Últimas 3 (5 min) |
| **Activación** | 3 velas consecutivas en la misma dirección |
| **Umbral** | Diferencia acumulada > $100 |
| **COMPRAR_YES** | 3 velas alcistas (BTC subiendo) |
| **COMPRAR_NO** | 3 velas bajistas (BTC bajando) |
| **Confianza** | `diff / umbral` (máximo 100%) |

### 2. RSI — Índice de Fuerza Relativa (`trades_rsi.csv`)
Oscilador de momentum que mide la velocidad y magnitud de los cambios de precio.

| Parámetro | Valor |
|-----------|-------|
| **Velas analizadas** | Hasta 14 (5 min) |
| **Fórmula** | `RSI = 100 - (100 / (1 + ganancia_prom/pérdida_prom))` |
| **COMPRAR_NO** | RSI > 70 (sobrecomprado, espera corrección bajista) |
| **COMPRAR_YES** | RSI < 30 (sobrevendido, espera rebote alcista) |
| **Confianza** | Distancia del umbral (70 o 30) |

### 3. Reversión a la Media (`trades_mean_rev.csv`)
Apuesta a que las desviaciones extremas del promedio se corregirán.

| Parámetro | Valor |
|-----------|-------|
| **Velas analizadas** | Todas las disponibles (hasta 14) |
| **Promedio** | Media Móvil Simple de precios de cierre |
| **Umbral** | Desviación de $150 del promedio |
| **COMPRAR_NO** | Precio > promedio + $150 (espera corrección bajista) |
| **COMPRAR_YES** | Precio < promedio - $150 (espera rebote alcista) |
| **Confianza** | `desviación / umbral / 2` |

### 4. Liquidez (`trades_liquidity.csv`)
Analiza el orderbook de Polymarket para detectar presión de compra/venta.

| Parámetro | Valor |
|-----------|-------|
| **Fuente de datos** | `GET /book?token_id=TOKEN` del CLOB de Polymarket |
| **Métricas** | Spread, profundidad de bids, profundidad de asks, score de liquidez |
| **Profundidad mínima** | $50 de profundidad total requerida |
| **Score mínimo** | 20/100 de score de liquidez |
| **COMPRAR_YES** | Ratio Bid/Ask > 1.5 (más compradores que vendedores) |
| **COMPRAR_NO** | Ratio Bid/Ask < 0.67 (más vendedores que compradores) |
| **Confianza** | `(ratio - 1) / 2` |

#### Cálculo del Score de Liquidez
```
score_spread = (1 - spread_pct/10%) × 50      // Máx 50 pts si spread < 1%
score_profundidad = min(depth_total/$1000, 1) × 50  // Máx 50 pts si depth > $1000
score_liquidez = score_spread + score_profundidad    // 0-100
```

---

## Tracking de Ganancias/Pérdidas

Cada trade pasa por dos fases:

| Fase | Entrada CSV | Descripción |
|------|-------------|-------------|
| **ENTRADA** | `outcome: PENDING` | Señal detectada, precio BTC registrado |
| **RESOLUCIÓN** | `outcome: WIN/LOSS` | Después de 5 min, se compara precio BTC |

**Cálculo de P&L:**
- **WIN**: `+$3.00 - comisión` = +$2.99 (apuesta $3 a precio $0.50, gana $6, beneficio $3 menos 0.2%)
- **LOSS**: `-$3.00 - comisión` = -$3.01 (pierde toda la apuesta más comisiones)

Cada CSV tiene una **fila TOTAL** al final visible en Excel:
```
--- TOTAL --- | | | | | $18.00 | | W:3 L:2 P:1 | GANANCIA | $5.95 | TOTAL: $5.95
```

---

## Endpoints de la API

| Método | Endpoint | Descripción |
|--------|----------|-------------|
| `GET` | `/wallet` | Dirección de la wallet del bot |
| `GET` | `/balance` | Balance CLOB + USDC on-chain |
| `POST` | `/trade` | Ejecutar/simular un trade manual |
| `GET` | `/trades` | Comparación de todas las estrategias (ranking) |
| `GET` | `/trades?strategy=momentum` | Historial de una estrategia específica |
| `GET` | `/strategies` | **Ranking** de todas las estrategias por P&L |
| `GET` | `/resultado?slug=...` | Consultar estado de un mercado vía Gamma API |

### Ejemplo: Ranking de Estrategias
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
    { "rank": 4, "strategy": "mean_rev", "total_profit_loss": "0.00", "resultado": "NEUTRO" },
    { "rank": 5, "strategy": "manual", "total_profit_loss": "0.00", "resultado": "NEUTRO" }
  ]
}
```

---

## Estructura del Proyecto

```
src/
├── main.rs          # Arranque, rutas, estado compartido (AppState)
├── auth.rs          # Autenticación HMAC-SHA256 L2
├── handlers.rs      # Manejadores HTTP (/balance, /trades, /strategies)
├── models.rs        # Estructuras de datos (Order, TradeRequest, EIP-712)
├── monitor.rs       # Tarea en segundo plano: ejecuta estrategias cada 30s
├── strategies.rs    # 4 estrategias: Momentum, RSI, Mean Reversion, Liquidez
├── trade_logger.rs  # Logging CSV por estrategia con totales de P&L
└── trading.rs       # Lógica de ejecución de trades (simulación y real)
```

---

## Configuración (.env)

| Variable | Descripción | Default |
|----------|-------------|---------|
| `POLYMARKET_PRIVATE_KEY` | Clave privada de Ethereum | *requerida* |
| `CLOB_API_KEY` | API key del CLOB de Polymarket | *requerida* |
| `CLOB_SECRET` | Secreto API CLOB (Base64) | *requerida* |
| `CLOB_PASSPHRASE` | Frase de paso API CLOB | *requerida* |
| `POLYGON_RPC_URL` | Endpoint RPC de Polygon | `https://polygon-rpc.com` |
| `POLY_PROXY_ADDRESS` | Dirección Gnosis Safe / proxy | *opcional* |
| `TEST_MODE` | `true` = simulación, `false` = real | `true` |
| `TRADING_FEE_BPS` | Comisión en puntos básicos | `20` (0.2%) |
| `MIN_ORDER_AMOUNT` | Monto mínimo de orden (USDC) | `1.0` |
| `MAX_ORDER_AMOUNT` | Monto máximo de orden (USDC) | `1000.0` |
| `RUST_LOG` | Nivel de detalle de logs | `info` |

### Tipos de Wallet
| Tipo | `signature_type` | Cuándo |
|------|-------------------|--------|
| EOA | `0` | Sin `POLY_PROXY_ADDRESS` |
| Gnosis Safe | `2` | Con `POLY_PROXY_ADDRESS` configurado |

---

## Instalación

```bash
# Instalar Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Compilar
cd ~/Descargas/polymarketbot
cargo build --release
```

## Uso

```bash
# 1. Configurar
cp .env.example .env   # Editar con tus credenciales

# 2. Ejecutar
./target/release/polymarket_bot_hft

# 3. Ver ranking de estrategias (esperar ~10 min)
curl http://localhost:8000/strategies | python3 -m json.tool

# 4. Ver estrategia específica
curl "http://localhost:8000/trades?strategy=momentum" | python3 -m json.tool

# 5. Trade manual de prueba
curl -X POST http://localhost:8000/trade \
  -H "Content-Type: application/json" \
  -d '{"token_id":"123456","price":0.5,"amount":2.2,"side":"BUY"}'
```

## Archivos CSV

Abre cualquier CSV directamente en Excel o LibreOffice Calc:

| Archivo | Estrategia |
|---------|------------|
| `trades_momentum.csv` | Momentum BTC (3 velas) |
| `trades_rsi.csv` | RSI Sobrecompra/Sobreventa |
| `trades_mean_rev.csv` | Reversión a la Media |
| `trades_liquidity.csv` | Liquidez del Orderbook |
| `trades_manual.csv` | Trades manuales vía API |

Columnas: `timestamp, side, token_id, price, amount, total_usdc, fee, mode, result, profit_loss, outcome`

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

## Futuro: Integración con AI

El sistema de estrategias está diseñado para integración con AI. Para agregar una estrategia con IA:

1. Crear una función en `strategies.rs` que reciba datos del mercado y devuelva `Option<StrategySignal>`
2. Agregarla al vector `signals` en `monitor.rs`
3. La estrategia AI tendrá automáticamente su propio CSV (`trades_ai.csv`), tracking de P&L, y posición en el ranking
