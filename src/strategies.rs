//! # Estrategias de Trading
//!
//! Módulo con múltiples estrategias que analizan velas BTC de Binance
//! y generan señales de trading independientes.

/// Señal generada por una estrategia
pub struct StrategySignal {
    pub strategy_name: String,
    pub signal: String,       // "COMPRAR_YES", "COMPRAR_NO", "ESPERAR"
    pub confidence: f64,      // 0.0 - 1.0
    pub btc_price: f64,
    pub detail: String,       // Descripción del indicador
}

/// 1. MOMENTUM — 3 velas consecutivas en la misma dirección + diff > umbral
pub fn run_momentum(klines: &[serde_json::Value], umbral: f64) -> Option<StrategySignal> {
    if klines.len() < 3 { return None; }

    let btc_price = parse_close(klines.last()?);
    let mut diff_acumulada = 0.0;
    let mut todas_up = true;
    let mut todas_down = true;

    for kline in klines.iter().rev().take(3) {
        let open = parse_open(kline);
        let close = parse_close(kline);
        let diff = close - open;
        diff_acumulada += diff;
        if diff <= 0.0 { todas_up = false; }
        if diff >= 0.0 { todas_down = false; }
    }

    if todas_up && diff_acumulada.abs() >= umbral {
        Some(StrategySignal {
            strategy_name: "momentum".into(),
            signal: "COMPRAR_YES".into(),
            confidence: (diff_acumulada.abs() / umbral).min(1.0),
            btc_price,
            detail: format!("3 velas UP, diff: ${:.2}", diff_acumulada),
        })
    } else if todas_down && diff_acumulada.abs() >= umbral {
        Some(StrategySignal {
            strategy_name: "momentum".into(),
            signal: "COMPRAR_NO".into(),
            confidence: (diff_acumulada.abs() / umbral).min(1.0),
            btc_price,
            detail: format!("3 velas DOWN, diff: ${:.2}", diff_acumulada),
        })
    } else {
        None
    }
}

/// 2. RSI — Relative Strength Index sobre las velas disponibles
/// RSI > 70 → overbought → COMPRAR_NO (espera corrección bajista)
/// RSI < 30 → oversold → COMPRAR_YES (espera rebote alcista)
pub fn run_rsi(klines: &[serde_json::Value]) -> Option<StrategySignal> {
    if klines.len() < 3 { return None; }

    let btc_price = parse_close(klines.last()?);
    let mut gains = 0.0;
    let mut losses = 0.0;
    let mut count = 0u32;

    for i in 1..klines.len() {
        let prev_close = parse_close(&klines[i - 1]);
        let curr_close = parse_close(&klines[i]);
        let change = curr_close - prev_close;
        if change > 0.0 {
            gains += change;
        } else {
            losses += change.abs();
        }
        count += 1;
    }

    if count == 0 { return None; }

    let avg_gain = gains / count as f64;
    let avg_loss = losses / count as f64;

    let rsi = if avg_loss == 0.0 {
        100.0
    } else {
        let rs = avg_gain / avg_loss;
        100.0 - (100.0 / (1.0 + rs))
    };

    if rsi > 70.0 {
        Some(StrategySignal {
            strategy_name: "rsi".into(),
            signal: "COMPRAR_NO".into(),
            confidence: ((rsi - 70.0) / 30.0).min(1.0),
            btc_price,
            detail: format!("RSI: {:.1} (overbought >70)", rsi),
        })
    } else if rsi < 30.0 {
        Some(StrategySignal {
            strategy_name: "rsi".into(),
            signal: "COMPRAR_YES".into(),
            confidence: ((30.0 - rsi) / 30.0).min(1.0),
            btc_price,
            detail: format!("RSI: {:.1} (oversold <30)", rsi),
        })
    } else {
        None
    }
}

/// 3. MEAN REVERSION — Precio actual vs promedio de N velas
/// Si precio > promedio + umbral → espera corrección → COMPRAR_NO
/// Si precio < promedio - umbral → espera rebote → COMPRAR_YES
pub fn run_mean_reversion(klines: &[serde_json::Value], umbral: f64) -> Option<StrategySignal> {
    if klines.len() < 3 { return None; }

    let btc_price = parse_close(klines.last()?);
    
    let sum: f64 = klines.iter().map(|k| parse_close(k)).sum();
    let avg = sum / klines.len() as f64;
    let deviation = btc_price - avg;

    if deviation > umbral {
        Some(StrategySignal {
            strategy_name: "mean_rev".into(),
            signal: "COMPRAR_NO".into(),
            confidence: (deviation / umbral / 2.0).min(1.0),
            btc_price,
            detail: format!("Precio ${:.0} > Avg ${:.0} + ${:.0}", btc_price, avg, umbral),
        })
    } else if deviation < -umbral {
        Some(StrategySignal {
            strategy_name: "mean_rev".into(),
            signal: "COMPRAR_YES".into(),
            confidence: (deviation.abs() / umbral / 2.0).min(1.0),
            btc_price,
            detail: format!("Precio ${:.0} < Avg ${:.0} - ${:.0}", btc_price, avg, umbral),
        })
    } else {
        None
    }
}

