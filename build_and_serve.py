#!/usr/bin/env python3
import subprocess
import sys
import os
import shutil

def run_command(cmd, cwd=None, check=True):
    print(f"➜ Ejecutando: {' '.join(cmd)}")
    try:
        subprocess.run(cmd, cwd=cwd, check=check)
    except subprocess.CalledProcessError as e:
        print(f"❌ Error al ejecutar el comando. Código de salida: {e.returncode}")
        sys.exit(e.returncode)
    except FileNotFoundError:
        print(f"❌ Error: Comando '{cmd[0]}' no encontrado. ¿Está instalado y en tu PATH?")
        sys.exit(1)

def main():
    print("=== Iniciando compilación dual (Web/Nativo) y servidor ===")

    # 1. Verificar si cargo-bundle está instalado, y sino, avisar.
    if not shutil.which("cargo-bundle"):
        print("Instalando cargo-bundle (requerido para generar el .app)...")
        run_command(["cargo", "install", "cargo-bundle"])

    # 2. Generar el .app nativo de macOS
    print("\n[1/3] Construyendo el binario nativo y empaquetando como .app...")
    run_command(["cargo", "bundle", "--release", "--target", "aarch64-apple-darwin"])
    app_path = "target/aarch64-apple-darwin/release/bundle/osx/nubes.app"
    print(f"✅ .app nativo generado en: {app_path}")
    
    print("\n🚀 Abriendo la aplicación nativa...")
    subprocess.Popen(["open", app_path])

if __name__ == "__main__":
    main()
