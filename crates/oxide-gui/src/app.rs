use eframe::egui::{self, Color32, Margin, Pos2, Rect, Rounding, Stroke, Vec2};
use oxide_core::{Category, DownloadStatus, SegmentRange};
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct UiDownloadItem {
    pub id: String,
    pub filename: String,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub speed_bps: u64,
    pub status: DownloadStatus,
    pub category: Category,
    pub segments: Vec<SegmentRange>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiFilter {
    All,
    Active,
    Completed,
    Videos,
    Programs,
    Compressed,
}

pub struct OxideApp {
    pub filter: UiFilter,
    pub items: Arc<RwLock<Vec<UiDownloadItem>>>,
    pub selected_item: Option<String>,
    pub new_url_input: String,
    pub show_add_modal: bool,
}

impl OxideApp {
    pub fn new(items: Arc<RwLock<Vec<UiDownloadItem>>>) -> Self {
        Self {
            filter: UiFilter::All,
            items,
            selected_item: None,
            new_url_input: String::new(),
            show_add_modal: false,
        }
    }
}

impl eframe::App for OxideApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request repaint every 100ms when active downloads exist, otherwise stay dormant at 0% CPU
        let has_active = {
            let items = self.items.read();
            items.iter().any(|i| i.status == DownloadStatus::Downloading)
        };
        if has_active {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // Apply sleek dark theme
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(Color32::from_rgb(230, 235, 240));
        visuals.panel_fill = Color32::from_rgb(18, 20, 24);
        visuals.window_fill = Color32::from_rgb(24, 26, 32);
        ctx.set_visuals(visuals);

        // TOP HEADER BAR: Filter Pills & Action Buttons
        egui::TopBottomPanel::top("top_panel")
            .frame(egui::Frame::none().fill(Color32::from_rgb(22, 24, 29)).inner_margin(Margin::same(12.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("OXIDE").strong().color(Color32::from_rgb(0, 210, 255)).size(16.0));
                    ui.add_space(16.0);

                    // Filter Pills
                    filter_button(ui, &mut self.filter, UiFilter::All, "All");
                    filter_button(ui, &mut self.filter, UiFilter::Active, "Active");
                    filter_button(ui, &mut self.filter, UiFilter::Completed, "Completed");
                    filter_button(ui, &mut self.filter, UiFilter::Videos, "Videos");
                    filter_button(ui, &mut self.filter, UiFilter::Programs, "Programs");
                    filter_button(ui, &mut self.filter, UiFilter::Compressed, "Archives");

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new("+ Add URL").strong().color(Color32::BLACK)).clicked() {
                            self.show_add_modal = true;
                        }
                    });
                });
            });

        // MAIN CONTENT: Virtualized Download Rows
        egui::CentralPanel::default().show(ctx, |ui| {
            let items = self.items.read().clone();
            let filtered_items: Vec<&UiDownloadItem> = items
                .iter()
                .filter(|item| match self.filter {
                    UiFilter::All => true,
                    UiFilter::Active => item.status == DownloadStatus::Downloading || item.status == DownloadStatus::Connecting,
                    UiFilter::Completed => item.status == DownloadStatus::Completed,
                    UiFilter::Videos => item.category == Category::Videos,
                    UiFilter::Programs => item.category == Category::Programs,
                    UiFilter::Compressed => item.category == Category::Compressed,
                })
                .collect();

            if filtered_items.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.label(egui::RichText::new("No downloads in this view").color(Color32::from_rgb(120, 130, 140)).size(15.0));
                    ui.label(egui::RichText::new("Add a URL or trigger a download from your browser").color(Color32::from_rgb(90, 100, 110)).size(12.0));
                });
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for item in filtered_items {
                        render_download_row(ui, item, &mut self.selected_item);
                        ui.add_space(4.0);
                    }
                });
            }

            // COLLAPSIBLE BOTTOM INSPECTOR: Multi-Segment Live Work-Stealing Map
            if let Some(ref sel_id) = self.selected_item {
                if let Some(item) = items.iter().find(|i| &i.id == sel_id) {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("Segment Inspector - {}", item.filename)).strong().size(13.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("[X] Close").clicked() {
                                self.selected_item = None;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    render_segment_stealing_map(ui, item);
                }
            }
        });

        // MODAL: Quick Add Download
        if self.show_add_modal {
            egui::Window::new("Add Download")
                .collapsible(false)
                .resizable(false)
                .fixed_size(Vec2::new(420.0, 140.0))
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Enter target download URL:");
                    ui.add(egui::TextEdit::singleline(&mut self.new_url_input).hint_text("https://...").desired_width(f32::INFINITY));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Start Download").color(Color32::from_rgb(0, 210, 255))).clicked() && !self.new_url_input.trim().is_empty() {
                            self.new_url_input.clear();
                            self.show_add_modal = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_add_modal = false;
                        }
                    });
                });
        }
    }
}

fn filter_button(ui: &mut egui::Ui, current: &mut UiFilter, target: UiFilter, label: &str) {
    let is_selected = *current == target;
    let text_color = if is_selected { Color32::from_rgb(0, 210, 255) } else { Color32::from_rgb(160, 170, 180) };
    let bg_color = if is_selected { Color32::from_rgb(32, 40, 52) } else { Color32::TRANSPARENT };

    let btn = egui::Button::new(egui::RichText::new(label).color(text_color).size(13.0)).fill(bg_color);
    if ui.add(btn).clicked() {
        *current = target;
    }
}

