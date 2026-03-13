#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use eframe::egui;
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use url::Url;
use uuid::Uuid;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VPN Aggregator")
            .with_inner_size([750.0, 750.0])
            .with_min_inner_size([600.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "VPN Aggregator",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App::default()))
        }),
    )
}

// ── Models ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FetchResult {
    url: String,
    status: FetchStatus,
    duration_ms: u128,
    count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FetchStatus {
    Ok,
    HttpError(u16),
    Timeout,
    Error(String),
}

#[derive(Clone, Debug)]
struct ConfigResult {
    protocol: String,
    addr: String,
    ping_ms: Option<u128>,
    is_alive: bool,
}

#[derive(Clone)]
enum WorkerMsg {
    Progress(String),
    FetchDone(FetchResult),
    ConfigDone(ConfigResult),
    Finished {
        result_b64: String,
    },
}

// ── Application State ────────────────────────────────────────────────────────

struct App {
    urls_input: String,
    result_output: String,
    status: String,
    is_loading: bool,
    deduplicate: bool,
    check_configs: bool,
    fetch_log: Vec<FetchResult>,
    health_log: Vec<ConfigResult>,
    hwid: String,
    show_log: bool,
    rx: Option<mpsc::Receiver<WorkerMsg>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            urls_input: String::new(),
            result_output: String::new(),
            status: "Ready".into(),
            is_loading: false,
            deduplicate: true,
            check_configs: true,
            fetch_log: Vec::new(),
            health_log: Vec::new(),
            hwid: Uuid::new_v4().to_string(),
            show_log: false,
            rx: None,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMsg::Progress(s) => self.status = s,
                    WorkerMsg::FetchDone(r) => self.fetch_log.push(r),
                    WorkerMsg::ConfigDone(r) => self.health_log.push(r),
                    WorkerMsg::Finished { result_b64 } => {
                        self.result_output = result_b64;
                        self.is_loading = false;
                        self.status = "Aggregation finished".into();
                    }
                }
            }
        }

        if self.is_loading {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new("⚡ VPN Aggregator").size(24.0).strong());
            });

            ui.separator();

            ui.label(egui::RichText::new("📋 Subscription links:").strong());
            egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.urls_input)
                        .hint_text("Paste URLs here (one per line)...")
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(4),
                );
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.deduplicate, "Deduplicate");
                ui.checkbox(&mut self.check_configs, "Check Health (TCP)");
            });

            ui.horizontal(|ui| {
                let can_run = !self.is_loading && !self.urls_input.trim().is_empty();
                
                // Start Button
                if ui.add_enabled(can_run, egui::Button::new(egui::RichText::new("🚀 Start").size(15.0)).min_size(egui::vec2(110.0, 32.0))).clicked() {
                    self.start_work(ctx);
                }

                // Copy Button
                if ui.add_enabled(!self.result_output.is_empty(), egui::Button::new(egui::RichText::new("📎 Copy").size(15.0)).min_size(egui::vec2(110.0, 32.0))).clicked() {
                    if let Ok(mut c) = arboard::Clipboard::new() {
                        let _ = c.set_text(&self.result_output);
                        self.status = "Copied to clipboard".into();
                    }
                }

                // Save Button
                if ui.add_enabled(!self.result_output.is_empty(), egui::Button::new(egui::RichText::new("💾 Save").size(15.0)).min_size(egui::vec2(110.0, 32.0))).clicked() {
                    if let Some(path) = rfd::FileDialog::new().set_file_name("subscription.txt").save_file() {
                        let _ = std::fs::write(path, &self.result_output);
                        self.status = "File saved successfully".into();
                    }
                }

                // Log Toggle Button
                let log_text = if self.show_log { "🔍 Hide Log" } else { "🔍 Show Log" };
                if ui.add(egui::Button::new(egui::RichText::new(log_text).size(15.0)).min_size(egui::vec2(120.0, 32.0))).clicked() {
                    self.show_log = !self.show_log;
                }

                if self.is_loading {
                    ui.spinner();
                }
            });

            ui.colored_label(
                if self.is_loading { egui::Color32::YELLOW } else { egui::Color32::LIGHT_GREEN },
                &self.status,
            );

            if self.show_log && (!self.fetch_log.is_empty() || !self.health_log.is_empty()) {
                ui.separator();
                ui.label(egui::RichText::new("Activity Log:").strong());
                egui::ScrollArea::vertical().id_salt("log_scroll").max_height(200.0).show(ui, |ui| {
                    egui::Grid::new("log_grid").num_columns(4).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
                        for r in &self.fetch_log {
                            ui.label(match r.status {
                                FetchStatus::Ok => egui::RichText::new("FETCH").color(egui::Color32::LIGHT_BLUE),
                                _ => egui::RichText::new("ERROR").color(egui::Color32::RED),
                            });
                            ui.label(truncate_url(&r.url, 45));
                            ui.label(format!("{}ms", r.duration_ms));
                            ui.label(format!("{} refs", r.count));
                            ui.end_row();
                        }
                        for r in &self.health_log {
                            ui.label(egui::RichText::new(&r.protocol).color(egui::Color32::from_rgb(120, 220, 120)));
                            ui.label(truncate_url(&r.addr, 45));
                            ui.label(match r.ping_ms {
                                Some(p) => format!("{}ms", p),
                                None => "---".into(),
                            });
                            ui.label(if r.is_alive {
                                egui::RichText::new("ONLINE").color(egui::Color32::LIGHT_GREEN)
                            } else {
                                egui::RichText::new("OFFLINE").color(egui::Color32::RED)
                            });
                            ui.end_row();
                        }
                    });
                });
            }

            ui.separator();

            ui.label(egui::RichText::new("📦 Result (base64):").strong());
            egui::ScrollArea::vertical().id_salt("res_scroll").show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.result_output)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(12),
                );
            });

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Session HWID: {}", self.hwid)).color(egui::Color32::DARK_GRAY).small());
            });
        });
    }
}

