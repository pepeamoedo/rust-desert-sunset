# Instrucciones de Compilación y Ejecución (Nubes - Volumetric Raymarching)

Este proyecto está diseñado para funcionar de manera dual: como un ejecutable nativo de alto rendimiento para macOS y como una aplicación WebAssembly (Wasm) para navegadores.

## Requisitos Previos

1. **Rust y Cargo**: Asegúrate de tener instalada la última versión estable de Rust.
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **Dependencias para WebAssembly**:
   Necesitas instalar el target de WebAssembly y la herramienta `wasm-pack`.
   ```bash
   rustup target add wasm32-unknown-unknown
   cargo install wasm-pack
   ```
3. **Servidor HTTP simple** (para probar la versión web):
   Puedes usar `python`, `http-server` (npm) o `miniserve` (cargo).
   ```bash
   cargo install miniserve
   ```

---

## A) Compilar y Servir para Navegador (Prueba de Estrés WebGPU)

1. **Compilar con `wasm-pack`**:
   Desde la raíz del proyecto (`/Users/pepeamoedo/.gemini/antigravity-ide/scratch/nubes`), ejecuta:
   ```bash
   wasm-pack build --target web
   ```
   Esto compilará el código de Rust a WebAssembly y generará los enlaces JS en la carpeta `pkg`.

2. **Servir la Aplicación**:
   Inicia un servidor HTTP en el directorio raíz.
   Usando `miniserve`:
   ```bash
   miniserve . --index index.html
   ```
   Usando `python`:
   ```bash
   python3 -m http.server 8080
   ```

3. **Ejecutar en el Navegador**:
   Abre Chrome o Edge (WebGPU habilitado) y navega a `http://localhost:8080`.

---

## B) Compilar el Ejecutable Nativo para macOS

El target `aarch64-apple-darwin` es el predeterminado en Macs con procesadores Apple Silicon (M1/M2/M3).

1. **Compilar y Ejecutar en modo Release (Optimizado)**:
   ```bash
   cargo run --release --target aarch64-apple-darwin
   ```

2. **Solo Compilar**:
   Si prefieres solo compilar sin ejecutar:
   ```bash
   cargo build --release --target aarch64-apple-darwin
   ```
   El binario ejecutable se encontrará en `target/aarch64-apple-darwin/release/nubes`.

### Empaquetar como `.app` (Opcional)
Si deseas generar un archivo `.app` nativo de macOS (Application Bundle):
```bash
cargo install cargo-bundle
cargo bundle --release --target aarch64-apple-darwin
```
La aplicación `.app` resultante se generará en `target/aarch64-apple-darwin/release/bundle/osx/nubes.app`.
