use eframe::egui::{self, Color32, Margin, Pos2, Rect, Rounding, Stroke, Vec2};
use oxide_core::{
    expand_wildcard_url, Category, DownloadJobSnapshot, DownloadManager, DownloadRequest,
    DownloadStatus, QueueConfig, RefreshLinkPayload,
};
use std::collections::HashMap;

#[derive(Clone, PartialEq, Eq)]
pub enum SidebarFilter {
    All,
    Unfinished,
    Finished,
    Category(Category),
    Queue(String),
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
    pub search_query: String,
    pub selected_job_id: Option<String>,
    pub inspected_job_id: Option<String>,
    pub inspector_tab: InspectorTab,

    // Add URL Modal state
    pub show_add_modal: bool,
    pub add_url: String,
    pub add_filename: String,
    pub add_connections: usize,
    pub add_cookie: String,
    pub add_sha256: String,

    // Batch Wildcard Modal state
    pub show_batch_modal: bool,
    pub batch_pattern: String,

    // Scheduler Modal state
    pub show_scheduler_modal: bool,
    pub sched_queue_id: String,
    pub sched_queue_name: String,
    pub sched_max_concurrent: usize,
    pub sched_start_cron: String,
    pub sched_stop_cron: String,
    pub sched_max_retries: u32,

    // Speed Limiter inputs
    pub global_limit_kbps: u64,
    pub task_limit_kbps: u64,

    // Refresh Link inputs
    pub refresh_url_input: String,
    pub refresh_cookie_input: String,
    pub refresh_referer_input: String,
}

impl OxideApp {
    pub fn new(manager: DownloadManager) -> Self {
        Self {
            manager,
            filter: SidebarFilter::All,
            search_query: String::new(),
            selected_job_id: None,
            inspected_job_id: None,
            inspector_tab: InspectorTab::DownloadStatus,
            show_add_modal: false,
            add_url: String::new(),
            add_filename: String::new(),
            add_connections: 8,
            add_cookie: String::new(),
            add_sha256: String::new(),
            show_batch_modal: false,
            batch_pattern: String::from("https://example.com/archive_part_[01-05].zip"),
            show_scheduler_modal: false,
            sched_queue_id: String::from("default"),
            sched_queue_name: String::from("Main Download Queue"),
            sched_max_concurrent: 4,
            sched_start_cron: String::new(),
            sched_stop_cron: String::new(),
            sched_max_retries: 5,
            global_limit_kbps: 0,
            task_limit_kbps: 0,
            refresh_url_input: String::new(),
            refresh_cookie_input: String::new(),
            refresh_referer_input: String::new(),
        }
    }
}

