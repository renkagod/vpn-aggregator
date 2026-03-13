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
    let icon = load_icon();
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VPN Aggregator")
            .with_inner_size([800.0, 660.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(icon),
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

fn load_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.png");
    if let Ok(img) = image::load_from_memory(icon_bytes) {
        let img = img.to_rgba8();
        let (width, height) = img.dimensions();
        let rgba = img.into_raw();
        egui::IconData { rgba, width, height }
    } else {
        egui::IconData::default()
    }
}

// -- Models -------------------------------------------------------------------

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
    Progress(String, f32),
    FetchDone(FetchResult),
    ConfigDone(ConfigResult),
    Finished {
        result_b64: String,
    },
}

// -- Application State --------------------------------------------------------

struct App {
    urls_input: String,
    status: String,
    progress: f32,
    is_loading: bool,
    deduplicate: bool,
    check_configs: bool,
    emulate_hwid: bool,
    fetch_log: Vec<FetchResult>,
    health_log: Vec<ConfigResult>,
    hwid: String,
    rx: Option<mpsc::Receiver<WorkerMsg>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            urls_input: String::new(),
            status: "Ready for work".into(),
            progress: 0.0,
            is_loading: false,
            deduplicate: true,
            check_configs: true,
            emulate_hwid: true,
            fetch_log: Vec::new(),
            health_log: Vec::new(),
            hwid: Uuid::new_v4().to_string(),
            rx: None,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMsg::Progress(s, p) => {
                        self.status = s;
                        self.progress = p;
                    }
                    WorkerMsg::FetchDone(r) => self.fetch_log.push(r),
                    WorkerMsg::ConfigDone(r) => self.health_log.push(r),
                    WorkerMsg::Finished { result_b64 } => {
                        self.is_loading = false;
                        
                        // Automatic save
                        if let Ok(mut path) = std::env::current_exe() {
                            path.set_file_name("subscription.txt");
                            if std::fs::write(&path, result_b64).is_ok() {
                                self.status = format!("Done! Saved to: {:?}", path.file_name().unwrap());
                            } else {
                                self.status = "Task completed, but file save failed".into();
                            }
                        }
                        self.progress = 1.0;
                    }
                }
            }
        }

        if self.is_loading {
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            ui.label(egui::RichText::new("Source URLs:").strong());
                
                egui::ScrollArea::vertical()
                    .id_salt("urls_scroll")
                    .max_height(85.0)
                    .min_scrolled_height(85.0)
                    .auto_shrink([false, false]) // strictly fixed height and width
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.urls_input)
                                .hint_text("Insert subscription links here...")
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                        );
                    });

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.deduplicate, "Remove duplicate configs");
                    ui.checkbox(&mut self.check_configs, "Detailed ping check (TCP)");
                    ui.checkbox(&mut self.emulate_hwid, "HWID Emulation");
                });

                ui.horizontal(|ui| {
                    let can_run = !self.is_loading && !self.urls_input.trim().is_empty();
                    
                    if ui.add_enabled(can_run, egui::Button::new(egui::RichText::new("START").strong().size(16.0)).min_size(egui::vec2(100.0, 36.0))).clicked() {
                        self.start_work(ctx);
                    }

                    if self.is_loading {
                        ui.spinner();
                    }
                });

                if self.is_loading {
                    ui.add(egui::ProgressBar::new(self.progress).text(format!("{}: {:.0}%", self.status, self.progress * 100.0)));
                } else {
                    ui.add_space(20.0); // spacer instead of small label
                }

                ui.separator();
                ui.label(egui::RichText::new("Activity Details:").strong());
                
                let area = egui::ScrollArea::vertical()
                    .id_salt("activity_area")
                    .max_height(250.0)
                    .min_scrolled_height(250.0) // force exact size always
                    .auto_shrink([false, false])
                    .stick_to_bottom(true); 

                area.show(ui, |ui| {
                        if !self.fetch_log.is_empty() {
                            ui.label(egui::RichText::new("🌐 Fetch Results:").strong().color(egui::Color32::LIGHT_GRAY));
                            let max_w = ui.available_width();
                            let url_w = (max_w - 50.0 - 80.0 - 80.0 - 60.0).max(150.0); // calculate dynamic width for URL column
                            egui::ScrollArea::horizontal().id_salt("fetch_h").max_width(max_w).show(ui, |ui| {
                                egui::Grid::new("fetch_grid").num_columns(4).spacing([15.0, 6.0]).striped(true).show(ui, |ui| {
                                    for r in self.fetch_log.iter().rev().take(100).rev() {
                                        ui.add_sized([50.0, 14.0], egui::Label::new(match r.status {
                                            FetchStatus::Ok => egui::RichText::new("FETCH").color(egui::Color32::LIGHT_BLUE),
                                            _ => egui::RichText::new("ERROR").color(egui::Color32::RED),
                                        }).truncate());
                                        
                                        ui.add_sized([url_w, 14.0], egui::Label::new(truncate_url(&r.url, 200)).truncate());
                                        
                                        ui.add_sized([80.0, 14.0], egui::Label::new(format!("{}ms", r.duration_ms)).truncate());
                                        
                                        ui.add_sized([80.0, 14.0], egui::Label::new(format!("{} refs", r.count)).truncate());
                                        
                                        ui.end_row();
                                    }
                                });
                            });
                        }
                        
                        if !self.health_log.is_empty() {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("⚡ Ping Results:").strong().color(egui::Color32::LIGHT_GRAY));
                            let max_w = ui.available_width();
                            let url_w = (max_w - 50.0 - 80.0 - 80.0 - 60.0).max(150.0);
                            egui::ScrollArea::horizontal().id_salt("health_h").max_width(max_w).show(ui, |ui| {
                                egui::Grid::new("health_grid").num_columns(4).spacing([15.0, 6.0]).striped(true).show(ui, |ui| {
                                    for r in self.health_log.iter().rev().take(100).rev() {
                                        ui.add_sized([50.0, 14.0], egui::Label::new(egui::RichText::new(&r.protocol).color(egui::Color32::from_rgb(120, 220, 120))).truncate());
                                        
                                        ui.add_sized([url_w, 14.0], egui::Label::new(truncate_url(&r.addr, 200)).truncate());
                                        
                                        ui.add_sized([80.0, 14.0], egui::Label::new(match r.ping_ms {
                                            Some(p) => format!("{}ms", p),
                                            None => "timeout".into(),
                                        }).truncate());
                                        
                                        ui.add_sized([80.0, 14.0], egui::Label::new(if r.is_alive {
                                            egui::RichText::new("ONLINE").color(egui::Color32::LIGHT_GREEN)
                                        } else {
                                            egui::RichText::new("DEAD").color(egui::Color32::RED)
                                        }).truncate());
                                        
                                        ui.end_row();
                                    }
                                });
                            });
                        }
                    });
                ui.add_space(8.0);
                ui.label(egui::RichText::new(format!("Session HWID: {}", self.hwid)).color(egui::Color32::DARK_GRAY).small());

                if !self.is_loading && (self.status.contains("Done") || self.status.contains("Success") || self.status.contains("Ready")) {
                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(&self.status)
                                .color(egui::Color32::LIGHT_GREEN)
                                .size(16.0)
                                .strong()
                        );
                    });
                }
        });
    }
}

