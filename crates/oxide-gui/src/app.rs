use crate::theme::{
    action_ribbon_button, apply_modern_theme, category_color, draw_badge_pill, draw_progress_bar,
    draw_speed_sparkline, segment_state_color, status_color, Palette,
};
use eframe::egui::{self, Color32, Margin, Pos2, Rect, Rounding, Sense, Stroke, Vec2};
use oxide_core::{
    compute_file_sha256, expand_wildcard_url, Category, DownloadJobSnapshot, DownloadManager,
    DownloadRequest, DownloadStatus, QueueConfig, RefreshLinkPayload,
};
use oxide_spider::{crawl_page, DiscoveredLink, SpiderFilter};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Eq)]
pub enum SidebarFilter {
    All,
    Active,
    Unfinished,
    Finished,
    Category(Category),
    Queue(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Newest,
    Filename,
    Size,
    Speed,
    Progress,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InspectorTab {
    DownloadStatus,
    SpeedLimiter,
    CompletionAndRefresh,
}

pub struct OxideApp {
    pub manager: DownloadManager,
    pub filter: SidebarFilter,
    pub sort_by: SortBy,
    pub search_query: String,
    pub selected_job_id: Option<String>,
    pub inspected_job_id: Option<String>,
    pub inspector_tab: InspectorTab,
    pub show_bottom_dock: bool,

    // Quick-Add Bar State
    pub quick_url: String,
    pub quick_connections: usize,

    // Throughput Sparkline History (60 samples @ 250ms)
    pub speed_history: VecDeque<u64>,
    pub last_sample_time: Instant,

    // Add URL Modal State
    pub show_add_modal: bool,
    pub add_url: String,
    pub add_filename: String,
    pub add_save_dir: String,
    pub add_connections: usize,
    pub add_queue_id: String,
    pub add_cookie: String,
    pub add_referer: String,
    pub add_user_agent: String,
    pub add_sha256: String,

    // Batch Wildcard Modal State
    pub show_batch_modal: bool,
    pub batch_pattern: String,
    pub batch_connections: usize,
    pub batch_queue_id: String,

    // Site Grabber / Spider Modal State (`oxide-spider`)
    pub show_spider_modal: bool,
    pub spider_url: String,
    pub spider_extensions: String,
    pub spider_same_domain: bool,
    pub spider_results: Arc<RwLock<Vec<(bool, DiscoveredLink)>>>,
    pub spider_status: Arc<RwLock<String>>,

    // Queue & Cron Scheduler Modal State
    pub show_scheduler_modal: bool,
    pub sched_queue_id: String,
    pub sched_queue_name: String,
    pub sched_max_concurrent: usize,
    pub sched_start_cron: String,
    pub sched_stop_cron: String,
    pub sched_max_retries: u32,

    // Global Speed Limiter Modal State
    pub show_speed_modal: bool,
    pub global_limit_kbps: u64,
    pub task_limit_kbps: u64,

    // Expired Link Refresh & Checksum State
    pub refresh_url_input: String,
    pub refresh_cookie_input: String,
    pub refresh_referer_input: String,
    pub verify_hash_result: Option<String>,
}

impl OxideApp {
    pub fn new(manager: DownloadManager, default_save_dir: String) -> Self {
        let mut speed_history = VecDeque::with_capacity(60);
        for _ in 0..60 {
            speed_history.push_back(0);
        }

        Self {
            manager,
            filter: SidebarFilter::All,
            sort_by: SortBy::Newest,
            search_query: String::new(),
            selected_job_id: None,
            inspected_job_id: None,
            inspector_tab: InspectorTab::DownloadStatus,
            show_bottom_dock: true,
            quick_url: String::new(),
            quick_connections: 8,
            speed_history,
            last_sample_time: Instant::now(),
            show_add_modal: false,
            add_url: String::new(),
            add_filename: String::new(),
            add_save_dir: default_save_dir,
            add_connections: 8,
            add_queue_id: String::from("default"),
            add_cookie: String::new(),
            add_referer: String::new(),
            add_user_agent: String::new(),
            add_sha256: String::new(),
            show_batch_modal: false,
            batch_pattern: String::from("https://example.com/release_part_[01-10].zip"),
            batch_connections: 8,
            batch_queue_id: String::from("default"),
            show_spider_modal: false,
            spider_url: String::from("https://releases.ubuntu.com/"),
            spider_extensions: String::from("iso, zip, tar, gz, deb, mp4, mkv, pdf"),
            spider_same_domain: true,
            spider_results: Arc::new(RwLock::new(Vec::new())),
            spider_status: Arc::new(RwLock::new(String::from(
                "Enter a target webpage URL and click Crawl Webpage.",
            ))),
            show_scheduler_modal: false,
            sched_queue_id: String::from("default"),
            sched_queue_name: String::from("Main Download Queue"),
            sched_max_concurrent: 4,
            sched_start_cron: String::new(),
            sched_stop_cron: String::new(),
            sched_max_retries: 5,
            show_speed_modal: false,
            global_limit_kbps: 0,
            task_limit_kbps: 0,
            refresh_url_input: String::new(),
            refresh_cookie_input: String::new(),
            refresh_referer_input: String::new(),
            verify_hash_result: None,
        }
    }
}

impl eframe::App for OxideApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_modern_theme(ctx);

        let snapshots = self.manager.list_snapshots();
        let total_speed: u64 = snapshots
            .iter()
            .filter(|s| s.status == DownloadStatus::Downloading)
            .map(|s| s.speed_bps)
            .sum();
        let active_workers: usize = snapshots
            .iter()
            .filter(|s| s.status == DownloadStatus::Downloading)
            .map(|s| {
                s.segments
                    .iter()
                    .filter(|seg| !seg.completed)
                    .count()
            })
            .sum();

        if self.last_sample_time.elapsed() >= Duration::from_millis(250) {
            if self.speed_history.len() >= 60 {
                self.speed_history.pop_front();
            }
            self.speed_history.push_back(total_speed);
            self.last_sample_time = Instant::now();
        }

        let has_active = snapshots.iter().any(|s| {
            s.status == DownloadStatus::Downloading || s.status == DownloadStatus::Connecting
        });
        if has_active {
            ctx.request_repaint_after(Duration::from_millis(80));
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        // Auto-select first job if none is selected
        if self.selected_job_id.is_none() && !snapshots.is_empty() {
            self.selected_job_id = Some(snapshots[0].id.clone());
        }

        // ====================================================================
        // 1. TOP BRAND & ACTION RIBBON (IDM PRO STYLE)
        // ====================================================================
        egui::TopBottomPanel::top("oxide_top_ribbon")
            .frame(
                egui::Frame::none()
                    .fill(Palette::BG_TOOLBAR)
                    .inner_margin(Margin::symmetric(16.0, 10.0))
                    .stroke(Stroke::new(1.0_f32, Palette::BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Brand Logo Box
                    egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(59, 130, 246, 38))
                        .stroke(Stroke::new(1.0_f32, Palette::BLUE))
                        .rounding(Rounding::same(7.0))
                        .inner_margin(Margin::symmetric(9.0, 5.0))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("OXIDE")
                                    .size(16.0)
                                    .strong()
                                    .color(Palette::CYAN_BRIGHT),
                            );
                        });

                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 1.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Download Manager")
                                    .size(13.5)
                                    .strong()
                                    .color(Palette::TEXT_PRIMARY),
                            );
                            draw_badge_pill(ui, "RUST NATIVE", Palette::BLUE);
                        });
                        ui.label(
                            egui::RichText::new("Dynamic 32-Segment Work-Stealing Engine")
                                .size(10.5)
                                .color(Palette::TEXT_MUTED),
                        );
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Primary IDM Action Ribbon Buttons
                    if action_ribbon_button(ui, "+ Add URL", Palette::BLUE, true) {
                        self.show_add_modal = true;
                    }
                    if action_ribbon_button(ui, "Batch Wildcard", Palette::INDIGO, false) {
                        self.show_batch_modal = true;
                    }
                    if action_ribbon_button(ui, "Site Grabber", Palette::TEAL, false) {
                        self.show_spider_modal = true;
                    }

                    ui.separator();

                    if action_ribbon_button(ui, "Resume", Palette::EMERALD_BRIGHT, false) {
                        if let Some(ref id) = self.selected_job_id {
                            let mgr = self.manager.clone();
                            let job_id = id.clone();
                            tokio::spawn(async move {
                                let _ = mgr.resume_download(&job_id).await;
                            });
                        }
                    }
                    if action_ribbon_button(ui, "Stop", Palette::AMBER, false) {
                        if let Some(ref id) = self.selected_job_id {
                            let mgr = self.manager.clone();
                            let job_id = id.clone();
                            tokio::spawn(async move {
                                let _ = mgr.pause_download(&job_id).await;
                            });
                        }
                    }
                    if action_ribbon_button(ui, "Stop All", Palette::ORANGE, false) {
                        let mgr = self.manager.clone();
                        tokio::spawn(async move {
                            let _ = mgr.pause_all().await;
                        });
                    }
                    if action_ribbon_button(ui, "Delete", Palette::ROSE, false) {
                        if let Some(id) = self.selected_job_id.take() {
                            if self.inspected_job_id.as_deref() == Some(&id) {
                                self.inspected_job_id = None;
                            }
                            let mgr = self.manager.clone();
                            tokio::spawn(async move {
                                let _ = mgr.delete_download(&id, false).await;
                            });
                        }
                    }

                    ui.separator();

                    if action_ribbon_button(ui, "Scheduler & Queues", Palette::PURPLE, false) {
                        self.show_scheduler_modal = true;
                    }
                    if action_ribbon_button(ui, "Speed Limiter", Palette::CYAN_BRIGHT, false) {
                        self.global_limit_kbps = self.manager.get_global_speed_limit() / 1024;
                        self.show_speed_modal = true;
                    }

                    // Right-side Live Telemetry Pill
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::Frame::none()
                            .fill(Palette::BG_CARD)
                            .stroke(Stroke::new(1.0_f32, Palette::BORDER_SUBTLE))
                            .rounding(Rounding::same(7.0))
                            .inner_margin(Margin::symmetric(10.0, 5.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}/s",
                                            format_bytes(total_speed)
                                        ))
                                        .monospace()
                                        .strong()
                                        .size(13.0)
                                        .color(Palette::CYAN_BRIGHT),
                                    );
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new(format!("{} Threads", active_workers))
                                            .monospace()
                                            .size(11.5)
                                            .color(Palette::EMERALD_BRIGHT),
                                    );
                                });
                            });
                    });
                });
            });

        // ====================================================================
        // 2. LEFT SIDEBAR NAVIGATION TREE & TELEMETRY
        // ====================================================================
        egui::SidePanel::left("oxide_sidebar")
            .resizable(false)
            .exact_width(235.0)
            .frame(
                egui::Frame::none()
                    .fill(Palette::BG_SIDEBAR)
                    .inner_margin(Margin::symmetric(12.0, 14.0))
                    .stroke(Stroke::new(1.0_f32, Palette::BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("TASK FILTERS")
                        .size(10.5)
                        .strong()
                        .color(Palette::TEXT_DIM),
                );
                ui.add_space(4.0);

                sidebar_nav_row(
                    ui,
                    &mut self.filter,
                    SidebarFilter::All,
                    "All Downloads",
                    snapshots.len(),
                    Palette::BLUE,
                );
                sidebar_nav_row(
                    ui,
                    &mut self.filter,
                    SidebarFilter::Active,
                    "Downloading Now",
                    snapshots
                        .iter()
                        .filter(|s| s.status == DownloadStatus::Downloading)
                        .count(),
                    Palette::CYAN_BRIGHT,
                );
                sidebar_nav_row(
                    ui,
                    &mut self.filter,
                    SidebarFilter::Unfinished,
                    "Unfinished / Paused",
                    snapshots
                        .iter()
                        .filter(|s| s.status != DownloadStatus::Completed)
                        .count(),
                    Palette::AMBER,
                );
                sidebar_nav_row(
                    ui,
                    &mut self.filter,
                    SidebarFilter::Finished,
                    "Completed",
                    snapshots
                        .iter()
                        .filter(|s| s.status == DownloadStatus::Completed)
                        .count(),
                    Palette::EMERALD_BRIGHT,
                );

                ui.add_space(14.0);
                ui.label(
                    egui::RichText::new("CATEGORIES")
                        .size(10.5)
                        .strong()
                        .color(Palette::TEXT_DIM),
                );
                ui.add_space(4.0);

                for (cat, label) in [
                    (Category::Compressed, "Compressed Archives"),
                    (Category::Documents, "Documents & Books"),
                    (Category::Music, "Music & Audio"),
                    (Category::Programs, "Programs & ISOs"),
                    (Category::Videos, "Video & Streams"),
                    (Category::General, "General Files"),
                ] {
                    let count = snapshots.iter().filter(|s| s.category == cat).count();
                    sidebar_nav_row(
                        ui,
                        &mut self.filter,
                        SidebarFilter::Category(cat),
                        label,
                        count,
                        category_color(cat),
                    );
                }

                ui.add_space(14.0);
                ui.label(
                    egui::RichText::new("QUEUES & SCHEDULER")
                        .size(10.5)
                        .strong()
                        .color(Palette::TEXT_DIM),
                );
                ui.add_space(4.0);

                let queues = self.manager.db().list_queues().unwrap_or_default();
                for q in &queues {
                    let count = snapshots.iter().filter(|s| s.queue_id == q.id).count();
                    let label = format!("{} (Max {})", q.name, q.max_concurrent);
                    sidebar_nav_row(
                        ui,
                        &mut self.filter,
                        SidebarFilter::Queue(q.id.clone()),
                        &label,
                        count,
                        Palette::PURPLE,
                    );
                }

                // Bottom Throughput Graph & Engine Telemetry Card
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    egui::Frame::none()
                        .fill(Palette::BG_CARD)
                        .stroke(Stroke::new(1.0_f32, Palette::BORDER_SUBTLE))
                        .rounding(Rounding::same(8.0))
                        .inner_margin(Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("BANDWIDTH TELEMETRY")
                                            .size(10.0)
                                            .strong()
                                            .color(Palette::TEXT_MUTED),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let cap = self.manager.get_global_speed_limit();
                                            let cap_str = if cap > 0 {
                                                format!("Cap: {}/s", format_bytes(cap))
                                            } else {
                                                "Unlimited".to_string()
                                            };
                                            ui.label(
                                                egui::RichText::new(cap_str)
                                                    .size(10.0)
                                                    .monospace()
                                                    .color(Palette::EMERALD_BRIGHT),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(4.0);
                                let hist_slice: Vec<u64> =
                                    self.speed_history.iter().copied().collect();
                                draw_speed_sparkline(ui, &hist_slice, 190.0, 38.0);
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("SQLite WAL:")
                                            .size(10.5)
                                            .color(Palette::TEXT_MUTED),
                                    );
                                    ui.label(
                                        egui::RichText::new("500ms Sync Active")
                                            .size(10.5)
                                            .monospace()
                                            .color(Palette::CYAN_BRIGHT),
                                    );
                                });
                            });
                        });
                });
            });

        // ====================================================================
        // 3. BOTTOM COLLAPSIBLE LIVE SEGMENT INSPECTOR DOCK
        // ====================================================================
        if self.show_bottom_dock {
            if let Some(ref sel_id) = self.selected_job_id.clone() {
                if let Some(job) = snapshots.iter().find(|j| &j.id == sel_id) {
                    egui::TopBottomPanel::bottom("oxide_bottom_inspector")
                        .resizable(true)
                        .default_height(245.0)
                        .min_height(180.0)
                        .max_height(390.0)
                        .frame(
                            egui::Frame::none()
                                .fill(Palette::BG_SIDEBAR)
                                .inner_margin(Margin::symmetric(16.0, 12.0))
                                .stroke(Stroke::new(1.0_f32, Palette::BORDER_STRONG)),
                        )
                        .show(ctx, |ui| {
                            self.render_job_inspector_contents(ui, job);
                        });
                }
            }
        }

        // ====================================================================
        // 4. MAIN CENTER WORKSPACE: QUICK-ADD BAR + FILTER BAR + IDM TABLE
        // ====================================================================
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Palette::BG_CANVAS)
                    .inner_margin(Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                // 4A. Instant Quick-Add URL Bar
                egui::Frame::none()
                    .fill(Palette::BG_CARD)
                    .stroke(Stroke::new(1.0_f32, Palette::BORDER_SUBTLE))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            draw_badge_pill(ui, "QUICK DOWNLOAD", Palette::CYAN_BRIGHT);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.quick_url)
                                    .hint_text(
                                        "Paste HTTP / HTTPS / HLS (.m3u8) / DASH (.mpd) URL here and press Enter...",
                                    )
                                    .desired_width(ui.available_width() - 250.0),
                            );
                            ui.label(
                                egui::RichText::new("Threads:")
                                    .size(11.5)
                                    .color(Palette::TEXT_MUTED),
                            );
                            ui.add(
                                egui::DragValue::new(&mut self.quick_connections)
                                    .range(1..=32)
                                    .speed(0.2),
                            );

                            let enter_pressed =
                                resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if (action_ribbon_button(ui, "Download Now", Palette::BLUE, true)
                                || enter_pressed)
                                && !self.quick_url.trim().is_empty()
                            {
                                let req = DownloadRequest {
                                    url: self.quick_url.trim().to_string(),
                                    filename: None,
                                    save_dir: None,
                                    headers: HashMap::new(),
                                    max_segments: Some(self.quick_connections),
                                    speed_limit_bps: None,
                                    queue_id: Some("default".to_string()),
                                    expected_sha256: None,
                                };
                                let mgr = self.manager.clone();
                                tokio::spawn(async move {
                                    let _ = mgr.add_download(req, true).await;
                                });
                                self.quick_url.clear();
                            }
                        });
                    });

                ui.add_space(10.0);

                // 4B. Search, Sort & Dock Toggle Bar
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Search:")
                            .size(12.0)
                            .color(Palette::TEXT_SECONDARY),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("Filter by file name, extension, or domain...")
                            .desired_width(240.0),
                    );

                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Sort By:")
                            .size(12.0)
                            .color(Palette::TEXT_SECONDARY),
                    );
                    egui::ComboBox::from_id_salt("sort_by_combo")
                        .selected_text(match self.sort_by {
                            SortBy::Newest => "Date Added (Newest)",
                            SortBy::Filename => "File Name (A-Z)",
                            SortBy::Size => "Total Size",
                            SortBy::Speed => "Transfer Rate",
                            SortBy::Progress => "Progress %",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.sort_by,
                                SortBy::Newest,
                                "Date Added (Newest)",
                            );
                            ui.selectable_value(
                                &mut self.sort_by,
                                SortBy::Filename,
                                "File Name (A-Z)",
                            );
                            ui.selectable_value(&mut self.sort_by, SortBy::Size, "Total Size");
                            ui.selectable_value(
                                &mut self.sort_by,
                                SortBy::Speed,
                                "Transfer Rate",
                            );
                            ui.selectable_value(
                                &mut self.sort_by,
                                SortBy::Progress,
                                "Progress %",
                            );
                        });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let dock_label = if self.show_bottom_dock {
                            "Hide Live Segment Dock"
                        } else {
                            "Show Live Segment Dock"
                        };
                        if ui.button(dock_label).clicked() {
                            self.show_bottom_dock = !self.show_bottom_dock;
                        }

                        if ui.button("Resume All").clicked() {
                            let mgr = self.manager.clone();
                            tokio::spawn(async move {
                                let _ = mgr.resume_all().await;
                            });
                        }
                    });
                });

                ui.add_space(8.0);

                // 4C. Column Header Bar
                egui::Frame::none()
                    .fill(Palette::BG_HEADER)
                    .stroke(Stroke::new(1.0_f32, Palette::BORDER_SUBTLE))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(Margin::symmetric(12.0, 7.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [270.0, 16.0],
                                egui::Label::new(
                                    egui::RichText::new("FILE NAME & CATEGORY")
                                        .size(11.0)
                                        .strong()
                                        .color(Palette::TEXT_MUTED),
                                ),
                            );
                            ui.add_sized(
                                [115.0, 16.0],
                                egui::Label::new(
                                    egui::RichText::new("SIZE (DL / TOTAL)")
                                        .size(11.0)
                                        .strong()
                                        .color(Palette::TEXT_MUTED),
                                ),
                            );
                            ui.add_sized(
                                [185.0, 16.0],
                                egui::Label::new(
                                    egui::RichText::new("STATUS & PROGRESS")
                                        .size(11.0)
                                        .strong()
                                        .color(Palette::TEXT_MUTED),
                                ),
                            );
                            ui.add_sized(
                                [75.0, 16.0],
                                egui::Label::new(
                                    egui::RichText::new("TIME LEFT")
                                        .size(11.0)
                                        .strong()
                                        .color(Palette::TEXT_MUTED),
                                ),
                            );
                            ui.add_sized(
                                [100.0, 16.0],
                                egui::Label::new(
                                    egui::RichText::new("TRANSFER RATE")
                                        .size(11.0)
                                        .strong()
                                        .color(Palette::TEXT_MUTED),
                                ),
                            );
                            ui.add_sized(
                                [75.0, 16.0],
                                egui::Label::new(
                                    egui::RichText::new("SEGMENTS")
                                        .size(11.0)
                                        .strong()
                                        .color(Palette::TEXT_MUTED),
                                ),
                            );
                            ui.label(
                                egui::RichText::new("QUICK ACTIONS")
                                    .size(11.0)
                                    .strong()
                                    .color(Palette::TEXT_MUTED),
                            );
                        });
                    });

                ui.add_space(6.0);

                // Filter & Sort Rows
                let q_lower = self.search_query.to_lowercase();
                let mut filtered: Vec<&DownloadJobSnapshot> = snapshots
                    .iter()
                    .filter(|s| match &self.filter {
                        SidebarFilter::All => true,
                        SidebarFilter::Active => {
                            s.status == DownloadStatus::Downloading
                                || s.status == DownloadStatus::Connecting
                        }
                        SidebarFilter::Unfinished => s.status != DownloadStatus::Completed,
                        SidebarFilter::Finished => s.status == DownloadStatus::Completed,
                        SidebarFilter::Category(cat) => s.category == *cat,
                        SidebarFilter::Queue(qid) => &s.queue_id == qid,
                    })
                    .filter(|s| {
                        q_lower.is_empty()
                            || s.filename.to_lowercase().contains(&q_lower)
                            || s.url.to_lowercase().contains(&q_lower)
                    })
                    .collect();

                match self.sort_by {
                    SortBy::Newest => filtered.sort_by_key(|a| std::cmp::Reverse(a.created_at)),
                    SortBy::Filename => {
                        filtered.sort_by_key(|a| a.filename.to_lowercase())
                    }
                    SortBy::Size => {
                        filtered.sort_by_key(|a| std::cmp::Reverse(a.total_size.unwrap_or(0)))
                    }
                    SortBy::Speed => {
                        filtered.sort_by_key(|a| std::cmp::Reverse(a.speed_bps))
                    }
                    SortBy::Progress => filtered.sort_by(|a, b| {
                        let pa = a
                            .total_size
                            .map(|t| a.downloaded_bytes as f64 / t.max(1) as f64)
                            .unwrap_or(0.0);
                        let pb = b
                            .total_size
                            .map(|t| b.downloaded_bytes as f64 / t.max(1) as f64)
                            .unwrap_or(0.0);
                        pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
                    }),
                }

                if filtered.is_empty() {
                    egui::Frame::none()
                        .fill(Palette::BG_CARD)
                        .stroke(Stroke::new(1.0_f32, Palette::BORDER_SUBTLE))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(Margin::symmetric(24.0, 48.0))
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new("No Downloads Match Current Filter")
                                        .size(16.0)
                                        .strong()
                                        .color(Palette::TEXT_PRIMARY),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(
                                        "Paste a URL in the Quick-Add bar above, click '+ Add URL', or use 'Batch Wildcard' / 'Site Grabber'.",
                                    )
                                    .size(12.5)
                                    .color(Palette::TEXT_MUTED),
                                );
                            });
                        });
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for job in filtered {
                                self.render_idm_job_card(ui, job);
                                ui.add_space(5.0);
                            }
                        });
                }
            });

        // ====================================================================
        // 5. DETACHABLE FLOATING INSPECTOR MODAL (IF OPENED VIA POP-OUT)
        // ====================================================================
        if let Some(insp_id) = self.inspected_job_id.clone() {
            if let Some(job) = snapshots.iter().find(|j| j.id == insp_id) {
                let mut open = true;
                egui::Window::new(format!("IDM Live Segment Dialog — {}", job.filename))
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(true)
                    .default_size(Vec2::new(760.0, 460.0))
                    .show(ctx, |ui| {
                        self.render_job_inspector_contents(ui, job);
                    });
                if !open {
                    self.inspected_job_id = None;
                }
            }
        }

        // ====================================================================
        // 6. MODALS: ADD URL, BATCH WILDCARD, SITE GRABBER, SCHEDULER, SPEED
        // ====================================================================
        self.render_modals(ctx);
    }
}

