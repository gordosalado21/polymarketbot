//! # Monitor Multi-Estrategia
//!
//! Ejecuta Momentum, RSI, y Mean Reversion simultáneamente.
//! Cada estrategia tiene su propio CSV y tracking de P&L.

use crate::AppState;
use crate::strategies;
use crate::trade_logger;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Trade pendiente por estrategia
struct PendingTrade {
    strategy: String,
    signal: String,
    btc_entry: f64,
    entry_time: u64,
    slug: String,
    amount_usdc: f64,
}

pub async fn monitor_trades(state: AppState) {
    tracing::info!("🔍 Monitor MULTI-ESTRATEGIA iniciado (Momentum + RSI + Mean Reversion)");

    let mut pending_trades: HashMap<String, PendingTrade> = HashMap::new();

    loop {
        let btc_res = state.http_client
            .get("https://api.binance.com/api/v3/klines")
            .query(&[("symbol", "BTCUSDT"), ("interval", "5m"), ("limit", "14")])
            .send()
            .await;

        if let Ok(response) = btc_res {
            if let Ok(klines) = response.json::<Vec<serde_json::Value>>().await {
                if klines.len() >= 3 {
                    let ahora = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                    let btc_price = klines.last()
                        .and_then(|k| k[4].as_str()).unwrap_or("0")
                        .parse::<f64>().unwrap_or(0.0);

                    // ══════════════════════════════════════
                    // RESOLVER TRADES PENDIENTES (5 min)
                    // ══════════════════════════════════════
                    let resolved: Vec<String> = pending_trades.iter()
                        .filter(|(_, t)| ahora - t.entry_time >= 300)
                        .map(|(k, _)| k.clone()).collect();

                    for key in resolved {
                        if let Some(trade) = pending_trades.remove(&key) {
                            let btc_diff = btc_price - trade.btc_entry;
                            let win = match trade.signal.as_str() {
                                "COMPRAR_YES" => btc_diff > 0.0,
                                "COMPRAR_NO" => btc_diff < 0.0,
                                _ => false,
                            };
                            let fee = trade.amount_usdc * (state.fee_bps as f64 / 10000.0);
                            let profit_loss = if win {
                                trade.amount_usdc - fee  // Ganancia neta
                            } else {
                                -(trade.amount_usdc + fee)  // Pérdida
                            };
                            let outcome = if win { "WIN" } else { "LOSS" };
                            tracing::info!("📊 [{}] RESUELTO: {} | BTC ${:.0}→${:.0} | {} | P&L: ${:.2}",
                                trade.strategy.to_uppercase(), trade.signal, trade.btc_entry, btc_price, outcome, profit_loss);
                            trade_logger::log_trade(
                                &trade.strategy, "BUY", &format!("RESOLVED_{}", trade.slug),
                                0.50, trade.amount_usdc / 0.50, trade.amount_usdc, fee,
                                "RESUELTO", &format!("{}_BTC_{:.0}_to_{:.0}", trade.signal, trade.btc_entry, btc_price),
                                profit_loss, outcome,
                            );
                        }
                    }

                    // ══════════════════════════════════════
                    // EJECUTAR TODAS LAS ESTRATEGIAS
                    // ══════════════════════════════════════
                    let mut signals: Vec<Option<strategies::StrategySignal>> = vec![
                        strategies::run_momentum(&klines, 100.0),
                        strategies::run_rsi(&klines),
                        strategies::run_mean_reversion(&klines, 150.0),
                    ];

                    // Buscar un mercado BTC real para analizar liquidez
                    let ahora_iv = (ahora / 300) * 300;
                    let liq_slug = format!("btc-updown-5m-{}", ahora_iv);
                    let gamma_url = format!("https://gamma-api.polymarket.com/events?slug={}", liq_slug);
                    if let Ok(gamma_res) = state.http_client.get(&gamma_url).send().await {
                        if let Ok(data) = gamma_res.json::<Vec<serde_json::Value>>().await {
                            if !data.is_empty() && data[0]["markets"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                                let mercado = &data[0]["markets"][0];
                                let token_ids: Vec<String> = serde_json::from_str(mercado["clobTokenIds"].as_str().unwrap_or("[]")).unwrap_or_default();
                                if let Some(yes_token) = token_ids.first() {
                                    // Analizar orderbook del token YES
                                    if let Some(liq) = strategies::analyze_orderbook(&state.http_client, yes_token).await {
                                        tracing::info!("📊 LIQUIDEZ: Bid:${:.0} Ask:${:.0} Spread:{:.1}% Depth:${:.0} Score:{:.0}",
                                            liq.bid_depth, liq.ask_depth, liq.spread_pct, liq.total_depth, liq.liquidity_score);
                                        // Agregar estrategia de liquidez
                                        signals.push(strategies::run_liquidity_strategy(&liq, btc_price));
                                    }
                                }
                            }
                        }
                    }

                    for signal_opt in signals {
                        if let Some(signal) = signal_opt {
                            let inicio_intervalo = (ahora / 300) * 300;
                            let target_slug = format!("btc-5m-{}", inicio_intervalo);
                            let clave = format!("{}_{}", signal.strategy_name, target_slug);

                            // Verificar si ya operamos este intervalo con esta estrategia
                            if pending_trades.contains_key(&clave) {
                                continue;
                            }
                            let ya_operado = {
                                let cache = state.notified_slugs.lock().await;
                                cache.contains(&clave)
                            };
                            if ya_operado { continue; }

                            tracing::info!("📈 [{}] SEÑAL: {} | BTC: ${:.2} | {} | Conf: {:.0}%",
                                signal.strategy_name.to_uppercase(),
                                signal.signal, signal.btc_price, signal.detail,
                                signal.confidence * 100.0);

                            if state.test_mode {
                                let amt = 3.0;
                                tracing::info!("📝 [{}] Señal registrada → se resolverá en 5 min",
                                    signal.strategy_name.to_uppercase());

                                pending_trades.insert(clave.clone(), PendingTrade {
                                    strategy: signal.strategy_name,
                                    signal: signal.signal,
                                    btc_entry: btc_price,
                                    entry_time: ahora,
                                    slug: target_slug,
                                    amount_usdc: amt,
                                });
                            }

                            state.notified_slugs.lock().await.insert(clave);
                        }
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
