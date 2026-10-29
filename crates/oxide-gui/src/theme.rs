use eframe::egui::{
    self, Color32, FontFamily, FontId, Margin, Pos2, Rect, Rounding, Sense, Stroke, TextStyle, Vec2,
};
use oxide_core::{Category, DownloadStatus, SegmentState};

/// Modern, high-contrast colorful palette for Oxide Download Manager
pub struct Palette;

impl Palette {
    // Deep Surface Hierarchy
    pub const BG_CANVAS: Color32 = Color32::from_rgb(9, 12, 18); // #090C12
    pub const BG_SIDEBAR: Color32 = Color32::from_rgb(13, 17, 26); // #0D111A
    pub const BG_TOOLBAR: Color32 = Color32::from_rgb(16, 21, 32); // #101520
    pub const BG_HEADER: Color32 = Color32::from_rgb(20, 26, 40); // #141A28
    pub const BG_CARD: Color32 = Color32::from_rgb(18, 24, 36); // #121824
    pub const BG_CARD_HOVER: Color32 = Color32::from_rgb(25, 33, 50); // #192132
    pub const BG_CARD_SELECTED: Color32 = Color32::from_rgb(26, 39, 68); // #1A2744
    pub const BG_INPUT: Color32 = Color32::from_rgb(11, 15, 23); // #0B0F17

    // Borders & Dividers
    pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(32, 41, 60); // #20293C
    pub const BORDER_STRONG: Color32 = Color32::from_rgb(48, 62, 90); // #303E5A
    pub const BORDER_FOCUS: Color32 = Color32::from_rgb(59, 130, 246); // #3B82F6

    // Text Hierarchy
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(248, 250, 252); // #F8FAFC
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(203, 213, 225); // #CBD5E1
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(138, 153, 173); // #8A99AD
    pub const TEXT_DIM: Color32 = Color32::from_rgb(94, 109, 130); // #5E6D82

    // Vibrant Brand & Semantic Accents
    pub const BLUE: Color32 = Color32::from_rgb(59, 130, 246); // #3B82F6
    pub const BLUE_DARK: Color32 = Color32::from_rgb(29, 78, 216); // #1D4ED8
    pub const CYAN: Color32 = Color32::from_rgb(6, 182, 212); // #06B6D4
    pub const CYAN_BRIGHT: Color32 = Color32::from_rgb(34, 211, 238); // #22D3EE
    pub const INDIGO: Color32 = Color32::from_rgb(99, 102, 241); // #6366F1
    pub const PURPLE: Color32 = Color32::from_rgb(168, 85, 247); // #A855F7
    pub const PINK: Color32 = Color32::from_rgb(236, 72, 153); // #EC4899
    pub const EMERALD: Color32 = Color32::from_rgb(16, 185, 129); // #10B981
    pub const EMERALD_BRIGHT: Color32 = Color32::from_rgb(52, 211, 153); // #34D399
    pub const AMBER: Color32 = Color32::from_rgb(245, 158, 11); // #F59E0B
    pub const ORANGE: Color32 = Color32::from_rgb(249, 115, 22); // #F97316
    pub const ROSE: Color32 = Color32::from_rgb(244, 63, 94); // #F43F5E
    pub const TEAL: Color32 = Color32::from_rgb(20, 184, 166); // #14B8A6
}

/// Configures global `egui::Context` style, typography hierarchy, spacing, and widget visuals.
pub fn apply_modern_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Spacing & Sizing Hierarchy
    style.spacing.item_spacing = Vec2::new(8.0, 7.0);
    style.spacing.window_margin = Margin::same(16.0);
    style.spacing.button_padding = Vec2::new(11.0, 6.0);
    style.spacing.indent = 16.0;
    style.spacing.scroll.bar_width = 7.0;

    // Typography Scale
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(18.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(12.5, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Palette::TEXT_PRIMARY);
    visuals.panel_fill = Palette::BG_CANVAS;
    visuals.window_fill = Palette::BG_SIDEBAR;
    visuals.window_stroke = Stroke::new(1.0_f32, Palette::BORDER_STRONG);
    visuals.window_rounding = Rounding::same(10.0);
    visuals.extreme_bg_color = Palette::BG_INPUT;
    visuals.faint_bg_color = Palette::BG_CARD;

    visuals.widgets.noninteractive.bg_fill = Palette::BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Palette::BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Palette::TEXT_SECONDARY);
    visuals.widgets.noninteractive.rounding = Rounding::same(6.0);

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(24, 32, 48);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Palette::BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Palette::TEXT_PRIMARY);
    visuals.widgets.inactive.rounding = Rounding::same(6.0);

    visuals.widgets.hovered.bg_fill = Palette::BG_CARD_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Palette::BLUE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    visuals.widgets.hovered.rounding = Rounding::same(6.0);

    visuals.widgets.active.bg_fill = Palette::BLUE_DARK;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Palette::CYAN_BRIGHT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    visuals.widgets.active.rounding = Rounding::same(6.0);

    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(59, 130, 246, 70);
    visuals.selection.stroke = Stroke::new(1.0_f32, Palette::CYAN_BRIGHT);

    style.visuals = visuals;
    ctx.set_style(style);
}