impl eframe::App for OxideApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let snapshots = self.manager.list_snapshots();
        let has_active = snapshots
            .iter()
            .any(|s| s.status == DownloadStatus::Downloading || s.status == DownloadStatus::Connecting);

        if has_active {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(Color32::from_rgb(226, 232, 240));
        visuals.panel_fill = Color32::from_rgb(11, 13, 19);
        visuals.window_fill = Color32::from_rgb(18, 22, 32);
        ctx.set_visuals(visuals);

        // 1. TOP ACTION TOOLBAR (IDM Style)
        egui::TopBottomPanel::top("idm_toolbar")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(18, 22, 32))
                    .inner_margin(Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("OXIDE")
                            .strong()
                            .color(Color32::from_rgb(59, 130, 246))
                            .size(17.0),
                    );
                    ui.label(
                        egui::RichText::new("IDM Engine")
                            .color(Color32::from_rgb(148, 163, 184))
                            .size(12.0),
                    );
                    ui.separator();

                    if ui
                        .button(
                            egui::RichText::new("+ Add URL")
                                .strong()
                                .color(Color32::from_rgb(96, 165, 250)),
                        )
                        .clicked()
                    {
                        self.show_add_modal = true;
                    }

                    if ui.button("Batch / Wildcard").clicked() {
                        self.show_batch_modal = true;
                    }

                    ui.separator();

                    if ui
                        .button(
                            egui::RichText::new("Resume")
                                .color(Color32::from_rgb(52, 211, 153)),
                        )
                        .clicked()
                    {
                        if let Some(ref id) = self.selected_job_id {
                            let mgr = self.manager.clone();
                            let job_id = id.clone();
                            tokio::spawn(async move {
                                let _ = mgr.resume_download(&job_id).await;
                            });
                        }
                    }

                    if ui
                        .button(
                            egui::RichText::new("Stop")
                                .color(Color32::from_rgb(251, 191, 36)),
                        )
                        .clicked()
                    {
                        if let Some(ref id) = self.selected_job_id {
                            let mgr = self.manager.clone();
                            let job_id = id.clone();
                            tokio::spawn(async move {
                                let _ = mgr.pause_download(&job_id).await;
                            });
                        }
                    }

                    if ui.button("Stop All").clicked() {
                        let mgr = self.manager.clone();
                        tokio::spawn(async move {
                            let _ = mgr.pause_all().await;
                        });
                    }

                    if ui
                        .button(
                            egui::RichText::new("Delete")
                                .color(Color32::from_rgb(244, 63, 94)),
                        )
                        .clicked()
                    {
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

                    if ui.button("Scheduler & Queues").clicked() {
                        self.show_scheduler_modal = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("Live Web UI: http://127.0.0.1:46890")
                                .monospace()
                                .size(11.0)
                                .color(Color32::from_rgb(16, 185, 129)),
                        );
                    });
                });
            });

        // 2. LEFT SIDEBAR TREE (`All Downloads`, `Unfinished`, `Finished`, Categories, Queues)
        egui::SidePanel::left("idm_sidebar")
            .resizable(false)
            .exact_width(210.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(15, 19, 28))
                    .inner_margin(Margin::same(10.0)),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("TASKS")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                ui.add_space(4.0);

                sidebar_item(
                    ui,
                    &mut self.filter,
                    SidebarFilter::All,
                    "All Downloads",
                    snapshots.len(),
                );
                sidebar_item(
                    ui,
                    &mut self.filter,
                    SidebarFilter::Unfinished,
                    "Unfinished",
                    snapshots
                        .iter()
                        .filter(|s| s.status != DownloadStatus::Completed)
                        .count(),
                );
                sidebar_item(
                    ui,
                    &mut self.filter,
                    SidebarFilter::Finished,
                    "Finished",
                    snapshots
                        .iter()
                        .filter(|s| s.status == DownloadStatus::Completed)
                        .count(),
                );

                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("CATEGORIES")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                ui.add_space(4.0);

                for (cat, label) in [
                    (Category::Compressed, "Compressed"),
                    (Category::Documents, "Documents"),
                    (Category::Music, "Music"),
                    (Category::Programs, "Programs"),
                    (Category::Videos, "Video"),
                    (Category::General, "General"),
                ] {
                    let count = snapshots.iter().filter(|s| s.category == cat).count();
                    sidebar_item(
                        ui,
                        &mut self.filter,
                        SidebarFilter::Category(cat),
                        label,
                        count,
                    );
                }

                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("QUEUES")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_rgb(148, 163, 184)),
                );
                ui.add_space(4.0);

                let queues = self.manager.db().list_queues().unwrap_or_default();
                for q in queues {
                    let count = snapshots.iter().filter(|s| s.queue_id == q.id).count();
                    sidebar_item(
                        ui,
                        &mut self.filter,
                        SidebarFilter::Queue(q.id),
                        &q.name,
                        count,
                    );
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    let total_speed: u64 = snapshots
                        .iter()
                        .filter(|s| s.status == DownloadStatus::Downloading)
                        .map(|s| s.speed_bps)
                        .sum();
                    ui.label(
                        egui::RichText::new(format!("Total Speed: {}/s", format_bytes(total_speed)))
                            .monospace()
                            .size(11.5)
                            .color(Color32::from_rgb(34, 211, 238)),
                    );
                });
            });

        // 3. MAIN DATA TABLE (`File Name`, `Size`, `Status`, `Time Left`, `Transfer Rate`, `Last Try`)
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(11, 13, 19))
                    .inner_margin(Margin::same(12.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Search:").size(12.0));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("Filter by filename or URL...")
                            .desired_width(260.0),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(
                            "Click any download row to select; click 'Inspect' to open the Live Segmented Progress Window.",
                        )
                        .size(11.5)
                        .color(Color32::from_rgb(148, 163, 184)),
                    );
                });
                ui.add_space(8.0);

                // Table Header
                egui::Frame::none()
                    .fill(Color32::from_rgb(18, 22, 32))
                    .inner_margin(Margin::symmetric(10.0, 6.0))
                    .rounding(Rounding::same(4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_sized([260.0, 16.0], egui::Label::new(egui::RichText::new("File Name").strong().size(11.5)));
                            ui.add_sized([95.0, 16.0], egui::Label::new(egui::RichText::new("Size").strong().size(11.5)));
                            ui.add_sized([130.0, 16.0], egui::Label::new(egui::RichText::new("Status").strong().size(11.5)));
                            ui.add_sized([80.0, 16.0], egui::Label::new(egui::RichText::new("Time Left").strong().size(11.5)));
                            ui.add_sized([105.0, 16.0], egui::Label::new(egui::RichText::new("Transfer Rate").strong().size(11.5)));
                            ui.add_sized([75.0, 16.0], egui::Label::new(egui::RichText::new("Segments").strong().size(11.5)));
                            ui.add_sized([90.0, 16.0], egui::Label::new(egui::RichText::new("Actions").strong().size(11.5)));
                        });
                    });

                ui.add_space(4.0);

                let q_lower = self.search_query.to_lowercase();
                let filtered: Vec<&DownloadJobSnapshot> = snapshots
                    .iter()
                    .filter(|s| match &self.filter {
                        SidebarFilter::All => true,
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

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for job in filtered {
                        let is_selected = self.selected_job_id.as_deref() == Some(&job.id);
                        let row_bg = if is_selected {
                            Color32::from_rgb(30, 41, 59)
                        } else {
                            Color32::from_rgb(17, 21, 30)
                        };

                        egui::Frame::none()
                            .fill(row_bg)
                            .inner_margin(Margin::symmetric(10.0, 7.0))
                            .rounding(Rounding::same(5.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_sized(
                                            [260.0, 18.0],
                                            egui::Label::new(
                                                egui::RichText::new(&job.filename)
                                                    .color(Color32::from_rgb(241, 245, 249))
                                                    .size(12.5),
                                            )
                                            .truncate(),
                                        )
                                        .clicked()
                                    {
                                        self.selected_job_id = Some(job.id.clone());
                                    }

                                    let size_text = match job.total_size {
                                        Some(t) => format_bytes(t),
                                        None => format_bytes(job.downloaded_bytes),
                                    };
                                    ui.add_sized(
                                        [95.0, 18.0],
                                        egui::Label::new(egui::RichText::new(size_text).monospace().size(11.5)),
                                    );

                                    let pct = job
                                        .total_size
                                        .map(|t| {
                                            if t > 0 {
                                                (job.downloaded_bytes as f64 / t as f64) * 100.0
                                            } else {
                                                0.0
                                            }
                                        })
                                        .unwrap_or(0.0);
                                    let status_str = format!("{} ({:.1}%)", job.status.as_str(), pct);
                                    ui.add_sized(
                                        [130.0, 18.0],
                                        egui::Label::new(
                                            egui::RichText::new(status_str)
                                                .monospace()
                                                .size(11.0)
                                                .color(status_color(job.status)),
                                        ),
                                    );

                                    let eta_str = job
                                        .time_left_secs
                                        .map(|s| format!("{}s", s))
                                        .unwrap_or_else(|| "-".into());
                                    ui.add_sized(
                                        [80.0, 18.0],
                                        egui::Label::new(egui::RichText::new(eta_str).monospace().size(11.5)),
                                    );

                                    let speed_str = if job.speed_bps > 0 {
                                        format!("{}/s", format_bytes(job.speed_bps))
                                    } else {
                                        "-".into()
                                    };
                                    ui.add_sized(
                                        [105.0, 18.0],
                                        egui::Label::new(
                                            egui::RichText::new(speed_str)
                                                .monospace()
                                                .size(11.5)
                                                .color(Color32::from_rgb(34, 211, 238)),
                                        ),
                                    );

                                    ui.add_sized(
                                        [75.0, 18.0],
                                        egui::Label::new(
                                            egui::RichText::new(format!(
                                                "{}/{}",
                                                job.segments.len(),
                                                job.max_connections
                                            ))
                                            .monospace()
                                            .size(11.5),
                                        ),
                                    );

                                    if ui.small_button("Inspect").clicked() {
                                        self.selected_job_id = Some(job.id.clone());
                                        self.inspected_job_id = Some(job.id.clone());
                                        self.refresh_url_input = job.url.clone();
                                        self.task_limit_kbps = job.speed_limit_bps / 1024;
                                    }
                                });
                            });
                        ui.add_space(3.0);
                    }
                });
            });

        // 4. ACTIVE DOWNLOAD TABBED MODAL (`Download Status`, `Speed Limiter`, `Completion & Link Refresh`)
        if let Some(insp_id) = self.inspected_job_id.clone() {
            if let Some(job) = snapshots.iter().find(|j| j.id == insp_id) {
                let mut open = true;
                egui::Window::new(format!("Download Status — {}", job.filename))
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(true)
                    .default_size(Vec2::new(680.0, 440.0))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.inspector_tab,
                                InspectorTab::DownloadStatus,
                                "Download Status",
                            );
                            ui.selectable_value(
                                &mut self.inspector_tab,
                                InspectorTab::SpeedLimiter,
                                "Speed Limiter",
                            );
                            ui.selectable_value(
                                &mut self.inspector_tab,
                                InspectorTab::CompletionAndRefresh,
                                "Completion & Link Refresh",
                            );
                        });
                        ui.separator();

                        match self.inspector_tab {
                            InspectorTab::DownloadStatus => {
                                ui.label(
                                    egui::RichText::new(format!("URL: {}", job.final_url))
                                        .monospace()
                                        .size(11.0),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Downloaded: {} / {}   |   Transfer Rate: {}/s   |   Status: {}",
                                        format_bytes(job.downloaded_bytes),
                                        job.total_size.map(format_bytes).unwrap_or_else(|| "Unknown".into()),
                                        format_bytes(job.speed_bps),
                                        job.status.as_str()
                                    ))
                                    .size(12.0),
                                );
                                ui.add_space(6.0);

                                // Visual Segmented Progress Bar
                                render_segmented_progress_bar(ui, job);
                                ui.add_space(8.0);

                                ui.label(
                                    egui::RichText::new("Live Per-Connection Byte Ranges:")
                                        .strong()
                                        .size(12.0),
                                );
                                egui::ScrollArea::vertical()
                                    .max_height(190.0)
                                    .show(ui, |ui| {
                                        for seg in &job.segments {
                                            let span = (seg.end.saturating_sub(seg.start) + 1).max(1);
                                            let done = seg.current.saturating_sub(seg.start).min(span);
                                            let pct = (done as f64 / span as f64) * 100.0;
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "Conn #{:02} | Bytes {:>10} - {:<10} | {:>6.1}% | {:>9}/s | {:?}",
                                                    seg.id,
                                                    seg.start,
                                                    seg.end,
                                                    pct,
                                                    format_bytes(seg.speed_bps),
                                                    seg.state
                                                ))
                                                .monospace()
                                                .size(11.0),
                                            );
                                        }
                                    });
                            }
                            InspectorTab::SpeedLimiter => {
                                ui.label("Per-Task Bandwidth Limit (KB/s, 0 = Unlimited):");
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut self.task_limit_kbps)
                                            .speed(16.0)
                                            .range(0..=1_000_000),
                                    );
                                    if ui.button("Apply Task Speed Limit").clicked() {
                                        let _ = self
                                            .manager
                                            .set_task_speed_limit(&job.id, self.task_limit_kbps * 1024);
                                    }
                                });
                                ui.separator();
                                ui.label("Global Bandwidth Limit (KB/s, 0 = Unlimited):");
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut self.global_limit_kbps)
                                            .speed(32.0)
                                            .range(0..=1_000_000),
                                    );
                                    if ui.button("Apply Global Speed Limit").clicked() {
                                        self.manager
                                            .set_global_speed_limit(self.global_limit_kbps * 1024);
                                    }
                                });
                            }
                            InspectorTab::CompletionAndRefresh => {
                                ui.label(
                                    egui::RichText::new(
                                        "Refresh Expired URL & Session Headers (Preserves Downloaded Byte Ranges)",
                                    )
                                    .strong(),
                                );
                                ui.label("New URL:");
                                ui.text_edit_singleline(&mut self.refresh_url_input);
                                ui.label("Updated Cookie Header (optional):");
                                ui.text_edit_singleline(&mut self.refresh_cookie_input);
                                ui.label("Updated Referer Header (optional):");
                                ui.text_edit_singleline(&mut self.refresh_referer_input);
                                if ui.button("Refresh Link & Resume").clicked()
                                    && !self.refresh_url_input.trim().is_empty()
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
                            }
                        }
                    });
                if !open {
                    self.inspected_job_id = None;
                }
            }
        }

        // 5. ADD URL MODAL
        if self.show_add_modal {
            egui::Window::new("Add New Download")
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(460.0, 240.0))
                .show(ctx, |ui| {
                    ui.label("Target URL:");
                    ui.text_edit_singleline(&mut self.add_url);
                    ui.label("Custom Filename (optional):");
                    ui.text_edit_singleline(&mut self.add_filename);
                    ui.horizontal(|ui| {
                        ui.label("Max Connections (1-32):");
                        ui.add(egui::DragValue::new(&mut self.add_connections).range(1..=32));
                    });
                    ui.label("Cookie Header (optional):");
                    ui.text_edit_singleline(&mut self.add_cookie);
                    ui.label("Expected SHA-256 (optional):");
                    ui.text_edit_singleline(&mut self.add_sha256);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Download Now").clicked() && !self.add_url.trim().is_empty() {
                            self.spawn_add_download(true);
                            self.show_add_modal = false;
                        }
                        if ui.button("Queue Later").clicked() && !self.add_url.trim().is_empty() {
                            self.spawn_add_download(false);
                            self.show_add_modal = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_add_modal = false;
                        }
                    });
                });
        }

        // 6. BATCH WILDCARD MODAL
        if self.show_batch_modal {
            egui::Window::new("Batch Wildcard URL Expander")
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(460.0, 140.0))
                .show(ctx, |ui| {
                    ui.label("Wildcard Pattern (e.g. https://site.com/file_[01-20].zip):");
                    ui.text_edit_singleline(&mut self.batch_pattern);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Expand & Start Batch").clicked()
                            && !self.batch_pattern.trim().is_empty()
                        {
                            let urls = expand_wildcard_url(self.batch_pattern.trim(), None);
                            let mgr = self.manager.clone();
                            tokio::spawn(async move {
                                for u in urls {
                                    let req = DownloadRequest {
                                        url: u,
                                        filename: None,
                                        save_dir: None,
                                        headers: HashMap::new(),
                                        max_segments: Some(8),
                                        speed_limit_bps: None,
                                        queue_id: None,
                                        expected_sha256: None,
                                    };
                                    let _ = mgr.add_download(req, true).await;
                                }
                            });
                            self.show_batch_modal = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_batch_modal = false;
                        }
                    });
                });
        }

        // 7. QUEUE SCHEDULER MODAL
        if self.show_scheduler_modal {
            egui::Window::new("Queue Manager & Cron Scheduler")
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(420.0, 220.0))
                .show(ctx, |ui| {
                    ui.label("Queue ID:");
                    ui.text_edit_singleline(&mut self.sched_queue_id);
                    ui.label("Queue Name:");
                    ui.text_edit_singleline(&mut self.sched_queue_name);
                    ui.horizontal(|ui| {
                        ui.label("Max Concurrent Tasks:");
                        ui.add(egui::DragValue::new(&mut self.sched_max_concurrent).range(1..=32));
                    });
                    ui.label("Start Window UTC HH:MM (optional):");
                    ui.text_edit_singleline(&mut self.sched_start_cron);
                    ui.label("Stop Window UTC HH:MM (optional):");
                    ui.text_edit_singleline(&mut self.sched_stop_cron);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save Queue Schedule").clicked() {
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
                        if ui.button("Cancel").clicked() {
                            self.show_scheduler_modal = false;
                        }
                    });
                });
        }
    }
}