// Helpers para parsear velas
fn parse_open(kline: &serde_json::Value) -> f64 {
    kline[1].as_str().unwrap_or("0").parse().unwrap_or(0.0)
}

fn parse_close(kline: &serde_json::Value) -> f64 {
    kline[4].as_str().unwrap_or("0").parse().unwrap_or(0.0)
}

// ══════════════════════════════════════
// ANÁLISIS DE LIQUIDEZ (Orderbook)
// ══════════════════════════════════════

/// Información de liquidez extraída del orderbook
#[derive(Debug, Clone)]
pub struct LiquidityInfo {
    pub best_bid: f64,      // Mejor precio de compra
    pub best_ask: f64,      // Mejor precio de venta
    pub mid_price: f64,     // Precio medio
    pub spread: f64,        // Diferencia ask - bid
    pub spread_pct: f64,    // Spread como % del mid price
    pub bid_depth: f64,     // Volumen total en bids (USD)
    pub ask_depth: f64,     // Volumen total en asks (USD)
    pub total_depth: f64,   // Liquidez total
    pub liquidity_score: f64, // 0-100 score de liquidez
}

/// Analiza el orderbook de un token de Polymarket
pub async fn analyze_orderbook(
    http_client: &reqwest::Client,
    token_id: &str,
) -> Option<LiquidityInfo> {
    let url = format!("https://clob.polymarket.com/book?token_id={}", token_id);
    let res = http_client.get(&url).send().await.ok()?;
    let book: serde_json::Value = res.json().await.ok()?;

    let bids = book["bids"].as_array()?;
    let asks = book["asks"].as_array()?;

    if bids.is_empty() || asks.is_empty() {
        return None;
    }

    let best_bid: f64 = bids[0]["price"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
    let best_ask: f64 = asks[0]["price"].as_str().unwrap_or("0").parse().unwrap_or(0.0);

    if best_bid == 0.0 || best_ask == 0.0 {
        return None;
    }

    let mid_price = (best_bid + best_ask) / 2.0;
    let spread = best_ask - best_bid;
    let spread_pct = if mid_price > 0.0 { (spread / mid_price) * 100.0 } else { 100.0 };

    // Calcular profundidad (depth) = suma de price * size en cada lado
    let bid_depth: f64 = bids.iter().map(|b| {
        let p: f64 = b["price"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
        let s: f64 = b["size"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
        p * s
    }).sum();

    let ask_depth: f64 = asks.iter().map(|a| {
        let p: f64 = a["price"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
        let s: f64 = a["size"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
        p * s
    }).sum();

    let total_depth = bid_depth + ask_depth;

    // Score de liquidez (0-100):
    // - Spread bajo → mejor score
    // - Mayor profundidad → mejor score
    let spread_score = (1.0 - (spread_pct / 10.0).min(1.0)) * 50.0; // Max 50 pts si spread < 1%
    let depth_score = (total_depth / 1000.0).min(1.0) * 50.0;  // Max 50 pts si depth > $1000
    let liquidity_score = spread_score + depth_score;

    Some(LiquidityInfo {
        best_bid,
        best_ask,
        mid_price,
        spread,
        spread_pct,
        bid_depth,
        ask_depth,
        total_depth,
        liquidity_score,
    })
}

/// 4. LIQUIDITY STRATEGY — Señal basada en desequilibrio del orderbook
/// Si bid_depth >> ask_depth → presión compradora → COMPRAR_YES
/// Si ask_depth >> bid_depth → presión vendedora → COMPRAR_NO
pub fn run_liquidity_strategy(liquidity: &LiquidityInfo, btc_price: f64) -> Option<StrategySignal> {
    // Solo operar si hay liquidez mínima
    if liquidity.total_depth < 50.0 || liquidity.liquidity_score < 20.0 {
        return None;
    }

    let ratio = if liquidity.ask_depth > 0.0 {
        liquidity.bid_depth / liquidity.ask_depth
    } else {
        return None;
    };

    // Ratio > 1.5 = bids dominan (más compradores)
    // Ratio < 0.67 = asks dominan (más vendedores)
    if ratio > 1.5 {
        Some(StrategySignal {
            strategy_name: "liquidity".into(),
            signal: "COMPRAR_YES".into(),
            confidence: ((ratio - 1.0) / 2.0).min(1.0),
            btc_price,
            detail: format!("Bid/Ask:{:.2} Depth:${:.0} Spread:{:.1}%", ratio, liquidity.total_depth, liquidity.spread_pct),
        })
    } else if ratio < 0.67 {
        Some(StrategySignal {
            strategy_name: "liquidity".into(),
            signal: "COMPRAR_NO".into(),
            confidence: ((1.0 / ratio - 1.0) / 2.0).min(1.0),
            btc_price,
            detail: format!("Bid/Ask:{:.2} Depth:${:.0} Spread:{:.1}%", ratio, liquidity.total_depth, liquidity.spread_pct),
        })
    } else {
        None
    }
}
