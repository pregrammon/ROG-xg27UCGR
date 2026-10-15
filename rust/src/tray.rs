//! 系统托盘图标（红色 "R"），右键菜单与 C# 版一致：
//! 窗口透明 / 高斯模糊（两者互斥）、开机自启动、退出。
//! 菜单点击通过 mpsc 通道转发到 UI 线程。

use std::sync::mpsc;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    ToggleTransparent,
    ToggleBlur,
    ToggleAutoStart,
    Exit,
}

pub struct Tray {
    tray: TrayIcon,
    event_rx: mpsc::Receiver<TrayEvent>,
}

fn make_r_icon() -> Result<Icon, String> {
    let font_bytes = std::fs::read("C:\\Windows\\Fonts\\arialbd.ttf")
        .or_else(|_| std::fs::read("C:\\Windows\\Fonts\\arial.ttf"))
        .or_else(|_| std::fs::read("C:\\Windows\\Fonts\\simhei.ttf"))
        .map_err(|e| format!("no font: {e}"))?;
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| format!("font parse: {e}"))?;
    let (metrics, pixels) = font.rasterize('R', 24.0);
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];
    let x0 = (size as i32 - metrics.width as i32) / 2 - metrics.xmin;
    let y0 = (size as i32 - metrics.height as i32) / 2 - metrics.ymin;
    for (i, alpha) in pixels.iter().enumerate() {
        let x = i % metrics.width;
        let y = i / metrics.width;
        let px = x as i32 + x0;
        let py = y as i32 + y0;
        if px >= 0 && px < size as i32 && py >= 0 && py < size as i32 && *alpha > 0 {
            let idx = (py as usize * size + px as usize) * 4;
            // 与控件使用同一个强调红色（见 app::ACCENT_RGB）。
            let (r, g, b) = crate::app::ACCENT_RGB;
            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = *alpha;
        }
    }
    Icon::from_rgba(rgba, size as u32, size as u32).map_err(|e| format!("icon: {e}"))
}

fn state_suffix(on: bool) -> &'static str {
    if on { " ✓" } else { "" }
}

fn build_menu(transparent: bool, blur: bool, auto_start: bool) -> Menu {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id(
        "transparent",
        format!("窗口透明{}", state_suffix(transparent)),
        true,
        None,
    ));
    menu.append(&MenuItem::with_id(
        "blur",
        format!("高斯模糊{}", state_suffix(blur)),
        true,
        None,
    ));
    menu.append(&MenuItem::with_id(
        "autostart",
        format!("开机自启动{}", state_suffix(auto_start)),
        true,
        None,
    ));
    menu.append(&PredefinedMenuItem::separator());
    menu.append(&MenuItem::with_id("exit", "退出", true, None));
    menu
}

impl Tray {
    pub fn new(
        transparent: bool,
        blur: bool,
        auto_start: bool,
        ctx: egui::Context,
    ) -> Option<Tray> {
        let icon = make_r_icon().ok()?;
        let menu = build_menu(transparent, blur, auto_start);
        let tray = TrayIconBuilder::new()
            .with_tooltip("ROG 屏幕控制")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .ok()?;

        let (tx, rx) = mpsc::channel::<TrayEvent>();
        std::thread::spawn(move || {
            let receiver = MenuEvent::receiver();
            while let Ok(event) = receiver.recv() {
                let id = event.id.0;
                let msg = match id.as_str() {
                    "transparent" => Some(TrayEvent::ToggleTransparent),
                    "blur" => Some(TrayEvent::ToggleBlur),
                    "autostart" => Some(TrayEvent::ToggleAutoStart),
                    "exit" => Some(TrayEvent::Exit),
                    _ => None,
                };
                if let Some(m) = msg {
                    if tx.send(m).is_err() {
                        break;
                    }
                    // 用户操作托盘菜单时窗口处于空闲状态（没有鼠标/键盘输入）；
                    // 这里立刻唤醒 eframe，让开关马上生效、
                    // 勾选标记也立即刷新，
                    // 而不必先把鼠标移到窗口上。
                    ctx.request_repaint();
                }
            }
        });

        Some(Tray { tray, event_rx: rx })
    }

    /// UI 每帧调用的非阻塞轮询。
    pub fn poll(&self) -> Option<TrayEvent> {
        self.event_rx.try_recv().ok()
    }

    /// 重建菜单，使勾选标记反映当前状态。
    pub fn set_states(&mut self, transparent: bool, blur: bool, auto_start: bool) {
        let menu = build_menu(transparent, blur, auto_start);
        self.tray.set_menu(Some(Box::new(menu)));
    }
}