impl OxideApp {
    fn render_idm_job_card(&mut self, ui: &mut egui::Ui, job: &DownloadJobSnapshot) {
        let is_selected = self.selected_job_id.as_deref() == Some(&job.id);
        let fill_color = if is_selected {
            Palette::BG_CARD_SELECTED
        } else {
            Palette::BG_CARD
        };
        let border_color = if is_selected {
            Palette::BORDER_FOCUS
        } else {
            Palette::BORDER_SUBTLE
        };

        let ratio = job
            .total_size
            .map(|t| {
                if t > 0 {
                    (job.downloaded_bytes as f32 / t as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

        egui::Frame::none()
            .fill(fill_color)
            .stroke(Stroke::new(1.0_f32, border_color))
            .rounding(Rounding::same(8.0))
            .inner_margin(Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Column 1: File Name + Category & Range Badges (270px)
                    ui.allocate_ui(Vec2::new(270.0, 38.0), |ui| {
                        ui.vertical(|ui| {
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(&job.filename)
                                            .size(13.5)
                                            .strong()
                                            .color(Palette::TEXT_PRIMARY),
                                    )
                                    .truncate()
                                    .sense(Sense::click()),
                                )
                                .clicked()
                            {
                                self.selected_job_id = Some(job.id.clone());
                                self.refresh_url_input = job.url.clone();
                                self.task_limit_kbps = job.speed_limit_bps / 1024;
                            }
                            ui.horizontal(|ui| {
                                draw_badge_pill(
                                    ui,
                                    job.category.folder_name(),
                                    category_color(job.category),
                                );
                                draw_badge_pill(
                                    ui,
                                    if job.supports_ranges {
                                        "MULTI-RANGE"
                                    } else {
                                        "SINGLE-STREAM"
                                    },
                                    if job.supports_ranges {
                                        Palette::CYAN
                                    } else {
                                        Palette::TEXT_MUTED
                                    },
                                );
                            });
                        });
                    });

                    // Column 2: Size (Downloaded / Total) (115px)
                    ui.allocate_ui(Vec2::new(115.0, 36.0), |ui| {
                        ui.vertical(|ui| {
                            let total_str = job
                                .total_size
                                .map(format_bytes)
                                .unwrap_or_else(|| "Stream".to_string());
                            ui.label(
                                egui::RichText::new(total_str)
                                    .monospace()
                                    .strong()
                                    .size(12.0)
                                    .color(Palette::TEXT_PRIMARY),
                            );
                            ui.label(
                                egui::RichText::new(format_bytes(job.downloaded_bytes))
                                    .monospace()
                                    .size(11.0)
                                    .color(Palette::TEXT_MUTED),
                            );
                        });
                    });

                    // Column 3: Status Badge + Progress Bar (185px)
                    ui.allocate_ui(Vec2::new(185.0, 36.0), |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                draw_badge_pill(
                                    ui,
                                    job.status.as_str(),
                                    status_color(job.status),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{:.1}%", ratio * 100.0))
                                        .monospace()
                                        .strong()
                                        .size(11.5)
                                        .color(Palette::TEXT_SECONDARY),
                                );
                            });
                            ui.add_space(3.0);
                            draw_progress_bar(
                                ui,
                                172.0,
                                6.0,
                                ratio,
                                status_color(job.status),
                            );
                        });
                    });

                    // Column 4: Time Left (75px)
                    ui.allocate_ui(Vec2::new(75.0, 36.0), |ui| {
                        let eta_str = format_eta(job.time_left_secs);
                        ui.label(
                            egui::RichText::new(eta_str)
                                .monospace()
                                .size(12.0)
                                .color(Palette::TEXT_SECONDARY),
                        );
                    });

                    // Column 5: Transfer Rate (100px)
                    ui.allocate_ui(Vec2::new(100.0, 36.0), |ui| {
                        let spd_str = if job.speed_bps > 0 {
                            format!("{}/s", format_bytes(job.speed_bps))
                        } else {
                            "—".to_string()
                        };
                        ui.label(
                            egui::RichText::new(spd_str)
                                .monospace()
                                .strong()
                                .size(12.5)
                                .color(if job.speed_bps > 0 {
                                    Palette::CYAN_BRIGHT
                                } else {
                                    Palette::TEXT_DIM
                                }),
                        );
                    });

                    // Column 6: Segments (75px)
                    ui.allocate_ui(Vec2::new(75.0, 36.0), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} / {}",
                                job.segments.len(),
                                job.max_connections
                            ))
                            .monospace()
                            .size(12.0)
                            .color(Palette::INDIGO),
                        );
                    });

                    // Column 7: Quick Actions
                    ui.horizontal(|ui| {
                        if action_ribbon_button(ui, "Inspect", Palette::BLUE, false) {
                            self.selected_job_id = Some(job.id.clone());
                            self.show_bottom_dock = true;
                            self.refresh_url_input = job.url.clone();
                            self.task_limit_kbps = job.speed_limit_bps / 1024;
                        }
                        if action_ribbon_button(ui, "Dialog", Palette::INDIGO, false) {
                            self.selected_job_id = Some(job.id.clone());
                            self.inspected_job_id = Some(job.id.clone());
                            self.refresh_url_input = job.url.clone();
                            self.task_limit_kbps = job.speed_limit_bps / 1024;
                        }

                        if job.status == DownloadStatus::Downloading {
                            if action_ribbon_button(ui, "Pause", Palette::AMBER, false) {
                                let mgr = self.manager.clone();
                                let id = job.id.clone();
                                tokio::spawn(async move {
                                    let _ = mgr.pause_download(&id).await;
                                });
                            }
                        } else if job.status == DownloadStatus::AwaitingRefresh {
                            if action_ribbon_button(ui, "Refresh Link", Palette::ORANGE, true) {
                                self.selected_job_id = Some(job.id.clone());
                                self.inspected_job_id = Some(job.id.clone());
                                self.inspector_tab = InspectorTab::CompletionAndRefresh;
                                self.refresh_url_input = job.url.clone();
                            }
                        } else if job.status != DownloadStatus::Completed
                            && action_ribbon_button(ui, "Resume", Palette::EMERALD_BRIGHT, false)
                        {
                            let mgr = self.manager.clone();
                            let id = job.id.clone();
                            tokio::spawn(async move {
                                let _ = mgr.resume_download(&id).await;
                            });
                        }
                    });
                });
            });
    }

    fn render_job_inspector_contents(&mut self, ui: &mut egui::Ui, job: &DownloadJobSnapshot) {
        // Top Header & Tab Bar inside Inspector
        ui.horizontal(|ui| {
            draw_badge_pill(ui, job.status.as_str(), status_color(job.status));
            ui.label(
                egui::RichText::new(&job.filename)
                    .size(14.0)
                    .strong()
                    .color(Palette::TEXT_PRIMARY),
            );
            ui.separator();

            if ui
                .selectable_label(
                    self.inspector_tab == InspectorTab::DownloadStatus,
                    egui::RichText::new("1. Download Status & Dynamic Segments").strong(),
                )
                .clicked()
            {
                self.inspector_tab = InspectorTab::DownloadStatus;
            }
            if ui
                .selectable_label(
                    self.inspector_tab == InspectorTab::SpeedLimiter,
                    egui::RichText::new("2. Speed Limiter").strong(),
                )
                .clicked()
            {
                self.inspector_tab = InspectorTab::SpeedLimiter;
            }
            if ui
                .selectable_label(
                    self.inspector_tab == InspectorTab::CompletionAndRefresh,
                    egui::RichText::new("3. Link Refresh (403/410) & Checksum").strong(),
                )
                .clicked()
            {
                self.inspector_tab = InspectorTab::CompletionAndRefresh;
                if self.refresh_url_input.is_empty() {
                    self.refresh_url_input = job.url.clone();
                }
            }
        });
        ui.add_space(6.0);

        match self.inspector_tab {
            InspectorTab::DownloadStatus => {
                // 4 Metric Cards Row
                ui.horizontal(|ui| {
                    kpi_card(
                        ui,
                        "DOWNLOADED / TOTAL",
                        &format!(
                            "{} / {}",
                            format_bytes(job.downloaded_bytes),
                            job.total_size
                                .map(format_bytes)
                                .unwrap_or_else(|| "Stream".into())
                        ),
                        Palette::TEXT_PRIMARY,
                    );
                    kpi_card(
                        ui,
                        "REAL-TIME SPEED",
                        &format!("{}/s", format_bytes(job.speed_bps)),
                        Palette::CYAN_BRIGHT,
                    );
                    kpi_card(
                        ui,
                        "ESTIMATED TIME LEFT",
                        &format_eta(job.time_left_secs),
                        Palette::EMERALD_BRIGHT,
                    );
                    kpi_card(
                        ui,
                        "DYNAMIC CONNECTIONS",
                        &format!(
                            "{} Active / {} Max",
                            job.segments.len(),
                            job.max_connections
                        ),
                        Palette::INDIGO,
                    );
                });

                ui.add_space(6.0);

                // Visual 32-Segment Dynamic Byte Map
                render_colorful_segment_map(ui, job);

                ui.add_space(6.0);

                // Per-Connection Table
                egui::ScrollArea::vertical()
                    .max_height(110.0)
                    .show(ui, |ui| {
                        for seg in &job.segments {
                            let span = (seg.end.saturating_sub(seg.start) + 1).max(1);
                            let done = seg.current.saturating_sub(seg.start).min(span);
                            let ratio = (done as f32 / span as f32).clamp(0.0, 1.0);

                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [55.0, 16.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!("Conn #{:02}", seg.id))
                                            .monospace()
                                            .strong()
                                            .size(11.0)
                                            .color(Palette::CYAN_BRIGHT),
                                    ),
                                );
                                ui.add_sized(
                                    [195.0, 16.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!(
                                            "{:>10} .. {:<10}",
                                            seg.start, seg.end
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(Palette::TEXT_MUTED),
                                    ),
                                );
                                ui.add_sized(
                                    [135.0, 16.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!(
                                            "{} / {}",
                                            format_bytes(done),
                                            format_bytes(span)
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(Palette::TEXT_SECONDARY),
                                    ),
                                );
                                draw_progress_bar(
                                    ui,
                                    120.0,
                                    6.0,
                                    ratio,
                                    segment_state_color(seg.state),
                                );
                                ui.add_sized(
                                    [85.0, 16.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!(
                                            "{}/s",
                                            format_bytes(seg.speed_bps)
                                        ))
                                        .monospace()
                                        .size(11.0)
                                        .color(Palette::CYAN_BRIGHT),
                                    ),
                                );
                                draw_badge_pill(
                                    ui,
                                    seg.state.as_str(),
                                    segment_state_color(seg.state),
                                );
                            });
                        }
                    });
            }

            InspectorTab::SpeedLimiter => {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Per-Task Token-Bucket Bandwidth Limiter")
                                .size(13.0)
                                .strong()
                                .color(Palette::CYAN_BRIGHT),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Throttle this download in real time without dropping active TCP connections:",
                            )
                            .size(11.5)
                            .color(Palette::TEXT_MUTED),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            for (preset_label, kbps) in [
                                ("Unlimited", 0u64),
                                ("512 KB/s", 512),
                                ("2 MB/s", 2048),
                                ("5 MB/s", 5120),
                                ("10 MB/s", 10240),
                            ] {
                                if action_ribbon_button(ui, preset_label, Palette::BLUE, false) {
                                    self.task_limit_kbps = kbps;
                                    let _ = self
                                        .manager
                                        .set_task_speed_limit(&job.id, kbps * 1024);
                                }
                            }
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Custom Task Limit (KB/s):");
                            ui.add(
                                egui::DragValue::new(&mut self.task_limit_kbps)
                                    .speed(32.0)
                                    .range(0..=1_000_000),
                            );
                            if action_ribbon_button(
                                ui,
                                "Apply Task Limit",
                                Palette::EMERALD_BRIGHT,
                                true,
                            ) {
                                let _ = self
                                    .manager
                                    .set_task_speed_limit(&job.id, self.task_limit_kbps * 1024);
                            }
                        });
                    });
                });
            }

            InspectorTab::CompletionAndRefresh => {
                ui.columns(2, |cols| {
                    cols[0].vertical(|ui| {
                        ui.label(
                            egui::RichText::new(
                                "Expired Link & Session Refresh (HTTP 403 / 410 Recovery)",
                            )
                            .size(13.0)
                            .strong()
                            .color(Palette::ORANGE),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Update the signed URL, Cookie, or Referer while keeping all downloaded byte segments intact:",
                            )
                            .size(11.0)
                            .color(Palette::TEXT_MUTED),
                        );
                        ui.horizontal(|ui| {
                            ui.label("New URL:");
                            ui.text_edit_singleline(&mut self.refresh_url_input);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Cookie:");
                            ui.text_edit_singleline(&mut self.refresh_cookie_input);
                            ui.label("Referer:");
                            ui.text_edit_singleline(&mut self.refresh_referer_input);
                        });
                        if action_ribbon_button(
                            ui,
                            "Refresh Link & Resume Intact Segments",
                            Palette::EMERALD_BRIGHT,
                            true,
                        ) && !self.refresh_url_input.trim().is_empty()
                        {
                            let mut headers = HashMap::new();
                            if !self.refresh_cookie_input.trim().is_empty() {
                                headers.insert(
                                    "Cookie".to_string(),
                                    self.refresh_cookie_input.trim().to_string(),
                                );
                            }
                            if !self.refresh_referer_input.trim().is_empty() {
                                headers.insert(
                                    "Referer".to_string(),
                                    self.refresh_referer_input.trim().to_string(),
                                );
                            }
                            let payload = RefreshLinkPayload {
                                new_url: self.refresh_url_input.trim().to_string(),
                                new_headers: headers,
                            };
                            let mgr = self.manager.clone();
                            let id = job.id.clone();
                            tokio::spawn(async move {
                                let _ = mgr.refresh_download_link(&id, payload, true).await;
                            });
                            self.inspector_tab = InspectorTab::DownloadStatus;
                        }
                    });

                    cols[1].vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Target File & SHA-256 Integrity Verification")
                                .size(13.0)
                                .strong()
                                .color(Palette::CYAN_BRIGHT),
                        );
                        ui.label(
                            egui::RichText::new(format!("Saved Path: {}", job.target_path))
                                .monospace()
                                .size(11.0)
                                .color(Palette::TEXT_SECONDARY),
                        );
                        if let Some(ref err) = job.error_message {
                            ui.label(
                                egui::RichText::new(format!("Last Diagnostic: {}", err))
                                    .size(11.0)
                                    .color(Palette::ROSE),
                            );
                        }
                        if action_ribbon_button(
                            ui,
                            "Compute SHA-256 Checksum Now",
                            Palette::INDIGO,
                            false,
                        ) {
                            match compute_file_sha256(Path::new(&job.target_path)) {
                                Ok(digest) => {
                                    self.verify_hash_result = Some(format!("SHA-256: {}", digest));
                                }
                                Err(e) => {
                                    self.verify_hash_result =
                                        Some(format!("Checksum Error: {}", e));
                                }
                            }
                        }
                        if let Some(ref digest_msg) = self.verify_hash_result {
                            ui.label(
                                egui::RichText::new(digest_msg)
                                    .monospace()
                                    .size(10.5)
                                    .color(Palette::EMERALD_BRIGHT),
                            );
                        }
                    });
                });
            }
        }
    }

    fn render_modals(&mut self, ctx: &egui::Context) {
        // 1. ADD NEW DOWNLOAD MODAL
        if self.show_add_modal {
            egui::Window::new("Add New Download Job")
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(520.0, 320.0))
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Enter HTTP / HTTPS / HLS (.m3u8) / DASH (.mpd) URL & Custom Headers",
                        )
                        .size(12.0)
                        .color(Palette::TEXT_MUTED),
                    );
                    ui.add_space(6.0);

                    egui::Grid::new("add_dl_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Target URL:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_url)
                                    .hint_text("https://...")
                                    .desired_width(360.0),
                            );
                            ui.end_row();

                            ui.label("Custom Filename:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_filename)
                                    .hint_text("Leave blank to auto-detect from Content-Disposition")
                                    .desired_width(360.0),
                            );
                            ui.end_row();

                            ui.label("Save Directory:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_save_dir)
                                    .desired_width(360.0),
                            );
                            ui.end_row();

                            ui.label("Max Connections:");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::Slider::new(&mut self.add_connections, 1..=32)
                                        .text("Dynamic Segments"),
                                );
                            });
                            ui.end_row();

                            ui.label("Cookie Header:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_cookie)
                                    .hint_text("Optional session_id=...")
                                    .desired_width(360.0),
                            );
                            ui.end_row();

                            ui.label("Referer Header:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_referer)
                                    .hint_text("Optional https://...")
                                    .desired_width(360.0),
                            );
                            ui.end_row();

                            ui.label("Expected SHA-256:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.add_sha256)
                                    .hint_text("Optional 64-character hex checksum")
                                    .desired_width(360.0),
                            );
                            ui.end_row();
                        });

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if action_ribbon_button(ui, "Start Download Now", Palette::BLUE, true)
                            && !self.add_url.trim().is_empty()
                        {
                            self.submit_add_modal(true);
                            self.show_add_modal = false;
                        }
                        if action_ribbon_button(ui, "Add to Queue Later", Palette::INDIGO, false)
                            && !self.add_url.trim().is_empty()
                        {
                            self.submit_add_modal(false);
                            self.show_add_modal = false;
                        }
                        if action_ribbon_button(ui, "Cancel", Palette::ROSE, false) {
                            self.show_add_modal = false;
                        }
                    });
                });
        }

        // 2. BATCH WILDCARD GENERATOR MODAL
        if self.show_batch_modal {
            egui::Window::new("IDM Batch Wildcard URL Generator")
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(520.0, 220.0))
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Expand numeric ranges [01-20] or alphabetic ranges [a-z] into queued batch jobs:",
                        )
                        .size(12.0)
                        .color(Palette::TEXT_MUTED),
                    );
                    ui.add_space(6.0);
                    ui.label("Pattern URL:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.batch_pattern)
                            .desired_width(f32::INFINITY),
                    );

                    let preview = expand_wildcard_url(self.batch_pattern.trim(), Some(10));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Preview ({} URLs): {} ... {}",
                            preview.len(),
                            preview.first().cloned().unwrap_or_default(),
                            preview.last().cloned().unwrap_or_default()
                        ))
                        .monospace()
                        .size(11.0)
                        .color(Palette::CYAN_BRIGHT),
                    );

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if action_ribbon_button(
                            ui,
                            "Expand & Start Batch",
                            Palette::EMERALD_BRIGHT,
                            true,
                        ) && !self.batch_pattern.trim().is_empty()
                        {
                            let urls = expand_wildcard_url(self.batch_pattern.trim(), Some(10));
                            let mgr = self.manager.clone();
                            let conns = self.batch_connections;
                            let qid = self.batch_queue_id.clone();
                            tokio::spawn(async move {
                                for u in urls {
                                    let req = DownloadRequest {
                                        url: u,
                                        filename: None,
                                        save_dir: None,
                                        headers: HashMap::new(),
                                        max_segments: Some(conns),
                                        speed_limit_bps: None,
                                        queue_id: Some(qid.clone()),
                                        expected_sha256: None,
                                    };
                                    let _ = mgr.add_download(req, true).await;
                                }
                            });
                            self.show_batch_modal = false;
                        }
                        if action_ribbon_button(ui, "Cancel", Palette::ROSE, false) {
                            self.show_batch_modal = false;
                        }
                    });
                });
        }

        // 3. IDM SITE GRABBER / SPIDER MODAL (`oxide-spider`)
        if self.show_spider_modal {
            egui::Window::new("IDM Site Grabber & Link Spider")
                .collapsible(false)
                .resizable(true)
                .default_size(Vec2::new(640.0, 420.0))
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Webpage URL:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.spider_url).desired_width(340.0),
                        );
                        if action_ribbon_button(ui, "Crawl Webpage", Palette::TEAL, true)
                            && !self.spider_url.trim().is_empty()
                        {
                            let target = self.spider_url.trim().to_string();
                            let exts: HashSet<String> = self
                                .spider_extensions
                                .split(',')
                                .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                                .filter(|s| !s.is_empty())
                                .collect();
                            let filter = SpiderFilter {
                                allowed_extensions: exts,
                                same_domain_only: self.spider_same_domain,
                                max_depth: 1,
                            };
                            let results_arc = self.spider_results.clone();
                            let status_arc = self.spider_status.clone();
                            *status_arc.write() = format!("Crawling {} ...", target);

                            tokio::spawn(async move {
                                match crawl_page(&target, &filter).await {
                                    Ok(links) => {
                                        *status_arc.write() =
                                            format!("Found {} matching files.", links.len());
                                        *results_arc.write() =
                                            links.into_iter().map(|l| (true, l)).collect();
                                    }
                                    Err(e) => {
                                        *status_arc.write() = format!("Spider error: {}", e);
                                    }
                                }
                            });
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Extensions:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.spider_extensions)
                                .desired_width(300.0),
                        );
                        ui.checkbox(&mut self.spider_same_domain, "Same domain only");
                    });

                    let status_msg = self.spider_status.read().clone();
                    ui.label(
                        egui::RichText::new(status_msg)
                            .size(11.5)
                            .color(Palette::CYAN_BRIGHT),
                    );
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            let mut list = self.spider_results.write();
                            for (checked, link) in list.iter_mut() {
                                ui.horizontal(|ui| {
                                    ui.checkbox(checked, "");
                                    draw_badge_pill(ui, &link.extension, Palette::INDIGO);
                                    ui.label(
                                        egui::RichText::new(&link.filename)
                                            .strong()
                                            .color(Palette::TEXT_PRIMARY),
                                    );
                                    ui.label(
                                        egui::RichText::new(&link.url)
                                            .monospace()
                                            .size(10.5)
                                            .color(Palette::TEXT_MUTED),
                                    );
                                });
                            }
                        });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if action_ribbon_button(
                            ui,
                            "Download Selected Links",
                            Palette::EMERALD_BRIGHT,
                            true,
                        ) {
                            let selected: Vec<DiscoveredLink> = self
                                .spider_results
                                .read()
                                .iter()
                                .filter(|(c, _)| *c)
                                .map(|(_, l)| l.clone())
                                .collect();
                            let mgr = self.manager.clone();
                            tokio::spawn(async move {
                                for item in selected {
                                    let req = DownloadRequest {
                                        url: item.url,
                                        filename: Some(item.filename),
                                        save_dir: None,
                                        headers: HashMap::new(),
                                        max_segments: Some(8),
                                        speed_limit_bps: None,
                                        queue_id: Some("default".to_string()),
                                        expected_sha256: None,
                                    };
                                    let _ = mgr.add_download(req, true).await;
                                }
                            });
                            self.show_spider_modal = false;
                        }
                        if action_ribbon_button(ui, "Close", Palette::ROSE, false) {
                            self.show_spider_modal = false;
                        }
                    });
                });
        }

        // 4. QUEUE MANAGER & CRON SCHEDULER MODAL
        if self.show_scheduler_modal {
            egui::Window::new("IDM Queue Manager & Cron Scheduler")
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(460.0, 250.0))
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    egui::Grid::new("sched_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Queue ID:");
                            ui.text_edit_singleline(&mut self.sched_queue_id);
                            ui.end_row();

                            ui.label("Queue Name:");
                            ui.text_edit_singleline(&mut self.sched_queue_name);
                            ui.end_row();

                            ui.label("Max Concurrent Jobs:");
                            ui.add(
                                egui::Slider::new(&mut self.sched_max_concurrent, 1..=32)
                                    .text("tasks"),
                            );
                            ui.end_row();

                            ui.label("Start Time (UTC HH:MM):");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.sched_start_cron)
                                    .hint_text("e.g. 01:00 (optional)"),
                            );
                            ui.end_row();

                            ui.label("Stop Time (UTC HH:MM):");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.sched_stop_cron)
                                    .hint_text("e.g. 07:00 (optional)"),
                            );
                            ui.end_row();

                            ui.label("Max Auto-Retries:");
                            ui.add(egui::DragValue::new(&mut self.sched_max_retries).range(0..=20));
                            ui.end_row();
                        });

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if action_ribbon_button(
                            ui,
                            "Save Queue Schedule",
                            Palette::PURPLE,
                            true,
                        ) {
                            let cfg = QueueConfig {
                                id: self.sched_queue_id.trim().to_string(),
                                name: self.sched_queue_name.trim().to_string(),
                                max_concurrent: self.sched_max_concurrent,
                                speed_limit_bps: 0,
                                start_cron: if self.sched_start_cron.trim().is_empty() {
                                    None
                                } else {
                                    Some(self.sched_start_cron.trim().to_string())
                                },
                                stop_cron: if self.sched_stop_cron.trim().is_empty() {
                                    None
                                } else {
                                    Some(self.sched_stop_cron.trim().to_string())
                                },
                                max_retries: self.sched_max_retries,
                                post_action: "none".to_string(),
                            };
                            let _ = self.manager.db().upsert_queue(&cfg);
                            self.show_scheduler_modal = false;
                        }
                        if action_ribbon_button(ui, "Cancel", Palette::ROSE, false) {
                            self.show_scheduler_modal = false;
                        }
                    });
                });
        }

        // 5. GLOBAL SPEED LIMITER MODAL
        if self.show_speed_modal {
            egui::Window::new("Global Engine Bandwidth Throttling")
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(440.0, 170.0))
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Quick Presets:");
                    ui.horizontal(|ui| {
                        for (label, kbps) in [
                            ("Unlimited", 0u64),
                            ("1 MB/s", 1024),
                            ("5 MB/s", 5120),
                            ("10 MB/s", 10240),
                            ("25 MB/s", 25600),
                        ] {
                            if action_ribbon_button(ui, label, Palette::CYAN_BRIGHT, false) {
                                self.global_limit_kbps = kbps;
                                self.manager.set_global_speed_limit(kbps * 1024);
                                self.show_speed_modal = false;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Custom Global Cap (KB/s):");
                        ui.add(
                            egui::DragValue::new(&mut self.global_limit_kbps)
                                .speed(64.0)
                                .range(0..=1_000_000),
                        );
                        if action_ribbon_button(ui, "Apply Global Cap", Palette::BLUE, true) {
                            self.manager
                                .set_global_speed_limit(self.global_limit_kbps * 1024);
                            self.show_speed_modal = false;
                        }
                        if action_ribbon_button(ui, "Close", Palette::ROSE, false) {
                            self.show_speed_modal = false;
                        }
                    });
                });
        }
    }

    fn submit_add_modal(&mut self, start_now: bool) {
        let mut headers = HashMap::new();
        if !self.add_cookie.trim().is_empty() {
            headers.insert("Cookie".to_string(), self.add_cookie.trim().to_string());
        }
        if !self.add_referer.trim().is_empty() {
            headers.insert("Referer".to_string(), self.add_referer.trim().to_string());
        }
        if !self.add_user_agent.trim().is_empty() {
            headers.insert(
                "User-Agent".to_string(),
                self.add_user_agent.trim().to_string(),
            );
        }

        let req = DownloadRequest {
            url: self.add_url.trim().to_string(),
            filename: if self.add_filename.trim().is_empty() {
                None
            } else {
                Some(self.add_filename.trim().to_string())
            },
            save_dir: if self.add_save_dir.trim().is_empty() {
                None
            } else {
                Some(self.add_save_dir.trim().to_string())
            },
            headers,
            max_segments: Some(self.add_connections),
            speed_limit_bps: None,
            queue_id: Some(self.add_queue_id.trim().to_string()),
            expected_sha256: if self.add_sha256.trim().is_empty() {
                None
            } else {
                Some(self.add_sha256.trim().to_string())
            },
        };

        let mgr = self.manager.clone();
        tokio::spawn(async move {
            let _ = mgr.add_download(req, start_now).await;
        });

        self.add_url.clear();
        self.add_filename.clear();
        self.add_cookie.clear();
        self.add_referer.clear();
        self.add_sha256.clear();
    }
}

