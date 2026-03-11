//! # Modelos de datos para el Bot de Polymarket
//!
//! Este módulo define las estructuras de datos utilizadas para la comunicación con la API
//! de Polymarket CLOB y la representación interna de órdenes EIP-712.

use ethers::types::{Address, U256};
use ethers::contract::{Eip712, EthAbiCodec, EthAbiType};
use serde::{Deserialize, Serialize};

/// Estructura de una Orden compatible con EIP-712 para el ClobExchange de Polymarket.
///
/// Esta estructura debe coincidir exactamente con la definición del contrato inteligente
/// para que la firma sea válida.
#[derive(EthAbiType, EthAbiCodec, Eip712, Clone, Debug, Serialize)]
#[eip712(
    name = "ClobExchange",
    version = "1",
    chain_id = 137,
    verifying_contract = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E"
)]
pub struct Order {
    /// Dirección del creador de la orden (tu wallet).
    pub maker: Address,
    /// Dirección del tomador (generalmente Address::zero() para órdenes abiertas).
    pub taker: Address,
    /// ID único del token del mercado (obtenido de la API de Polymarket).
    pub token_id: U256,
    /// Cantidad que el creador ofrece.
    pub maker_amount: U256,
    /// Cantidad que el creador espera recibir.
    pub taker_amount: U256,
    /// Lado de la operación (0: COMPRA, 1: VENTA).
    pub side: u8,
    /// Tiempo de expiración de la orden (0 para GTC - Good Til Cancelled).
    pub expiration: U256,
    /// Número único para evitar ataques de repetición (timestamp en ms).
    pub nonce: U256,
    /// Tasa de comisión en puntos básicos (transferida desde el .env).
    pub fee_rate_bps: U256,
    /// Tipo de firma (1 para EOA - Externally Owned Account).
    pub signature_type: u8,
}

/// Petición JSON recibida para ejecutar un trade.
#[derive(Deserialize)]
pub struct TradeRequest {
    /// ID del token del mercado.
    pub token_id: String,
    /// Precio límite de la operación.
    pub price: f64,
    /// Cantidad de acciones (shares) a operar.
    pub amount: f64,
    /// Lado de la operación ("BUY" o "SELL").
    #[serde(default = "default_side")]
    pub side: String,
}

/// Valor por defecto para el lado de la operación.
fn default_side() -> String {
    "BUY".to_string()
}

/// Respuesta con los detalles del balance de la cuenta.
#[derive(Serialize)]
pub struct BalanceResponse {
    /// Balance disponible en USDC dentro del exchange (escalado a 6 decimales).
    pub clob_balance: f64,
    /// Balance real de USDC en la billetera on-chain.
    pub wallet_balance: f64,
    /// Cantidad de acciones en el mercado (reservado para uso futuro).
    pub shares: f64,
}

/// Parámetros de consulta para obtener resultados de un mercado.
#[derive(Deserialize)]
pub struct ResultadoQuery {
    /// El "slug" identificador del mercado.
    pub slug: String,
}

/// Estructura genérica para respuestas de error o éxito.
#[derive(Serialize, Debug)]
pub struct GenericResponse {
    /// Estado de la operación ("success" o "error").
    pub status: String,
    /// Mensaje descriptivo.
    pub message: String,
}