/// Returns the vibrant accent color associated with a file Category.
pub fn category_color(cat: Category) -> Color32 {
    match cat {
        Category::Compressed => Palette::PURPLE,
        Category::Documents => Palette::CYAN_BRIGHT,
        Category::Music => Palette::PINK,
        Category::Programs => Palette::INDIGO,
        Category::Videos => Palette::ORANGE,
        Category::Torrents => Palette::EMERALD,
        Category::General => Palette::TEXT_MUTED,
    }
}

/// Returns the semantic color for a `DownloadStatus`.
pub fn status_color(status: DownloadStatus) -> Color32 {
    match status {
        DownloadStatus::Downloading => Palette::CYAN_BRIGHT,
        DownloadStatus::Connecting => Palette::BLUE,
        DownloadStatus::Assembling => Palette::PURPLE,
        DownloadStatus::Completed => Palette::EMERALD_BRIGHT,
        DownloadStatus::Paused => Palette::AMBER,
        DownloadStatus::Queued => Palette::INDIGO,
        DownloadStatus::AwaitingRefresh => Palette::ORANGE,
        DownloadStatus::Failed => Palette::ROSE,
    }
}

/// Returns the semantic color for a `SegmentState`.
pub fn segment_state_color(state: SegmentState) -> Color32 {
    match state {
        SegmentState::Downloading => Palette::CYAN_BRIGHT,
        SegmentState::Connecting => Palette::BLUE,
        SegmentState::Completed => Palette::EMERALD,
        SegmentState::Paused => Palette::AMBER,
        SegmentState::Failed => Palette::ROSE,
        SegmentState::Idle => Palette::TEXT_DIM,
    }
}

/// Renders a colorful pill button for the top IDM action ribbon.
pub fn action_ribbon_button(
    ui: &mut egui::Ui,
    label: &str,
    accent: Color32,
    filled: bool,
) -> bool {
    let text_col = if filled {
        Color32::WHITE
    } else {
        accent
    };
    let bg_fill = if filled {
        accent
    } else {
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 26)
    };
    let border_col = if filled {
        accent
    } else {
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 95)
    };

    let btn = egui::Button::new(
        egui::RichText::new(label)
            .size(12.5)
            .strong()
            .color(text_col),
    )
    .fill(bg_fill)
    .stroke(Stroke::new(1.0_f32, border_col))
    .rounding(Rounding::same(6.0));

    ui.add(btn).clicked()
}

/// Renders a compact colorful status or category badge pill.
pub fn draw_badge_pill(ui: &mut egui::Ui, text: &str, accent: Color32) {
    let bg = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 32);
    let border = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 90);

    egui::Frame::none()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, border))
        .rounding(Rounding::same(4.0))
        .inner_margin(Margin::symmetric(6.0, 2.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(10.5)
                    .strong()
                    .color(accent),
            );
        });
}

/// Custom-paints a sleek multi-color progress bar with a subtle glow/shimmer.
pub fn draw_progress_bar(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    ratio: f32,
    fill_color: Color32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    ui.painter().rect_filled(
        rect,
        Rounding::same(height * 0.5),
        Color32::from_rgb(11, 15, 24),
    );
    ui.painter().rect_stroke(
        rect,
        Rounding::same(height * 0.5),
        Stroke::new(1.0_f32, Palette::BORDER_SUBTLE),
    );

    let clamped = ratio.clamp(0.0, 1.0);
    if clamped > 0.0 {
        let fill_w = (rect.width() * clamped).max(height);
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        ui.painter()
            .rect_filled(fill_rect, Rounding::same(height * 0.5), fill_color);
    }
}

/// Draws a live throughput history sparkline graph in the sidebar or header.
pub fn draw_speed_sparkline(ui: &mut egui::Ui, history: &[u64], width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, Rounding::same(6.0), Palette::BG_INPUT);
    ui.painter().rect_stroke(
        rect,
        Rounding::same(6.0),
        Stroke::new(1.0_f32, Palette::BORDER_SUBTLE),
    );

    if history.len() < 2 {
        return;
    }

    let max_val = (*history.iter().max().unwrap_or(&1)).max(1024) as f32;
    let step_x = rect.width() / (history.len().saturating_sub(1).max(1) as f32);

    let mut points = Vec::with_capacity(history.len());
    for (i, &val) in history.iter().enumerate() {
        let x = rect.min.x + (i as f32) * step_x;
        let norm = (val as f32 / max_val).clamp(0.0, 1.0);
        let y = rect.max.y - 3.0 - norm * (rect.height() - 6.0);
        points.push(Pos2::new(x, y));
    }

    for pair in points.windows(2) {
        ui.painter().line_segment(
            [pair[0], pair[1]],
            Stroke::new(1.6_f32, Palette::CYAN_BRIGHT),
        );
    }
}