fn sidebar_nav_row(
    ui: &mut egui::Ui,
    current: &mut SidebarFilter,
    target: SidebarFilter,
    label: &str,
    count: usize,
    dot_color: Color32,
) {
    let is_selected = *current == target;
    let bg = if is_selected {
        Color32::from_rgba_unmultiplied(dot_color.r(), dot_color.g(), dot_color.b(), 36)
    } else {
        Color32::TRANSPARENT
    };
    let border = if is_selected {
        Stroke::new(1.0_f32, dot_color)
    } else {
        Stroke::NONE
    };

    egui::Frame::none()
        .fill(bg)
        .stroke(border)
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(8.0, 5.0))
        .show(ui, |ui| {
            let resp = ui.horizontal(|ui| {
                // Colored Category / Filter Dot
                let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 4.0, dot_color);

                ui.label(
                    egui::RichText::new(label)
                        .size(12.0)
                        .strong()
                        .color(if is_selected {
                            Palette::TEXT_PRIMARY
                        } else {
                            Palette::TEXT_SECONDARY
                        }),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(count.to_string())
                            .monospace()
                            .size(11.0)
                            .color(if is_selected {
                                dot_color
                            } else {
                                Palette::TEXT_MUTED
                            }),
                    );
                });
            });
            if ui
                .interact(resp.response.rect, ui.next_auto_id(), Sense::click())
                .clicked()
            {
                *current = target;
            }
        });
}

