# Subscription Config Aggregator & TCP Health Checker

Rust desktop utility that pulls remote subscription feeds, merges proxy-style config lines, deduplicates them, and optionally filters by TCP reachability. Built with **egui/eframe**, **reqwest**, and **tokio**.

![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)

## What it does

- **Subscription fetching** — HTTP(S) download with base64/plain-text decoding.
- **URI parsing** — Extract host/port from common proxy URI schemes (VLESS, VMess, SS, Trojan, SSR).
- **Deduplication** — Merge multiple sources into one unique list.
- **TCP health checks** — Parallel connectivity probes with per-endpoint latency.
- **Desktop GUI** — Dark-themed **egui** interface; writes merged output next to the binary.

## Stack

| Layer | Choice |
|-------|--------|
| Language | Rust 2021 |
| UI | egui / eframe |
| HTTP | reqwest (blocking) |
| Async I/O | tokio (`TcpStream`, timeouts) |
| Packaging | Single release binary (Windows resource icon via `winres`) |

## Usage

1. Paste subscription or config URLs (one per line) into the source field.
2. Enable **Remove duplicate configs** and/or **Detailed ping check (TCP)** as needed.
3. Click **START** — output is base64-encoded and saved as `subscription.txt` beside the executable.

## Build

```powershell
cargo build --release
```

Release artifact: `target/release/vpn-aggregator` (`.exe` on Windows).

## Notes

- `target/`, `*.exe`, `*.pdb`, and local output files are gitignored.
- Crate / repo folder name remains `vpn-aggregator` for history; display name above is portfolio-neutral.
