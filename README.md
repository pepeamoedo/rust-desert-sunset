# Desert Sunset (Rust + WGPU)

A cinematic, highly-optimized 3D desert sunset renderer built entirely from scratch in Rust using `wgpu`. 

This project explores procedural generation and raymarching techniques entirely on the GPU, featuring:

## Features
- **Procedural Volumetric Clouds**: Raymarched 3D clouds using fractional Brownian motion (fBm) noise with realistic lighting, scattering, and beer's law shadows.
- **Infinite Sand Dunes**: Homogeneous Saharan dunes generated via ridged multifractal noise, complete with self-shadowing and wind ripples.
- **Cinematic Lighting & Optics**: Features atmospheric scattering, a glowing sunset, wide-angle "GoPro" barrel distortion, and mathematical lens flares (ghosts & halos).
- **Celestial Sphere**: A physically accurate starry night sky (mapped via Voronoi cells to prevent aliasing) with a distinct Venus and a waning crescent moon that incorporates Earthshine. Subtle parallax effects decouple the sky from the terrestrial rotation.
- **Performance Telemetry**: An integrated `egui` telemetry panel to monitor FPS and GPU rendering times in real-time.
- **Cross-Platform**: Compiles natively to macOS/Windows/Linux as well as to the Web (WebAssembly/WebGL2) natively via `wgpu`.

## Building & Running

**Prerequisites:** You need to have the Rust toolchain installed.

To run the native application locally:
```bash
cargo run --release
```

To build a bundled macOS application (`.app` / `.dmg`):
```bash
cargo bundle --release
```

To build and serve the WebAssembly version locally (requires python for the local server):
```bash
./build_and_serve.py
```
