//! # Polymarket HFT Bot - Punto de Entrada
//!
//! Este es el archivo principal que coordina el arranque, la configuración
//! y la ejecución del bot de trading de alta frecuencia para Polymarket.
//!
//! El bot utiliza una arquitectura modular:
//! - `auth`: Gestión de seguridad L2 y firmas HMAC.
//! - `handlers`: Lógica de los endpoints de la API.
//! - `models`: Estructuras de datos y tipos EIP-712.
//! - `monitor`: Tarea en segundo plano para resolución de mercados.

mod auth;
mod handlers;
mod models;
mod monitor;
mod strategies;
mod trade_logger;
mod trading;

use axum::{
    routing::{get, post},
    Router,
};
use ethers::providers::{Provider, Http};
use ethers::signers::{LocalWallet, Signer};
use reqwest::Client;
use std::{
    collections::HashSet,
    env,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth::PolyAuth;
use handlers::{get_balance, get_resultado, get_strategies, get_trades, get_wallet, post_trade};
use monitor::monitor_trades;

// ─────────────────────────────────────────────
// ESTADO GLOBAL
// ─────────────────────────────────────────────

/// Representa el estado compartido accesible por todos los manejadores de la API.
#[derive(Clone)]
pub struct AppState {
    /// Cliente HTTP optimizado con pool de conexiones persistentes.
    pub http_client: Client,
    /// Proveedor de RPC para la red Polygon (on-chain).
    pub provider: Arc<Provider<Http>>,
    /// Billetera Ethereum (LocalWallet) precargada con la clave privada.
    pub wallet: LocalWallet,
    /// Credenciales L2 para la autenticación en el CLOB.
    pub auth: PolyAuth,
    /// Interruptor para operar en modo simulación (true) o real (false).
    pub test_mode: bool,
    /// Tasa de comisión estimada en puntos básicos (ej: 20 = 0.2%).
    pub fee_bps: u64,
    /// Monto mínimo de seguridad para una orden en USDC.
    pub min_amount: f64,
    /// Monto máximo de seguridad para una orden en USDC.
    pub max_amount: f64,
    /// Dirección de la Proxy Wallet de Polymarket.
    pub proxy_address: Option<ethers::types::Address>,
    /// Caché de mercados ya operados para evitar duplicados.
    pub notified_slugs: Arc<Mutex<HashSet<String>>>,
}

/// Función principal que arranca el bot.
///
/// Realiza las siguientes tareas secuenciales:
/// 1. Carga las variables de entorno desde el archivo `.env`.
/// 2. Inicializa el sistema de logs `tracing`.
/// 3. Configura la wallet y las credenciales del CLOB.
/// 4. Crea el cliente HTTP con parámetros de rendimiento (timeouts, pools).
/// 5. Inicia el monitor en segundo plano (`monitor_trades`).
/// 6. Levanta el servidor web Axum en el puerto 8000.
#[tokio::main]
async fn main() {
    // 1. Cargar variables de entorno
    dotenvy::dotenv().ok();

    // 2. Inicializar Logging (Tracing) profesional con soporte para RUST_LOG
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "polymarket_bot_hft=info,axum=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("🚀 Iniciando Polymarket HFT Bot (Versión Modular & Documentada)...");

    // 3. Configuración de Wallet y Credenciales
    let chain_id: u64 = 137; // Polygon Mainnet
    let pk = env::var("POLYMARKET_PRIVATE_KEY").expect("POLYMARKET_PRIVATE_KEY no configurada");
    let wallet = pk.parse::<LocalWallet>().expect("Clave Privada inválida").with_chain_id(chain_id);
    
    let auth = PolyAuth {
        api_key: env::var("CLOB_API_KEY").expect("CLOB_API_KEY no configurada"),
        secret: env::var("CLOB_SECRET").expect("CLOB_SECRET no configurada"),
        passphrase: env::var("CLOB_PASSPHRASE").expect("CLOB_PASSPHRASE no configurada"),
    };

    // 4. Parámetros de Operación y Seguridad
    let test_mode = env::var("TEST_MODE").unwrap_or_else(|_| "true".to_string()) == "true";
    let fee_bps = env::var("TRADING_FEE_BPS").unwrap_or_else(|_| "20".to_string()).parse::<u64>().unwrap_or(20);
    let min_amount = env::var("MIN_ORDER_AMOUNT").unwrap_or_else(|_| "5.0".to_string()).parse::<f64>().unwrap_or(5.0);
    let max_amount = env::var("MAX_ORDER_AMOUNT").unwrap_or_else(|_| "1000.0".to_string()).parse::<f64>().unwrap_or(1000.0);

    // 5. Cliente HTTP optimizado para HFT (mantiene conexiones calientes)
    let http_client = Client::builder()
        .pool_idle_timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(200)
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // 6. Configuración de Proveedor RPC para Polygon
    let rpc_url = env::var("POLYGON_RPC_URL").unwrap_or_else(|_| "https://polygon-rpc.com".to_string());
    let provider = Arc::new(Provider::<Http>::try_from(rpc_url).expect("URL de RPC inválida"));

    let app_state = AppState {
        http_client,
        provider,
        wallet,
        auth,
        test_mode,
        fee_bps,
        min_amount,
        max_amount,
        proxy_address: env::var("POLY_PROXY_ADDRESS").ok().map(|s| s.parse().expect("Proxy Address inválida")),
        notified_slugs: Arc::new(Mutex::new(HashSet::new())),
    };

    tracing::info!("🔧 Modo: {}", if test_mode { "TESTEO (Simulación)" } else { "PRODUCCIÓN (Real)" });
    tracing::info!("💰 Configuración: Fee {} bps | Min ${:.2} | Max ${:.2}", fee_bps, min_amount, max_amount);

    // 6. Lanzar monitor de mercado en una tarea asíncrona dedicada
    tokio::spawn(monitor_trades(app_state.clone()));

    // 7. Configuración de Rutas de la API
    let app = Router::new()
        .route("/wallet", get(get_wallet))       // Consultar dirección pública
        .route("/balance", get(get_balance))     // Consultar capital disponible
        .route("/trade", post(post_trade))       // Ejecutar transacciones
        .route("/trades", get(get_trades))       // Ver historial de trades (por estrategia)
        .route("/strategies", get(get_strategies)) // Ranking de estrategias
        .route("/resultado", get(get_resultado)) // Consultar mercados ad-hoc
        .with_state(app_state);

    // 8. Iniciar el servidor HTTP
    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
    tracing::info!("📡 Escuchando en http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
