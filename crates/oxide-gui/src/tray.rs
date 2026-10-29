use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

pub enum TrayAction {
    OpenManager,
    AddDownload,
    PauseAll,
    ResumeAll,
    Quit,
}

pub struct TrayManager {
    _tray_icon: TrayIcon,
    menu_open_id: String,
    menu_add_id: String,
    menu_pause_id: String,
    menu_resume_id: String,
    menu_quit_id: String,
}

impl TrayManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();

        let open_item = MenuItem::new("Open Oxide", true, None);
        let add_item = MenuItem::new("Add Download URL...", true, None);
        let pause_item = MenuItem::new("Pause All", true, None);
        let resume_item = MenuItem::new("Resume All", true, None);
        let quit_item = MenuItem::new("Quit Oxide", true, None);

        let menu_open_id = open_item.id().0.clone();
        let menu_add_id = add_item.id().0.clone();
        let menu_pause_id = pause_item.id().0.clone();
        let menu_resume_id = resume_item.id().0.clone();
        let menu_quit_id = quit_item.id().0.clone();

        menu.append(&open_item)?;
        menu.append(&add_item)?;
        menu.append(&pause_item)?;
        menu.append(&resume_item)?;
        menu.append(&quit_item)?;

        // Minimal 16x16 icon in RGBA format (Oxide cyan/dark theme dot)
        let icon_rgba = create_default_tray_icon();
        let icon = Icon::from_rgba(icon_rgba, 16, 16)?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Oxide - Download Manager")
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray_icon: tray_icon,
            menu_open_id,
            menu_add_id,
            menu_pause_id,
            menu_resume_id,
            menu_quit_id,
        })
    }

    /// Check for clicked tray menu items
    pub fn check_events(&self) -> Option<TrayAction> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id.0 == self.menu_open_id {
                return Some(TrayAction::OpenManager);
            } else if event.id.0 == self.menu_add_id {
                return Some(TrayAction::AddDownload);
            } else if event.id.0 == self.menu_pause_id {
                return Some(TrayAction::PauseAll);
            } else if event.id.0 == self.menu_resume_id {
                return Some(TrayAction::ResumeAll);
            } else if event.id.0 == self.menu_quit_id {
                return Some(TrayAction::Quit);
            }
        }
        None
    }
}

fn create_default_tray_icon() -> Vec<u8> {
    let mut rgba = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            // Draw a rounded circle
            let dx = (x as f32 - 7.5).abs();
            let dy = (y as f32 - 7.5).abs();
            let dist = (dx * dx + dy * dy).sqrt();

            if dist <= 6.5 {
                // Oxide cyan/teal dot: #00d2ff
                rgba.extend_from_slice(&[0, 210, 255, 255]);
            } else {
                // Transparent
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    rgba
}