fn kpi_card(ui: &mut egui::Ui, title: &str, value: &str, value_color: Color32) {
    egui::Frame::none()
        .fill(Palette::BG_CARD)
        .stroke(Stroke::new(1.0_f32, Palette::BORDER_SUBTLE))
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(12.0, 6.0))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .size(10.0)
                        .strong()
                        .color(Palette::TEXT_MUTED),
                );
                ui.label(
                    egui::RichText::new(value)
                        .monospace()
                        .strong()
                        .size(13.0)
                        .color(value_color),
                );
            });
        });
}

fn render_colorful_segment_map(ui: &mut egui::Ui, job: &DownloadJobSnapshot) {
    let total = job.total_size.unwrap_or(1).max(1);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, Rounding::same(5.0), Palette::BG_INPUT);
    ui.painter().rect_stroke(
        rect,
        Rounding::same(5.0),
        Stroke::new(1.0_f32, Palette::BORDER_SUBTLE),
    );

    let segment_palette = [
        Palette::CYAN_BRIGHT,
        Palette::BLUE,
        Palette::INDIGO,
        Palette::TEAL,
        Palette::PURPLE,
    ];

    for (i, seg) in job.segments.iter().enumerate() {
        let start_x = rect.min.x + (seg.start as f32 / total as f32) * rect.width();
        let cur_x = rect.min.x + (seg.current as f32 / total as f32) * rect.width();
        let end_x = rect.min.x + (seg.end as f32 / total as f32) * rect.width();

        let dl_w = (cur_x - start_x).max(1.5);
        let dl_rect =
            Rect::from_min_size(Pos2::new(start_x, rect.min.y + 1.0), Vec2::new(dl_w, rect.height() - 2.0));
        let col = if seg.completed {
            Palette::EMERALD
        } else {
            segment_palette[i % segment_palette.len()]
        };

        ui.painter().rect_filled(dl_rect, Rounding::ZERO, col);
        ui.painter().line_segment(
            [Pos2::new(end_x, rect.min.y), Pos2::new(end_x, rect.max.y)],
            Stroke::new(1.0_f32, Palette::BG_CANVAS),
        );
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn format_eta(secs_opt: Option<u64>) -> String {
    let Some(secs) = secs_opt else {
        return "—".to_string();
    };
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}
