#!/bin/bash
set -e

echo "🚀 Iniciando la instalación de dependencias para el Polymarket HFT Bot en Ubuntu..."

# 1. Actualizar repositorios
echo "📦 Actualizando listas de paquetes del sistema..."
sudo apt-get update -y
sudo apt-get upgrade -y

# 2. Instalar herramientas de sistema requeridas por Rust y sus librerías criptográficas
# build-essential: Compilador C/C++ (el equivalente al 'link.exe' de Windows que te tiró error)
# pkg-config: Ayuda a Rust a encontrar las librerías del sistema
# libssl-dev: Requerido obligatoriamente por 'reqwest' (HTTP) y 'ethers' (Criptografía de Ethereum)
echo "🛠️ Instalando compiladores C/C++, pkg-config y OpenSSL (libssl-dev)..."
sudo apt-get install -y build-essential pkg-config libssl-dev curl

# 3. Instalar Rust a través de Rustup
echo "🦀 Instalando el entorno de Rust (rustup, rustc y cargo)..."
if ! command -v cargo &> /dev/null
then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    
    # Cargar las variables de entorno de Rust inmediatamente en esta sesión
    source "$HOME/.cargo/env"
    echo "✅ Entorno de Rust instalado exitosamente."
else
    echo "✅ Rust ya se encuentra instalado. Verificando actualizaciones..."
    rustup update
fi

echo ""
echo "=========================================================="
echo "✨ INSTALACIÓN COMPLETA ✨"
echo "=========================================================="
echo "Tu servidor Ubuntu ahora tiene todo el software nativo "
echo "necesario para compilar el Bot de Alta Frecuencia."
echo ""
echo "⚠️  IMPORTANTE: Escribe el siguiente comando en la terminal"
echo "para activar Rust, o reinicia tu sesión SSH:"
echo "   source \$HOME/.cargo/env"
echo ""
echo "🎯 Siguiente paso: Navega a la carpeta del bot y ejecuta:"
echo "   cargo run --bin test_api"
echo "=========================================================="
