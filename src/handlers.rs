//! # Manejadores de API (Handlers)
//!
//! Este módulo contiene las funciones que procesan las peticiones HTTP
//! entrantes y coordinan la ejecución de trades y consultas de balance.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use ethers::types::Address;
use ethers::signers::Signer;
// use std::time::{SystemTime, UNIX_EPOCH};

use crate::AppState;
use crate::models::{TradeRequest, BalanceResponse, GenericResponse, ResultadoQuery};
use crate::auth::build_poly_headers;
use crate::trading::execute_trade_internal;

// ABI mínimo para balanceOf (USDC/ERC20)
ethers::prelude::abigen!(
    IERC20,
    r#"[
        function balanceOf(address account) external view returns (uint256)
    ]"#,
);

pub async fn get_wallet(State(state): State<AppState>) -> Json<serde_json::Value> {
    tracing::debug!("Solicitud de dirección de billetera");
    Json(serde_json::json!({ "address": ethers::utils::to_checksum(&state.wallet.address(), None) }))
}

/// Endpoint GET `/balance`: Consulta el balance real de USDC y permisos en el CLOB.
///
/// Llama al endpoint `/balance-allowance` de Polymarket usando autenticación HMAC L2
/// e interacciona directamente con la red Polygon para obtener el saldo on-chain.
pub async fn get_balance(State(state): State<AppState>) -> Result<Json<BalanceResponse>, (StatusCode, Json<GenericResponse>)> {
    tracing::info!("Consultando balances (Exchange + Wallet)...");
    
    // 1. Balance en el Exchange (CLOB)
    // IMPORTANT: HMAC signs ONLY the base path, query params go in URL only
    let sign_path = "/balance-allowance";
    let signature_type = if state.proxy_address.is_some() { 2 } else { 0 };
    let query_url = format!("https://clob.polymarket.com/balance-allowance?asset_type=COLLATERAL&signature_type={}", signature_type);
    let wallet_addr = ethers::utils::to_checksum(&state.wallet.address(), None);
    let headers = build_poly_headers(&state.auth, "GET", sign_path, None, &wallet_addr);
    
    let clob_res = state.http_client.get(&query_url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Error al consultar balance CLOB: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(GenericResponse { status: "error".into(), message: e.to_string() }))
        })?;

    let clob_status = clob_res.status();
    let clob_text = clob_res.text().await.unwrap_or_default();
    tracing::info!("📊 CLOB Response Status: {} | Body: {}", clob_status, clob_text);

    let clob_data: serde_json::Value = serde_json::from_str(&clob_text).unwrap_or_default();
    let clob_balance = clob_data["balance"].as_str().unwrap_or("0").parse::<f64>().unwrap_or(0.0) / 1_000_000.0;

    // 2. Balance On-Chain (Polygon)
    // Revisamos tanto USDC.e (bridged) como USDC (nativo)
    let target_address = state.proxy_address.unwrap_or_else(|| state.wallet.address());
    let usdc_e_address: Address = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174".parse().unwrap();
    let usdc_native_address: Address = "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359".parse().unwrap();
    
    let contract_e = IERC20::new(usdc_e_address, state.provider.clone());
    let contract_native = IERC20::new(usdc_native_address, state.provider.clone());

    let balance_e = contract_e.balance_of(target_address).call().await.unwrap_or_default();
    let balance_native = contract_native.balance_of(target_address).call().await.unwrap_or_default();

    let wallet_balance = (balance_e.as_u128() as f64 + balance_native.as_u128() as f64) / 1_000_000.0;
    
    tracing::info!("Balances: Exchange ${:.2} | Wallet ${:.2}", clob_balance, wallet_balance);

    Ok(Json(BalanceResponse {
        clob_balance,
        wallet_balance,
        shares: 0.0,
    }))
}

/// Endpoint POST `/trade`: Ejecuta o simula una orden de trading.
///
/// Esta función realiza los siguientes pasos críticos:
/// 1. Valida los límites monetarios configurados en el `.env`.
/// 2. Construye y firma una orden **EIP-712** nativa.
/// 3. Si `TEST_MODE=true`, devuelve la simulación.
/// 4. Si `TEST_MODE=false`, envía la orden autenticada al CLOB de Polymarket.
pub async fn post_trade(
    State(state): State<AppState>,
    Json(payload): Json<TradeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<GenericResponse>)> {
    let res = execute_trade_internal(&state, payload).await?;
    Ok(Json(res))
}

/// Endpoint GET `/resultado`: Consulta directa del estado de unSlug vía Gamma API.
pub async fn get_resultado(
    State(state): State<AppState>,
    Query(query): Query<ResultadoQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<GenericResponse>)> {
    tracing::debug!("Consultando resultado para slug: {}", query.slug);
    let url = format!("https://gamma-api.polymarket.com/events?slug={}", query.slug);
    let res = state.http_client.get(&url).send().await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(GenericResponse { status: "error".into(), message: "Error en Gamma API".into() })))?;
    let json_data = res.json::<serde_json::Value>().await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(GenericResponse { status: "error".into(), message: "Fallo de parseo".into() })))?;
    Ok(Json(json_data))
}

/// Query param para filtrar por estrategia
#[derive(Deserialize)]
pub struct StrategyQuery {
    pub strategy: Option<String>,
}

/// Endpoint GET `/trades?strategy=momentum` — historial de una estrategia o todas
pub async fn get_trades(Query(query): Query<StrategyQuery>) -> Json<serde_json::Value> {
    if let Some(strategy) = query.strategy {
        Json(crate::trade_logger::read_strategy_summary(&strategy))
    } else {
        Json(crate::trade_logger::compare_all_strategies())
    }
}

/// Endpoint GET `/strategies` — ranking de todas las estrategias por P&L
pub async fn get_strategies() -> Json<serde_json::Value> {
    Json(crate::trade_logger::compare_all_strategies())
}