impl OxideApp {
    fn spawn_add_download(&mut self, start_now: bool) {
        let mut headers = HashMap::new();
        if !self.add_cookie.trim().is_empty() {
            headers.insert("Cookie".to_string(), self.add_cookie.trim().to_string());
        }
        let req = DownloadRequest {
            url: self.add_url.trim().to_string(),
            filename: if self.add_filename.trim().is_empty() {
                None
            } else {
                Some(self.add_filename.trim().to_string())
            },
            save_dir: None,
            headers,
            max_segments: Some(self.add_connections),
            speed_limit_bps: None,
            queue_id: None,
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
        self.add_sha256.clear();
    }
}

fn sidebar_item(
    ui: &mut egui::Ui,
    current: &mut SidebarFilter,
    target: SidebarFilter,
    label: &str,
    count: usize,
) {
    let selected = *current == target;
    let fill = if selected {
        Color32::from_rgb(30, 58, 138)
    } else {
        Color32::TRANSPARENT
    };
    let text_col = if selected {
        Color32::from_rgb(147, 197, 253)
    } else {
        Color32::from_rgb(203, 213, 225)
    };

    let btn = egui::Button::new(
        egui::RichText::new(format!("{} ({})", label, count))
            .color(text_col)
            .size(12.0),
    )
    .fill(fill);
    if ui.add_sized([ui.available_width(), 24.0], btn).clicked() {
        *current = target;
    }
}

fn render_segmented_progress_bar(ui: &mut egui::Ui, job: &DownloadJobSnapshot) {
    let total = job.total_size.unwrap_or(1).max(1);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, Rounding::same(4.0), Color32::from_rgb(15, 23, 42));