impl App {
    fn start_work(&mut self, ctx: &egui::Context) {
        let urls: Vec<String> = self.urls_input.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        self.is_loading = true;
        self.fetch_log.clear();
        self.health_log.clear();
        self.result_output.clear();
        self.status = "Starting aggregation...".into();

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        let ctx = ctx.clone();
        let dedupe = self.deduplicate;
        let check = self.check_configs;
        let hwid = self.hwid.clone();

        thread::spawn(move || {
            run_core_logic(urls, tx, ctx, dedupe, check, hwid);
        });
    }
}

// ── Core Engine ──────────────────────────────────────────────────────────────

fn run_core_logic(urls: Vec<String>, tx: mpsc::Sender<WorkerMsg>, ctx: egui::Context, dedupe: bool, check: bool, hwid: String) {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) VPN-Aggregator/1.3")
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert("X-HWID", reqwest::header::HeaderValue::from_str(&hwid).unwrap());
            h
        })
        .build()
        .unwrap();

    let mut all_configs = Vec::new();
    let total = urls.len();

    // 1. Concurrent Fetch
    for (i, u) in urls.iter().enumerate() {
        let _ = tx.send(WorkerMsg::Progress(format!("Loading sources ({}/{})...", i + 1, total)));
        ctx.request_repaint();

        let start = Instant::now();
        let (status, lines) = fetch_sub(&client, u);
        let duration = start.elapsed().as_millis();
        
        all_configs.extend(lines.clone());
        let _ = tx.send(WorkerMsg::FetchDone(FetchResult { url: u.clone(), status, duration_ms: duration, count: lines.len() }));
        ctx.request_repaint();
    }

    // 2. Global Deduplicate
    let mut unique = all_configs;
    if dedupe {
        let _ = tx.send(WorkerMsg::Progress("Cleaning duplicates...".into()));
        ctx.request_repaint();
        let mut seen = HashSet::new();
        unique = unique.into_iter().filter(|s| !s.trim().is_empty() && seen.insert(s.clone())).collect();
    }

    // 3. Fast TCP Health Check
    let mut final_list = unique;
    if check && !final_list.is_empty() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        final_list = rt.block_on(async {
            let mut results = Vec::new();
            let mut tasks = Vec::new();

            for c in final_list {
                let tx_c = tx.clone();
                let ctx_c = ctx.clone();
                tasks.push(tokio::spawn(async move {
                    let mut res = ConfigResult {
                        protocol: "vpn".into(),
                        addr: "???".into(),
                        ping_ms: None,
                        is_alive: false,
                    };

                    if let Some((p, a, port)) = parse_link(&c) {
                        res.protocol = p.to_uppercase();
                        res.addr = format!("{}:{}", a, port);
                        let start = Instant::now();
                        match tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(&res.addr)).await {
                            Ok(Ok(_)) => {
                                res.ping_ms = Some(start.elapsed().as_millis());
                                res.is_alive = true;
                            }
                            _ => {}
                        }
                    }

                    let _ = tx_c.send(WorkerMsg::ConfigDone(res.clone()));
                    ctx_c.request_repaint();
                    if res.is_alive { Some(c) } else { None }
                }));
            }

            for t in tasks {
                if let Ok(Some(c)) = t.await { results.push(c); }
            }
            results
        });
    }

    // 4. Encode result
    let out = B64.encode(final_list.join("\n").as_bytes());
    let _ = tx.send(WorkerMsg::Finished { result_b64: out });
    ctx.request_repaint();
}

fn fetch_sub(client: &reqwest::blocking::Client, url: &str) -> (FetchStatus, Vec<String>) {
    let resp = match client.get(url).send() {
        Ok(r) => r,
        Err(e) => return (if e.is_timeout() { FetchStatus::Timeout } else { FetchStatus::Error(e.to_string()) }, vec![]),
    };

    if !resp.status().is_success() { return (FetchStatus::HttpError(resp.status().as_u16()), vec![]); }
    let body = resp.text().unwrap_or_default();
    let mut content = body.trim().to_string();
    
    // Auto-decode if base64
    let rem = content.len() % 4;
    if rem != 0 { content.push_str(&"=".repeat(4 - rem)); }
    let decoded = match B64.decode(&content) {
        Ok(b) => String::from_utf8(b).unwrap_or(body),
        Err(_) => body,
    };

    let lines = decoded.lines().map(|s| s.to_string()).filter(|s| !s.trim().is_empty()).collect();
    (FetchStatus::Ok, lines)
}

fn parse_link(link: &str) -> Option<(String, String, u16)> {
    if let Ok(u) = Url::parse(link) {
        let proto = u.scheme().to_string();
        let host = u.host_str()?.to_string();
        let port = u.port().unwrap_or(443);
        return Some((proto, host, port));
    }
    if link.starts_with("vmess://") {
        if let Ok(dec) = B64.decode(&link[8..]) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&dec) {
                let addr = v.get("add")?.as_str()?;
                let port = v.get("port")?.as_u64().or_else(|| v.get("port")?.as_str()?.parse().ok())? as u16;
                return Some(("vmess".into(), addr.into(), port));
            }
        }
    }
    None
}

fn truncate_url(u: &str, max: usize) -> String {
    if u.len() <= max { u.into() } else { format!("{}...", &u[..max-3]) }
}
