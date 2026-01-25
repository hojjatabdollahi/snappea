//! System tray icon for recording indicator using StatusNotifierItem (ksni)

use crossbeam_channel::Sender;
use ksni::{blocking::TrayMethods, menu::StandardItem, Icon, MenuItem, Tray};

/// Actions that can be triggered from the tray menu
#[derive(Debug, Clone)]
pub enum TrayAction {
    /// Stop the current recording
    StopRecording,
    /// Toggle toolbar visibility
    ToggleToolbar,
}

/// The tray icon state for recording
pub struct SnappeaTray {
    toolbar_visible: bool,
    tx: Sender<TrayAction>,
    icon: Vec<Icon>,
}

impl SnappeaTray {
    pub fn new(toolbar_visible: bool, tx: Sender<TrayAction>) -> Self {
        Self {
            toolbar_visible,
            tx,
            icon: create_recording_icon(),
        }
    }

    pub fn set_toolbar_visible(&mut self, visible: bool) {
        self.toolbar_visible = visible;
    }
}

/// Create a simple red circle icon for recording indicator
fn create_recording_icon() -> Vec<Icon> {
    // Create icons at multiple sizes for proper DPI scaling
    let sizes = [16, 22, 24, 32, 48, 64];
    let mut icons = Vec::new();

    for size in sizes {
        if let Some(icon) = create_red_circle_icon(size) {
            icons.push(icon);
        }
    }

    icons
}

/// Create a red circle icon at the specified size
fn create_red_circle_icon(size: i32) -> Option<Icon> {
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    let center = size as f32 / 2.0;
    let radius = center - 1.0; // Leave 1px border

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center + 0.5;
            let dy = y as f32 - center + 0.5;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist <= radius {
                // Inside circle - red with slight gradient for depth
                let intensity = 1.0 - (dist / radius) * 0.2;
                let r = (220.0 * intensity) as u8;
                let g = (50.0 * intensity) as u8;
                let b = (50.0 * intensity) as u8;
                // ARGB format (network byte order)
                data.push(255); // A
                data.push(r); // R
                data.push(g); // G
                data.push(b); // B
            } else if dist <= radius + 1.0 {
                // Anti-aliased edge
                let alpha = ((radius + 1.0 - dist) * 255.0) as u8;
                data.push(alpha);
                data.push(200);
                data.push(40);
                data.push(40);
            } else {
                // Outside - transparent
                data.push(0);
                data.push(0);
                data.push(0);
                data.push(0);
            }
        }
    }

    Some(Icon {
        width: size,
        height: size,
        data,
    })
}

impl Tray for SnappeaTray {
    fn id(&self) -> String {
        "dev.hojjat.snappea.recording".to_string()
    }

    fn title(&self) -> String {
        "Snappea Recording".to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icon.clone()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left-click on tray icon stops recording
        log::info!("Tray icon clicked - stopping recording");
        if let Err(e) = self.tx.send(TrayAction::StopRecording) {
            log::error!("Failed to send StopRecording: {}", e);
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Snappea - Recording".to_string(),
            description: "Click to stop recording".to_string(),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let tx_stop = self.tx.clone();
        let tx_toolbar = self.tx.clone();
        let toolbar_visible = self.toolbar_visible;

        vec![
            MenuItem::Standard(StandardItem {
                label: "Stop Recording".to_string(),
                activate: Box::new(move |_| {
                    log::info!("Menu: Stop Recording clicked");
                    if let Err(e) = tx_stop.send(TrayAction::StopRecording) {
                        log::error!("Failed to send StopRecording: {}", e);
                    }
                }),
                ..Default::default()
            }),
            MenuItem::Standard(StandardItem {
                label: if toolbar_visible {
                    "Hide Toolbar"
                } else {
                    "Show Toolbar"
                }
                .to_string(),
                activate: Box::new(move |_| {
                    log::info!("Menu: Toggle Toolbar clicked");
                    if let Err(e) = tx_toolbar.send(TrayAction::ToggleToolbar) {
                        log::error!("Failed to send ToggleToolbar: {}", e);
                    }
                }),
                ..Default::default()
            }),
        ]
    }
}

/// Handle type for the tray
pub type TrayHandle = ksni::blocking::Handle<SnappeaTray>;

/// Create the tray icon and return a handle for controlling it
pub fn create_tray(toolbar_visible: bool, tx: Sender<TrayAction>) -> TrayHandle {
    let tray = SnappeaTray::new(toolbar_visible, tx);
    tray.spawn().expect("Failed to spawn tray icon")
}