fn render_download_row(ui: &mut egui::Ui, item: &UiDownloadItem, selected_id: &mut Option<String>) {
    let is_selected = selected_id.as_ref().map(|s| s == &item.id).unwrap_or(false);
    let bg_color = if is_selected {
        Color32::from_rgb(28, 34, 46)
    } else {
        Color32::from_rgb(22, 25, 31)
    };

    let total = item.total_size.unwrap_or(1);
    let progress_ratio = (item.downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0);

    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), egui::Sense::click());

    if response.clicked() {
        if is_selected {
            *selected_id = None;
        } else {
            *selected_id = Some(item.id.clone());
        }
    }

    // Background card
    ui.painter().rect_filled(rect, Rounding::same(6.0), bg_color);

    // Subtle progress fill behind the row
    if progress_ratio > 0.0 && item.status == DownloadStatus::Downloading {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width() * progress_ratio, rect.height()));
        ui.painter().rect_filled(fill_rect, Rounding::same(6.0), Color32::from_rgba_unmultiplied(0, 210, 255, 18));
    }

    // Micro segment bar (4px line at bottom of row)
    if !item.segments.is_empty() && total > 1 {
        let bar_rect = Rect::from_min_size(Pos2::new(rect.min.x + 8.0, rect.max.y - 6.0), Vec2::new(rect.width() - 16.0, 3.0));
        ui.painter().rect_filled(bar_rect, Rounding::same(1.5), Color32::from_rgb(35, 40, 50));

        for seg in &item.segments {
            let seg_start_ratio = (seg.start as f32 / total as f32).clamp(0.0, 1.0);
            let seg_cur_ratio = (seg.current as f32 / total as f32).clamp(0.0, 1.0);
            let seg_w = ((seg_cur_ratio - seg_start_ratio) * (rect.width() - 16.0)).max(1.0);

            let seg_rect = Rect::from_min_size(Pos2::new(bar_rect.min.x + seg_start_ratio * bar_rect.width(), bar_rect.min.y), Vec2::new(seg_w, 3.0));
            ui.painter().rect_filled(seg_rect, Rounding::same(1.5), Color32::from_rgb(0, 210, 255));
        }
    }

    // Typography
    let name_rect = Rect::from_min_size(Pos2::new(rect.min.x + 12.0, rect.min.y + 10.0), Vec2::new(rect.width() - 200.0, 18.0));
    ui.painter().text(
        name_rect.min,
        egui::Align2::LEFT_TOP,
        &item.filename,
        egui::FontId::proportional(14.0),
        Color32::from_rgb(240, 245, 250),
    );

    let status_str = match item.status {
        DownloadStatus::Downloading => {
            let speed_kb = item.speed_bps as f64 / 1024.0;
            if speed_kb >= 1024.0 {
                format!("{:.1}% — {:.2} MB/s", progress_ratio * 100.0, speed_kb / 1024.0)
            } else {
                format!("{:.1}% — {:.0} KB/s", progress_ratio * 100.0, speed_kb)
            }
        }
        DownloadStatus::Completed => "Completed".into(),
        DownloadStatus::Assembling => "Verifying...".into(),
        DownloadStatus::Paused => "Paused".into(),
        DownloadStatus::Connecting => "Connecting...".into(),
        DownloadStatus::Queued => "Queued".into(),
        DownloadStatus::Failed => "Failed".into(),
        DownloadStatus::AwaitingRefresh => "Link Expired (Refresh)".into(),
    };

    let meta_rect = Rect::from_min_size(Pos2::new(rect.max.x - 12.0, rect.min.y + 10.0), Vec2::new(180.0, 18.0));
    ui.painter().text(
        meta_rect.min,
        egui::Align2::RIGHT_TOP,
        status_str,
        egui::FontId::proportional(13.0),
        if item.status == DownloadStatus::Completed {
            Color32::from_rgb(76, 217, 100)
        } else {
            Color32::from_rgb(0, 210, 255)
        },
    );
}

fn render_segment_stealing_map(ui: &mut egui::Ui, item: &UiDownloadItem) {
    let total = item.total_size.unwrap_or(1);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), egui::Sense::hover());

    // Base background
    ui.painter().rect_filled(rect, Rounding::same(4.0), Color32::from_rgb(30, 34, 44));

    if item.segments.is_empty() || total == 1 {
        return;
    }

    for (i, seg) in item.segments.iter().enumerate() {
        let start_x = rect.min.x + (seg.start as f32 / total as f32) * rect.width();
        let cur_x = rect.min.x + (seg.current as f32 / total as f32) * rect.width();
        let end_x = rect.min.x + (seg.end as f32 / total as f32) * rect.width();

        // Downloaded part in segment
        let dl_w = (cur_x - start_x).max(1.0);
        let dl_rect = Rect::from_min_size(Pos2::new(start_x, rect.min.y), Vec2::new(dl_w, rect.height()));
        let hue_color = if i % 2 == 0 {
            Color32::from_rgb(0, 210, 255)
        } else {
            Color32::from_rgb(80, 230, 255)
        };
        ui.painter().rect_filled(dl_rect, Rounding::ZERO, hue_color);

        // Segment divider line
        ui.painter().line_segment([Pos2::new(end_x, rect.min.y), Pos2::new(end_x, rect.max.y)], Stroke::new(1.0_f32, Color32::from_rgb(15, 18, 22)));
    }

    ui.add_space(4.0);
    ui.label(egui::RichText::new(format!("Active dynamic connections: {}", item.segments.len())).color(Color32::from_rgb(140, 150, 160)).size(11.0));
}
