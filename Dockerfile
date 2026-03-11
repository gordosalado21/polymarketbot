# ============================================
# Stage 1: Build
# ============================================
FROM rust:1.83-slim-bookworm AS builder

WORKDIR /app

# Instalar dependencias del sistema necesarias para compilar
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copiar manifiestos primero para cachear dependencias
COPY Cargo.toml Cargo.lock ./

# Crear un src dummy para compilar solo dependencias (cache layer)
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# Copiar código fuente real y compilar
COPY src/ src/
RUN touch src/main.rs && cargo build --release

# ============================================
# Stage 2: Runtime (imagen mínima)
# ============================================
FROM debian:bookworm-slim

WORKDIR /app

# Instalar solo las libs necesarias en runtime
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copiar el binario compilado
COPY --from=builder /app/target/release/polymarket_bot_hft /app/polymarket_bot_hft

# Puerto del servidor Axum
EXPOSE 8000

# Ejecutar el bot
CMD ["./polymarket_bot_hft"]
