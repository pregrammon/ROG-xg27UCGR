//! 系统托盘图标（红色 "R"）。右键菜单：显示主界面、窗口透明 /
//! 高斯模糊（互斥）、开机自启动、退出；左键单击也会打开主界面。
//! 事件通过 mpsc 通道转发到 UI 线程。

use std::sync::mpsc;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    ShowMain,
    ToggleTransparent,
    ToggleBlur,
    ToggleAutoStart,
    Exit,
}

pub struct Tray {
    tray: TrayIcon,
    event_rx: mpsc::Receiver<TrayEvent>,
}

/// 生成托盘/任务栏图标：32x32 的红色 "R"。返回值同时供 egui 视口图标使用。
pub fn icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let font_bytes = std::fs::read("C:\\Windows\\Fonts\\arialbd.ttf")
        .or_else(|_| std::fs::read("C:\\Windows\\Fonts\\arial.ttf"))
        .or_else(|_| std::fs::read("C:\\Windows\\Fonts\\simhei.ttf"))
        .ok()?;
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default()).ok()?;
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
    Some((rgba, size as u32, size as u32))
}

fn make_icon() -> Option<Icon> {
    let (rgba, width, height) = icon_rgba()?;
    Icon::from_rgba(rgba, width, height).ok()
}

fn state_suffix(on: bool) -> &'static str {
    if on { " ✓" } else { "" }
}

fn build_menu(transparent: bool, blur: bool, auto_start: bool) -> Menu {
    let menu = Menu::new();
    menu.append(&MenuItem::with_id(
        "show_main",
        "显示主界面",
        true,
        None,
    ));
    menu.append(&PredefinedMenuItem::separator());
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
        let icon = make_icon()?;
        let menu = build_menu(transparent, blur, auto_start);
        let tray = TrayIconBuilder::new()
            .with_tooltip("ROG 屏幕控制")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .ok()?;

        let (tx, rx) = mpsc::channel::<TrayEvent>();

        // 菜单事件。
        let menu_tx = tx.clone();
        let menu_ctx = ctx.clone();
        std::thread::spawn(move || {
            let receiver = MenuEvent::receiver();
            while let Ok(event) = receiver.recv() {
                let id = event.id.0;
                let msg = match id.as_str() {
                    "show_main" => Some(TrayEvent::ShowMain),
                    "transparent" => Some(TrayEvent::ToggleTransparent),
                    "blur" => Some(TrayEvent::ToggleBlur),
                    "autostart" => Some(TrayEvent::ToggleAutoStart),
                    "exit" => Some(TrayEvent::Exit),
                    _ => None,
                };
                if let Some(message) = msg {
                    if menu_tx.send(message).is_err() {
                        break;
                    }
                    // 托盘操作时窗口处于空闲状态，立刻唤醒 eframe。
                    menu_ctx.request_repaint();
                }
            }
        });

        // 左键单击图标 -> 打开主界面。
        let click_tx = tx;
        let click_ctx = ctx;
        std::thread::spawn(move || {
            let receiver = TrayIconEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    if click_tx.send(TrayEvent::ShowMain).is_err() {
                        break;
                    }
                    click_ctx.request_repaint();
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