impl App {
    fn start_work(&mut self, ctx: &egui::Context) {
        let urls: Vec<String> = self.urls_input.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        self.is_loading = true;
        self.fetch_log.clear();
        self.health_log.clear();
        self.status = "Starting aggregation...".into();
        self.progress = 0.0;

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        let ctx = ctx.clone();
        let dedupe = self.deduplicate;
        let check = self.check_configs;
        let emulate_hwid = self.emulate_hwid;
        let hwid = self.hwid.clone();

        thread::spawn(move || {
            run_core_logic(urls, tx, ctx, dedupe, check, emulate_hwid, hwid);
        });
    }
}

// -- Improved Engine ----------------------------------------------------------

fn run_core_logic(urls: Vec<String>, tx: mpsc::Sender<WorkerMsg>, ctx: egui::Context, dedupe: bool, check: bool, emulate_hwid: bool, hwid: String) {
    let mut headers = reqwest::header::HeaderMap::new();
    
    if emulate_hwid {
        headers.insert("User-Agent", reqwest::header::HeaderValue::from_static("v2rayNG/1.8.12"));
        headers.insert("X-HWID", reqwest::header::HeaderValue::from_str(&hwid).unwrap());
        headers.insert("hwid", reqwest::header::HeaderValue::from_str(&hwid).unwrap());
        headers.insert("X-Device-Id", reqwest::header::HeaderValue::from_str(&hwid).unwrap());
    } else {
        headers.insert("User-Agent", reqwest::header::HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"));
        headers.insert("Accept", reqwest::header::HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"));
        headers.insert("Accept-Language", reqwest::header::HeaderValue::from_static("en-US,en;q=0.9,ru;q=0.8"));
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .tcp_nodelay(true)
        .default_headers(headers)
        .build()
        .unwrap();

    let mut all_configs = Vec::new();
    let total = urls.len();

    // 1. Fetch or parse direct links
    for (i, u) in urls.iter().enumerate() {
        if u.starts_with("http://") || u.starts_with("https://") {
            let p = (i as f32) / (total as f32 + 2.0);
            let _ = tx.send(WorkerMsg::Progress(format!("Loading ({}/{})", i + 1, total), p));
            ctx.request_repaint();

            let start = Instant::now();
            let (status, lines) = fetch_sub(&client, u);
            let duration = start.elapsed().as_millis();
            
            all_configs.extend(lines.clone());
            let _ = tx.send(WorkerMsg::FetchDone(FetchResult { url: u.clone(), status, duration_ms: duration, count: lines.len() }));
            ctx.request_repaint();
        } else if u.starts_with("vless://") || u.starts_with("vmess://") || u.starts_with("trojan://") || u.starts_with("ss://") {
            // Direct config, add to processing directly without fetching
            all_configs.push(u.clone());
        } // ignore invalid non-http inputs to prevent errors
    }

    // 2. Deduplicate
    let mut unique = all_configs;
    if dedupe {
        let _ = tx.send(WorkerMsg::Progress("Deduplicating...".into(), (total as f32 + 0.5) / (total as f32 + 2.0)));
        ctx.request_repaint();
        let mut seen = HashSet::new();
        unique = unique.into_iter().filter(|s| !s.trim().is_empty() && seen.insert(s.clone())).collect();
    }

    // 3. Health Check
    let mut final_list = unique;
    if check && !final_list.is_empty() {
        let _ = tx.send(WorkerMsg::Progress("Checking health...".into(), (total as f32 + 1.0) / (total as f32 + 2.0)));
        ctx.request_repaint();
        
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
                        match tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(&res.addr)).await {
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

    // 4. Encode
    let out = B64.encode(final_list.join("\n").as_bytes());
    let _ = tx.send(WorkerMsg::Progress("Encoding...".into(), 0.99));
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
    let content = body.trim().replace(|c: char| c.is_whitespace(), "");
    
    // Attempt decoding as base64 or URL-safe base64
    let decoded = if content.starts_with("vmess://") || content.starts_with("vless://") || content.starts_with("ss://") || content.starts_with("trojan://") {
        body // It's already plain text lines
    } else {
        // Try to decode. Base64 can use -_ instead of +/
        let mut b64_content = content.replace('-', "+").replace('_', "/");
        let rem = b64_content.len() % 4;
        if rem != 0 { b64_content.push_str(&"=".repeat(4 - rem)); }
        
        match B64.decode(&b64_content) {
            Ok(b) => String::from_utf8(b).unwrap_or(body),
            Err(_) => body,
        }
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
