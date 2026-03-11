use ethers::types::{Address, U256};
use ethers::signers::Signer;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::AppState;
use crate::models::{TradeRequest, Order};
use crate::auth::build_poly_headers;
use crate::models::GenericResponse;
use crate::trade_logger;
use axum::http::StatusCode;
use axum::Json;

/// Ejecuta o simula una orden de trading en Polymarket.
/// Centraliza la lógica para que pueda ser llamada desde la API o el Monitor.
pub async fn execute_trade_internal(
    state: &AppState,
    payload: TradeRequest,
) -> Result<serde_json::Value, (StatusCode, Json<GenericResponse>)> {
    
    let order_value = payload.amount * payload.price;

    // --- VALIDACIÓN DE REGLAS MONETARIAS ---
    if order_value < state.min_amount {
        tracing::warn!("Monto ${:.2} por debajo del mínimo permitido ${:.2}", order_value, state.min_amount);
        return Err((StatusCode::BAD_REQUEST, Json(GenericResponse {
            status: "error".into(),
            message: format!("Monto mínimo permitido: ${:.2}", state.min_amount),
        })));
    }

    if order_value > state.max_amount {
        tracing::error!("Monto ${:.2} EXCEDE el límite máximo de seguridad ${:.2}", order_value, state.max_amount);
        return Err((StatusCode::BAD_REQUEST, Json(GenericResponse {
            status: "error".into(),
            message: format!("Monto máximo de seguridad excedido: ${:.2}", state.max_amount),
        })));
    }

    let side_int = if payload.side == "BUY" { 0 } else { 1 };
    let token_id_u256 = U256::from_dec_str(&payload.token_id).unwrap();
    
    // --- ESCALADO DE DECIMALES (USDC/CTF usan 6 decimales) ---
    let (maker_amount, taker_amount) = if side_int == 0 { // COMPRA
        (
            U256::from((payload.amount * payload.price * 1_000_000.0) as u64),
            U256::from((payload.amount * 1_000_000.0) as u64)
        )
    } else { // VENTA
        (
            U256::from((payload.amount * 1_000_000.0) as u64),
            U256::from((payload.amount * payload.price * 1_000_000.0) as u64)
        )
    };

    let fee_amount = order_value * (state.fee_bps as f64 / 10000.0);
    
    tracing::info!(
        "[{}] Ejecutando Orden {}: {} unidades de {} @ {} (Total: ${:.2}, Fee est: ${:.4})", 
        if state.test_mode { "SIMULACIÓN" } else { "REAL" },
        payload.side, payload.amount, payload.token_id, payload.price, order_value, fee_amount
    );

    // --- CONSTRUCCIÓN Y FIRMA DE LA ORDEN ---
    let nonce = U256::from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis());
    
    let (maker_address, signature_type) = match state.proxy_address {
        Some(proxy) => (proxy, 2), // POLY_GNOSIS_SAFE
        None => (state.wallet.address(), 0), // EOA
    };

    let order = Order {
        maker: maker_address,
        taker: Address::zero(),
        token_id: token_id_u256,
        maker_amount,
        taker_amount,
        side: side_int,
        expiration: U256::zero(),
        nonce,
        fee_rate_bps: U256::from(state.fee_bps),
        signature_type,
    };

    let signature = state.wallet.sign_typed_data(&order).await
        .map_err(|e| {
            tracing::error!("Fallo al firmar orden EIP-712: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(GenericResponse { status: "error".into(), message: e.to_string() }))
        })?;

    // --- MODO SIMULACIÓN ---
    if state.test_mode {
        tracing::info!("Simulación completada con éxito. Firma: {}", signature);
        trade_logger::log_trade(
            "manual", &payload.side, &payload.token_id, payload.price, payload.amount,
            order_value, fee_amount, "SIMULACION", "OK", 0.0, "N/A",
        );
        return Ok(serde_json::json!({
            "status": "success",
            "mode": "TEST_MODE",
            "message": "Simulación exitosa.",
            "fee_estimated": fee_amount,
            "order_signed": signature.to_string()
        }));
    }

    // --- EJECUCIÓN REAL (POST /orders) ---
    let order_json = serde_json::json!({
        "owner": format!("{:?}", order.maker),
        "taker": format!("{:?}", order.taker),
        "tokenId": order.token_id.to_string(),
        "makerAmount": order.maker_amount.to_string(),
        "takerAmount": order.taker_amount.to_string(),
        "side": side_int,
        "expiration": "0",
        "nonce": order.nonce.to_string(),
        "feeRateBps": state.fee_bps.to_string(),
        "signatureType": signature_type,
        "signature": signature.to_string(),
    });

    let body = serde_json::json!({ "order": order_json, "orderType": "GTC" });
    let serialized_body = serde_json::to_string(&body).unwrap();
    let wallet_addr = ethers::utils::to_checksum(&state.wallet.address(), None);
    let headers = build_poly_headers(&state.auth, "POST", "/orders", Some(&serialized_body), &wallet_addr);

    let res = state.http_client.post("https://clob.polymarket.com/orders")
        .headers(headers)
        .body(serialized_body)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Error de red enviando orden: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(GenericResponse { status: "error".into(), message: e.to_string() }))
        })?;

    let response_data = res.json::<serde_json::Value>().await
        .map_err(|e| {
            tracing::error!("Error parseando respuesta de Polymarket: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(GenericResponse { status: "error".into(), message: e.to_string() }))
        })?;

    let result_status = if response_data.get("errorMsg").is_some() { "ERROR" } else { "OK" };
    trade_logger::log_trade(
        "manual", &payload.side, &payload.token_id, payload.price, payload.amount,
        order_value, fee_amount, "REAL", result_status, 0.0, "N/A",
    );

    Ok(response_data)
}