    for (i, seg) in job.segments.iter().enumerate() {
        let start_x = rect.min.x + (seg.start as f32 / total as f32) * rect.width();
        let cur_x = rect.min.x + (seg.current as f32 / total as f32) * rect.width();
        let end_x = rect.min.x + (seg.end as f32 / total as f32) * rect.width();

        let dl_w = (cur_x - start_x).max(1.0);
        let dl_rect =
            Rect::from_min_size(Pos2::new(start_x, rect.min.y), Vec2::new(dl_w, rect.height()));
        let col = if seg.completed {
            Color32::from_rgb(16, 185, 129)
        } else if i % 2 == 0 {
            Color32::from_rgb(59, 130, 246)
        } else {
            Color32::from_rgb(6, 182, 212)
        };
        ui.painter().rect_filled(dl_rect, Rounding::ZERO, col);
        ui.painter().line_segment(
            [Pos2::new(end_x, rect.min.y), Pos2::new(end_x, rect.max.y)],
            Stroke::new(1.0_f32, Color32::from_rgb(11, 13, 19)),
        );
    }
}

fn status_color(status: DownloadStatus) -> Color32 {
    match status {
        DownloadStatus::Downloading => Color32::from_rgb(96, 165, 250),
        DownloadStatus::Completed => Color32::from_rgb(52, 211, 153),
        DownloadStatus::Paused => Color32::from_rgb(251, 191, 36),
        DownloadStatus::AwaitingRefresh | DownloadStatus::Failed => Color32::from_rgb(244, 63, 94),
        _ => Color32::from_rgb(148, 163, 184),
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
