# VPN Aggregator

Desktop utility for merging and checking VPN subscriptions (VLESS, VMess, SS, Trojan, SSR) into a single base64 link.

![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)

## Features

- **X-HWID Emulation** — Automated unique HWID header per session.
- **Fast TCP Health Checks** — Multi-threaded availability testing with real-time ping data.
- **Global Deduplication** — Precise merging of all sources into one unique list.
- **Minimalist GUI** — Clean dark interface built with `egui`.
- **Stand-alone Binary** — One file, zero dependencies.

## Usage

1. Paste your subscription links into the URLs field.
2. Click **Start Aggregation**.
3. Copy or Save the resulting base64 string.

## Build

```powershell
cargo build --release
```

Binary will be located at `target/release/vpn-aggregator.exe`.
