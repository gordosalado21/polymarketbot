//! # Trade Logger - CSV por estrategia con resumen P&L

use std::fs;
use std::path::Path;

/// Registra un trade en el CSV de la estrategia correspondiente
pub fn log_trade(
    strategy: &str, side: &str, token_id: &str, price: f64, amount: f64,
    total_usdc: f64, fee_estimated: f64, mode: &str, result: &str,
    profit_loss: f64, outcome: &str,
) {
    let csv_path = format!("trades_{}.csv", strategy);
    let mut existing: Vec<Vec<String>> = Vec::new();
    if Path::new(&csv_path).exists() {
        if let Ok(mut rdr) = csv::Reader::from_path(&csv_path) {
            for rec in rdr.records().flatten() {
                let row: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
                if row.first().map(|s| s.as_str()) == Some("--- TOTAL ---") { continue; }
                existing.push(row);
            }
        }
    }
    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    existing.push(vec![
        ts, side.into(), token_id.into(),
        format!("{:.6}", price), format!("{:.6}", amount),
        format!("{:.2}", total_usdc), format!("{:.4}", fee_estimated),
        mode.into(), result.into(),
        format!("{:.2}", profit_loss), outcome.into(),
    ]);
    let (mut w, mut l, mut p) = (0u32, 0u32, 0u32);
    let mut total_pl: f64 = 0.0;
    let mut total_inv: f64 = 0.0;
    for row in &existing {
        if let Some(o) = row.get(10) { match o.as_str() { "WIN" => w += 1, "LOSS" => l += 1, "PENDING" => p += 1, _ => {} } }
        if let Some(v) = row.get(9) { total_pl += v.parse::<f64>().unwrap_or(0.0); }
        if let Some(v) = row.get(5) { total_inv += v.parse::<f64>().unwrap_or(0.0); }
    }
    let res = if total_pl > 0.0 { "GANANCIA" } else if total_pl < 0.0 { "PERDIDA" } else { "NEUTRO" };
    let file = match fs::File::create(&csv_path) { Ok(f) => f, Err(e) => { tracing::error!("CSV error: {}", e); return; } };
    let mut wtr = csv::Writer::from_writer(file);
    let _ = wtr.write_record(&["timestamp","side","token_id","price","amount","total_usdc","fee","mode","result","profit_loss","outcome"]);
    for row in &existing { let _ = wtr.write_record(row); }
    let _ = wtr.write_record(&["--- TOTAL ---","","","","", &format!("{:.2}",total_inv),"", &format!("W:{} L:{} P:{}",w,l,p), res, &format!("{:.2}",total_pl), &format!("TOTAL: ${:.2}",total_pl)]);
    let _ = wtr.flush();
}

/// Lee resumen de UNA estrategia
pub fn read_strategy_summary(strategy: &str) -> serde_json::Value {
    let csv_path = format!("trades_{}.csv", strategy);
    if !Path::new(&csv_path).exists() {
        return serde_json::json!({"strategy": strategy, "total_trades":0,"wins":0,"losses":0,"pending":0,"total_profit_loss":"0.00","resultado":"NEUTRO","trades":[]});
    }
    let mut trades = Vec::new();
    let (mut w, mut l, mut p) = (0, 0, 0);
    let mut total_pl: f64 = 0.0;
    let mut rdr = match csv::Reader::from_path(&csv_path) { Ok(r) => r, Err(_) => return serde_json::json!({"strategy":strategy,"total_trades":0,"trades":[]}) };
    let headers: Vec<String> = match rdr.headers() { Ok(h) => h.iter().map(|s| s.to_string()).collect(), Err(_) => return serde_json::json!({"strategy":strategy,"total_trades":0,"trades":[]}) };
    for result in rdr.records() {
        if let Ok(record) = result {
            if record.get(0).unwrap_or("") == "--- TOTAL ---" { continue; }
            let mut trade = serde_json::Map::new();
            for (i, field) in record.iter().enumerate() {
                if let Some(key) = headers.get(i) { trade.insert(key.clone(), serde_json::Value::String(field.to_string())); }
            }
            if let Some(o) = trade.get("outcome").and_then(|v| v.as_str()) { match o { "WIN" => w += 1, "LOSS" => l += 1, "PENDING" => p += 1, _ => {} } }
            if let Some(v) = trade.get("profit_loss").and_then(|v| v.as_str()) { total_pl += v.parse::<f64>().unwrap_or(0.0); }
            trades.push(serde_json::Value::Object(trade));
        }
    }
    let res = if total_pl > 0.0 { "GANANCIA" } else if total_pl < 0.0 { "PERDIDA" } else { "NEUTRO" };
    let win_rate = if w + l > 0 { (w as f64 / (w + l) as f64) * 100.0 } else { 0.0 };
    serde_json::json!({
        "strategy": strategy, "total_trades": trades.len(), "wins": w, "losses": l, "pending": p,
        "win_rate": format!("{:.1}%", win_rate),
        "total_profit_loss": format!("{:.2}", total_pl), "resultado": res, "trades": trades
    })
}

/// Compara TODAS las estrategias, devuelve ranking por P&L
pub fn compare_all_strategies() -> serde_json::Value {
    let strategy_names = vec!["momentum", "rsi", "mean_rev", "liquidity", "manual"];
    let mut summaries: Vec<(f64, serde_json::Value)> = Vec::new();

    for name in &strategy_names {
        let summary = read_strategy_summary(name);
        let pl: f64 = summary["total_profit_loss"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
        summaries.push((pl, summary));
    }

    // Ordenar por P&L descendente (mejor primero)
    summaries.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let ranked: Vec<serde_json::Value> = summaries.into_iter().enumerate().map(|(i, (_, mut s))| {
        if let Some(obj) = s.as_object_mut() {
            obj.insert("rank".to_string(), serde_json::json!(i + 1));
        }
        s
    }).collect();

    let best = ranked.first().and_then(|s| s["strategy"].as_str()).unwrap_or("none");

    serde_json::json!({
        "best_strategy": best,
        "strategies": ranked
    })
}
