//! # Autenticación y Seguridad
//!
//! Este módulo gestiona la generación de firmas HMAC y las cabeceras requeridas
//! para el Nivel 2 (L2) de la API de Polymarket.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};

/// Credenciales necesarias para la autenticación en el CLOB de Polymarket.
#[derive(Clone)]
pub struct PolyAuth {
    /// API Key generada en el dashboard de Polymarket.
    pub api_key: String,
    /// Secreto de la API (en formato Base64).
    pub secret: String,
    /// Frase de paso configurada durante la creación de la API Key.
    pub passphrase: String,
}

/// Construye las cabeceras HTTP necesarias para una petición autenticada L2.
///
/// La firma se genera concatenando `timestamp + method + path + body` y 
/// firmando el resultado con el secreto de la API usando HMAC-SHA256.
///
/// # Argumentos
/// * `auth` - Las credenciales del cliente.
/// * `method` - Método HTTP (GET, POST, etc.).
/// * `path` - La ruta exacta del endpoint (ej: "/orders").
/// * `body` - El cuerpo de la petición serializado (si existe).
pub fn build_poly_headers(
    auth: &PolyAuth,
    method: &str,
    path: &str,
    body: Option<&str>,
    address: &str,
) -> HeaderMap {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();

    // Mensaje para el HMAC: timestamp + METHOD + path + body
    let message = format!("{}{}{}{}", timestamp, method, path, body.unwrap_or(""));
    
    // Decodificar el secreto de Base64 URL_SAFE
    let decoded_secret = URL_SAFE.decode(&auth.secret).unwrap_or_else(|e| {
        tracing::error!("ERROR CRÍTICO: CLOB_SECRET no es Base64 válido: {}", e);
        vec![]
    });

    // Generar firma HMAC-SHA256
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&decoded_secret).expect("HMAC acepta llaves de cualquier tamaño");
    mac.update(message.as_bytes());
    let result = mac.finalize();
    let signature = URL_SAFE.encode(result.into_bytes());

    let mut headers = HeaderMap::new();
    headers.insert("POLY_ADDRESS", HeaderValue::from_str(address).unwrap());
    headers.insert("POLY_API_KEY", HeaderValue::from_str(&auth.api_key).unwrap());
    headers.insert("POLY_SIGNATURE", HeaderValue::from_str(&signature).unwrap());
    headers.insert("POLY_PASSPHRASE", HeaderValue::from_str(&auth.passphrase).unwrap());
    headers.insert("POLY_TIMESTAMP", HeaderValue::from_str(&timestamp).unwrap());
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    
    headers
}
