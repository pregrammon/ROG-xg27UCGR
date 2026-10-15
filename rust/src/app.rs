//! 基于 egui 的 RogControl 窗口：透明无边框卡片，包含亮度
//! 滑块（DDC/CI）、4K/1K 双模式开关（VCP 0xFD）、从标题栏
//! 拖动（固定时除外）、8px 边缘/角落缩放、tray 菜单（透明 /
//! blur / 开机自启 / 退出）以及位置和尺寸的持久化。

use std::ffi::c_void;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use raw_window_handle::HasWindowHandle;

use egui::{Align2, Color32, CursorIcon, FontId, Rect, Rounding, Sense, Stroke};
use windows::Win32::Foundation::HWND;

use crate::config::{self, Config};
use crate::ddc::DdcController;
use crate::dual_mode::{self, DualMode};
use crate::tray::{Tray, TrayEvent};
use crate::win32;

/// 控件与 tray 图标共用的强调红色：ROG 红（#F51928）略微
/// 调柔（R 245->246，G 25->54，B 40->67）。修改这一处数值
/// 即可重新调整应用中所有的红色。
pub const ACCENT_RGB: (u8, u8, u8) = (0xF6, 0x36, 0x43);
const RED: Color32 = Color32::from_rgb(ACCENT_RGB.0, ACCENT_RGB.1, ACCENT_RGB.2);
const BORDER: Color32 = Color32::from_rgb(0x28, 0x29, 0x2A); // 窗口边框
const GRAY: Color32 = Color32::from_rgb(0x8A, 0x8B, 0x90);

const MIN_W: f32 = 280.0;
const MIN_H: f32 = 160.0;

const EDGE: f32 = 8.0; // 卡片边缘缩放热区宽度
const CARD_RADIUS: f32 = 12.0;

struct DragState {
    start: win32::Point,
    win_x: i32,
    win_y: i32,
}

struct ResizeState {
    dir: String,
    start: win32::Point,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct BrightnessRetry {
    attempt: u32,
    at: Instant,
}

type DualResult = (DualMode, Result<(), String>);

pub struct RogApp {
    // 窗口 / 样式
    config: Config,
    hwnd: Option<HWND>,
    inited: bool,
    pinned: bool,
    transparent: bool,
    blur: bool,
    auto_start: bool,
    drag: Option<DragState>,
    resize: Option<ResizeState>,
    // 亮度
    brightness: Option<DdcController>,
    brightness_percent: f32,
    brightness_text: String,
    brightness_retry: Option<BrightnessRetry>,
    brightness_apply_at: Option<Instant>,
    status: String,
    status_visible: bool,
    // 双模式
    dual_mode_on: bool, // true = 1K
    dual_mode_known: bool,
    switching_dual: bool,
    dual_rx: mpsc::Receiver<DualResult>,
    dual_tx: mpsc::Sender<DualResult>,
    // tray
    tray: Option<Tray>,
    // 上一次为其构建圆角窗口区域的 (w_px, h_px, radius_px)
    region_key: Option<(i32, i32, i32)>,
    // 上一次据此解析布局的主屏幕分辨率
    last_screen: Option<(i32, i32)>,
    // Windows 报告的原生 DPI（即 egui 每点像素数，未施加我们的缩放）
    native_ppp: f32,
    // 临时调试
    last_debug: Option<Instant>,
}

impl RogApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let config = config::load();
        let auto_start = is_auto_start_enabled();
        let tray = Tray::new(config.transparent, config.blur, auto_start, cc.egui_ctx.clone());
        let (dual_tx, dual_rx) = mpsc::channel::<DualResult>();

        RogApp {
            config,
            hwnd: None,
            inited: false,
            pinned: config.pinned,
            transparent: config.transparent,
            blur: config.blur,
            auto_start,
            drag: None,
            resize: None,
            brightness: None,
            brightness_percent: 50.0,
            brightness_text: "--".into(),
            brightness_retry: None,
            brightness_apply_at: None,
            status: String::new(),
            status_visible: false,
            dual_mode_on: false,
            dual_mode_known: false,
            switching_dual: false,
            dual_rx,
            dual_tx,
            tray,
            region_key: None,
            last_screen: None,
            native_ppp: 1.0,
            last_debug: None,
        }
    }

    // 临时调试：每次状态变化 / 指针移动追加一行（最多 10 次/秒）。
    fn debug_log(&self, msg: &str) {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{base}\\RogControl\\debug.log"))
        {
            let _ = writeln!(f, "{msg}");
        }
    }

    // ---- 一次性窗口设置 ----

    fn init_window(&mut self) {
        let Some(hwnd) = self.hwnd else {
            return;
        };

        // 依据屏幕分辨率的比例值来还原，使控件落在
        // 相同的视觉位置，且其尺寸随当前 4K/1K
        // 分辨率缩放；绝对 x/y/w/h 是旧版配置文件的回退方案。
        let screen = unsafe { win32::get_screen_size() };
        let (x, y, w, h) = match screen {
            Some((sw, sh)) => {
                self.last_screen = Some((sw, sh));
                let (x, y, w, h) = config::resolve_rect(&self.config, sw, sh);
                match unsafe { win32::get_work_area() } {
                    Some(a) => (
                        x.clamp(a.left, (a.right - w).max(a.left)),
                        y.clamp(a.top, (a.bottom - h).max(a.top)),
                        w,
                        h,
                    ),
                    None => (x, y, w, h),
                }
            }
            None => {
                let (w, h) = config::clamp_size(self.config.w, self.config.h);
                (self.config.x, self.config.y, w, h)
            }
        };
        unsafe { win32::resize_window(hwnd, x, y, w, h) };

        // 嵌入 Explorer 桌面层（位于所有应用窗口之下）；
        // 失败时回退为置于最底层 Z 序。
        if !unsafe { win32::attach_to_desktop_layer(hwnd) } {
            unsafe { win32::move_to_bottom(hwnd) };
        }
        self.apply_window_style();

        // 依据当前 Windows 分辨率确定双模式状态。
        let mode = dual_mode::get_current_mode();
        self.dual_mode_on = mode == DualMode::Mode1K;
        self.dual_mode_known = mode != DualMode::Unknown;

        self.init_brightness();
    }

    /// 每当物理尺寸或 DPI 缩放变化时（调整大小、4K/1K 模式切换、
    /// 缩放设置变更）重新应用圆角窗口区域。
    fn update_region(&mut self, ctx: &egui::Context) {
        let Some(hwnd) = self.hwnd else { return };
        let Some(rect) = (unsafe { win32::get_window_rect(hwnd) }) else {
            return;
        };
        let w = rect.width();
        let h = rect.height();
        // 将区域半径略缩小到边框外弧以内。
        // SetWindowRgn 是硬边缘的（无法抗锯齿），因此如果区域的
        // 圆角弧线与描边的外弧重合，就会切掉
        // 被平滑的圆角像素，四个角看起来就会发白。
        // 让硬裁剪刚好避开描边，抗锯齿的弧线就能
        // 保留下来，从而让圆角与直边一致。
        let radius = ((CARD_RADIUS * ctx.pixels_per_point()).round() as i32 - 3).max(1);
        let key = Some((w, h, radius));
        if self.region_key != key {
            unsafe { win32::set_round_region(hwnd, w, h, radius) };
            self.region_key = key;
        }
    }

    /// 在显示模式变化时保持控件的视觉位置不变。ROG
    /// 4K/1K 切换会改变面板分辨率，因此同一个物理矩形
    /// 会意味着不同的视觉位置和尺寸。布局以
    /// 主屏幕的比例值存储，所以只需把这些比例值
    /// 重新套用到新的分辨率上：视觉位置相同，宽高
    /// 则随分辨率缩放（2160 -> 1080 正好是一半）。
    fn follow_display(&mut self) {
        let Some(hwnd) = self.hwnd else { return };
        let Some((sw, sh)) = (unsafe { win32::get_screen_size() }) else {
            return;
        };
        if self.last_screen == Some((sw, sh)) {
            return;
        }
        self.last_screen = Some((sw, sh));

        let (x, y, w, h) = config::resolve_rect(&self.config, sw, sh);
        let (x, y) = match unsafe { win32::get_work_area() } {
            Some(a) => (
                x.clamp(a.left, (a.right - w).max(a.left)),
                y.clamp(a.top, (a.bottom - h).max(a.top)),
            ),
            None => (x, y),
        };
        unsafe { win32::resize_window(hwnd, x, y, w, h) };
        // 强制为新尺寸重建圆角区域。
        self.region_key = None;
    }

    fn apply_window_style(&mut self) {
        let Some(hwnd) = self.hwnd else { return };
        unsafe {
            win32::set_border_color(hwnd);
            win32::force_dark_mode(hwnd);
            if self.blur {
                // 清除 DWM 系统背景（窗口失去焦点时它会变白），
                // 改用固定的 acrylic blur。
                win32::set_backdrop_type(hwnd, 0);
                win32::set_acrylic_blur(hwnd, true);
            } else {
                win32::set_acrylic_blur(hwnd, false);
                win32::set_backdrop_type(hwnd, 0); // auto：完全透明
            }
        }
    }

    // ---- 亮度 ----

    fn init_brightness(&mut self) {
        // 只要控制器从未成功过就重新创建它（最初几帧
        // 可能与显示栈存在竞争；双模式切换也会
        // 使旧句柄失效）。可用的控制器会被复用，
        // 只重新读取数值。
        if self.brightness.as_ref().map_or(true, |b| !b.supported) {
            self.brightness = Some(DdcController::new());
        }
        let Some(b) = self.brightness.as_mut() else {
            return;
        };
        if b.supported {
            match b.get_brightness_percent() {
                Ok(p) => {
                    self.brightness_percent = p as f32;
                    self.brightness_text = format!("{p}%");
                    self.status_visible = false;
                    self.brightness_retry = None;
                }
                Err(_) => {
                    self.brightness_text = "--".into();
                    self.schedule_brightness_retry();
                }
            }
        } else {
            self.brightness_text = "--".into();
            self.schedule_brightness_retry();
        }
    }

    /// 以递增的延迟重试（2s/4s/6s，之后每 10s）。最初的
    /// 若干帧可能与显示栈竞争并返回 NULL 的物理显示器
    /// 句柄，所以持续重试，而不是尝试三次后就放弃。
    fn schedule_brightness_retry(&mut self) {
        let attempt = match &self.brightness_retry {
            Some(r) => r.attempt,
            None => 0,
        };
        let delay_secs = match attempt {
            0 => 2,
            1 => 4,
            2 => 6,
            _ => 10,
        };
        self.brightness_retry = Some(BrightnessRetry {
            attempt: attempt + 1,
            at: Instant::now() + Duration::from_secs(delay_secs),
        });
    }

    fn poll_brightness_retry(&mut self) {
        let due = match &self.brightness_retry {
            Some(r) if Instant::now() >= r.at => true,
            _ => false,
        };
        if due {
            self.init_brightness();
        }
    }

    /// 防抖的亮度写入（滑块停止移动 200 ms 后）。
    fn poll_brightness_apply(&mut self) {
        let due = match self.brightness_apply_at {
            Some(t) if Instant::now() >= t => true,
            _ => false,
        };
        if due {
            self.brightness_apply_at = None;
            self.apply_pending_brightness();
        }
    }

    fn apply_pending_brightness(&mut self) {
        let Some(b) = self.brightness.as_mut() else {
            return;
        };
        if !b.supported {
            return;
        }
        let percent = self.brightness_percent.round() as i32;
        match b.set_brightness_percent(percent) {
            Ok(()) => {
                self.brightness_text = format!("{percent}%");
                self.status_visible = false;
            }
            Err(e) => {
                self.status = format!("Brightness failed: {e}");
                self.status_visible = true;
            }
        }
    }

    // ---- 双模式 ----

    fn start_dual_switch(&mut self) {
        self.switching_dual = true;
        let target = if self.dual_mode_on {
            DualMode::Mode4K
        } else {
            DualMode::Mode1K
        };
        let tx = self.dual_tx.clone();
        std::thread::spawn(move || {
            let result = dual_mode::switch_to(target);
            let _ = tx.send((target, result));
        });
    }

    fn handle_dual_result(&mut self) {
        let Ok((_target, result)) = self.dual_rx.try_recv() else {
            return;
        };
        self.switching_dual = false;
        match result {
            Ok(()) => {
                let now = dual_mode::get_current_mode();
                if now != DualMode::Unknown {
                    self.dual_mode_on = now == DualMode::Mode1K;
                }
                // 面板重新握手已使缓存的显示器句柄失效；
                // 通过重试刷新亮度。
                self.brightness_retry = None;
                self.init_brightness();
            }
            Err(e) => {
                self.status = format!("Mode switch failed: {e}");
                self.status_visible = true;
                self.dual_mode_on = !self.dual_mode_on;
            }
        }
    }

    // ---- tray ----

    fn handle_tray_event(&mut self, ctx: &egui::Context, ev: TrayEvent) {
        match ev {
            TrayEvent::ToggleTransparent => {
                self.transparent = !self.transparent;
                if self.transparent {
                    self.blur = false;
                }
                self.apply_window_style();
            }
            TrayEvent::ToggleBlur => {
                self.blur = !self.blur;
                if self.blur {
                    self.transparent = false;
                }
                self.apply_window_style();
            }
            TrayEvent::ToggleAutoStart => {
                let wanted = !self.auto_start;
                if set_auto_start_enabled(wanted) {
                    self.auto_start = wanted;
                } else {
                    // 注册表写入失败：显示真实状态，而不是
                    // 一个与 Windows 实际行为不符的勾选标记。
                    self.auto_start = is_auto_start_enabled();
                    self.status = "Auto-start change failed".into();
                    self.status_visible = true;
                }
            }
            TrayEvent::Exit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if let Some(tray) = self.tray.as_mut() {
            tray.set_states(self.transparent, self.blur, self.auto_start);
        }
    }

    // ---- 拖动 / 缩放 ----

    fn start_drag(&mut self) {
        if self.pinned {
            return;
        }
        let Some(hwnd) = self.hwnd else { return };
        if let (Some(cursor), Some(rect)) = (
            unsafe { win32::get_cursor_pos() },
            unsafe { win32::get_window_rect(hwnd) },
        ) {
            self.drag = Some(DragState {
                start: cursor,
                win_x: rect.left,
                win_y: rect.top,
            });
        }
    }

    fn do_drag(&mut self) {
        let Some((hwnd, state)) = self.hwnd.zip(self.drag.as_ref()) else {
            return;
        };
        let Some(cursor) = (unsafe { win32::get_cursor_pos() }) else {
            return;
        };
        unsafe {
            win32::move_window(
                hwnd,
                state.win_x + cursor.x - state.start.x,
                state.win_y + cursor.y - state.start.y,
            );
        }
    }

    fn end_drag(&mut self) {
        self.drag = None;
        self.save_window_state();
    }

    fn start_resize(&mut self, dir: &str) {
        let Some(cursor) = (unsafe { win32::get_cursor_pos() }) else {
            return;
        };
        let Some(hwnd) = self.hwnd else { return };
        if let Some(rect) = unsafe { win32::get_window_rect(hwnd) } {
            self.resize = Some(ResizeState {
                dir: dir.to_string(),
                start: cursor,
                x: rect.left,
                y: rect.top,
                w: rect.width(),
                h: rect.height(),
            });
        }
    }

    fn do_resize(&mut self) {
        let Some((hwnd, state)) = self.hwnd.zip(self.resize.as_ref()) else {
            return;
        };
        let Some(cursor) = (unsafe { win32::get_cursor_pos() }) else {
            return;
        };
        let dx = cursor.x - state.start.x;
        let dy = cursor.y - state.start.y;

        let mut x = state.x;
        let mut y = state.y;
        let mut w = state.w;
        let mut h = state.h;

        if state.dir.contains('R') {
            w = (state.w + dx).max(MIN_W as i32);
        } else if state.dir.contains('L') {
            w = (state.w - dx).max(MIN_W as i32);
            x = state.x + (state.w - w);
        }
        if state.dir.contains('B') {
            h = (state.h + dy).max(MIN_H as i32);
        } else if state.dir.contains('T') {
            h = (state.h - dy).max(MIN_H as i32);
            y = state.y + (state.h - h);
        }

        unsafe { win32::resize_window(hwnd, x, y, w, h) };
    }

    fn end_resize(&mut self) {
        self.resize = None;
        self.save_window_state();
    }

    fn save_window_state(&mut self) {
        let Some(hwnd) = self.hwnd else { return };
        if let Some(rect) = unsafe { win32::get_window_rect(hwnd) } {
            self.config.transparent = self.transparent;
            self.config.blur = self.blur;
            self.config.pinned = self.pinned;
            match unsafe { win32::get_screen_size() } {
                // 同时以绝对值和屏幕比例值存储该矩形，另存
                // 每屏幕像素的 DPI，以便日后重建 ppp。
                Some((sw, sh)) => {
                    config::capture_rect(
                        &mut self.config,
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        sw,
                        sh,
                    );
                    if sw > 0 && self.native_ppp > 0.0 {
                        self.config.ppp_per_px = self.native_ppp / sw as f32;
                    }
                }
                None => {
                    self.config.x = rect.left;
                    self.config.y = rect.top;
                    self.config.w = rect.width();
                    self.config.h = rect.height();
                }
            }
            config::save(&self.config);
        }
    }

    // ---- ui ----

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let size = ui.available_size();
        let w = size.x;
        let h = size.y;
        let center_x = w / 2.0;

        let painter = ui.painter().clone();

        // 卡片填满整个窗口（操作系统窗口区域会将它裁剪为
        // 圆角形状，因此没有外边距）。填充始终是
        // 透明的：blur 模式的色调来自 acrylic 材质，
        // 透明模式则完全透视。
        let card = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h));
        let card_fill = Color32::TRANSPARENT;
        // 1px #28292A 边框，向内缩进半个描边宽度，以免
        // 被圆角窗口区域裁掉。
        let border_w = 1.0_f32;
        let inset = border_w * 0.5;
        let border_rect = card.shrink(inset);
        painter.rect(
            border_rect,
            Rounding::same((CARD_RADIUS - inset).max(0.0)),
            card_fill,
            Stroke::new(border_w, BORDER),
        );

        // ---- 沿卡片边缘的缩放热区（位于窗口内部）----
        // 角落热区为 16x16，因为圆角区域会裁掉
        // 外侧 8x8 的角方块。
        let cz = EDGE * 2.0;
        let gutters = [
            ("L", Rect::from_min_size(egui::pos2(0.0, EDGE), egui::vec2(EDGE, h - EDGE * 2.0)), CursorIcon::ResizeHorizontal),
            ("R", Rect::from_min_size(egui::pos2(w - EDGE, EDGE), egui::vec2(EDGE, h - EDGE * 2.0)), CursorIcon::ResizeHorizontal),
            ("T", Rect::from_min_size(egui::pos2(cz, 0.0), egui::vec2(w - cz * 2.0, EDGE)), CursorIcon::ResizeVertical),
            ("B", Rect::from_min_size(egui::pos2(cz, h - EDGE), egui::vec2(w - cz * 2.0, EDGE)), CursorIcon::ResizeVertical),
            ("TL", Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(cz, cz)), CursorIcon::ResizeNwSe),
            ("TR", Rect::from_min_size(egui::pos2(w - cz, 0.0), egui::vec2(cz, cz)), CursorIcon::ResizeNeSw),
            ("BL", Rect::from_min_size(egui::pos2(0.0, h - cz), egui::vec2(cz, cz)), CursorIcon::ResizeNeSw),
            ("BR", Rect::from_min_size(egui::pos2(w - cz, h - cz), egui::vec2(cz, cz)), CursorIcon::ResizeNwSe),
        ];
        for (dir, rect, cursor) in gutters {
            let resp = ui.interact(rect, ui.id().with(("resize", dir)), Sense::drag());
            if resp.hovered() {
                ctx.set_cursor_icon(cursor);
            }
            if resp.drag_started() {
                self.start_resize(dir);
            }
            if resp.dragged() {
                self.do_resize();
                ctx.request_repaint();
            }
            if resp.drag_stopped() {
                self.end_resize();
            }
        }

        // ---- 标题栏（拖动）+ 固定按钮 ----
        // 与顶部缩放边缘以及 16x16 的角落热区保持距离。
        let title_rect = Rect::from_min_size(
            egui::pos2(cz, EDGE),
            egui::vec2(card.width() - cz * 2.0, 34.0 - EDGE),
        );
        let title_resp = ui.interact(title_rect, ui.id().with("title_drag"), Sense::drag());
        if title_resp.drag_started() {
            self.start_drag();
        }
        if title_resp.dragged() {
            self.do_drag();
            ctx.request_repaint();
        }
        if title_resp.drag_stopped() {
            self.end_drag();
        }

        let pin_rect = Rect::from_min_size(
            egui::pos2(card.right() - 34.0, card.top() + 6.0),
            egui::vec2(26.0, 24.0),
        );
        let pin_resp = ui.interact(pin_rect, ui.id().with("pin"), Sense::click());
        if pin_resp.clicked() {
            self.pinned = !self.pinned;
            // 立即持久化：固定状态不与任何窗口移动或
            // 缩放绑定，若不在此保存，下次启动就会丢失。
            self.save_window_state();
        }
        let pin_glyph = if self.pinned { '\u{E77A}' } else { '\u{E718}' };
        let pin_color = if self.pinned { RED } else { GRAY };
        painter.text(
            pin_rect.center(),
            Align2::CENTER_CENTER,
            pin_glyph.to_string(),
            FontId::proportional(13.0),
            pin_color,
        );
        painter.text(
            egui::pos2(card.left() + 12.0, card.top() + 17.0),
            Align2::LEFT_CENTER,
            "ROG",
            FontId::proportional(12.0),
            RED,
        );

        // ---- 亮度行：灯泡 | 滑块 | 百分比 ----
        let row1_y = card.top() + 34.0 + 22.0;
        painter.text(
            egui::pos2(card.left() + 14.0, row1_y),
            Align2::LEFT_CENTER,
            '\u{EA80}'.to_string(),
            FontId::proportional(16.0),
            RED,
        );
        // 自定义滑块（比默认的更长）：深色滑轨、红色填充、
        // 白色圆形手柄。可在任意位置拖动或点击跳转。右侧留白
        // 复刻 C# 的网格布局（12 外侧内边距 + 42 标签 + 8 间距），
        // 因此滑轨在百分比数字前就结束。
        let slider_rect = Rect::from_min_size(
            egui::pos2(card.left() + 36.0, row1_y - 9.0),
            egui::vec2(card.width() - 36.0 - 62.0, 18.0),
        );
        let mut val = self.brightness_percent;
        let slider_resp =
            ui.interact(slider_rect, ui.id().with("bright_slider"), Sense::drag());
        let mut changed = false;
        if slider_resp.is_pointer_button_down_on() {
            if let Some(pos) = slider_resp.interact_pointer_pos() {
                let t = ((pos.x - slider_rect.left()) / slider_rect.width()).clamp(0.0, 1.0);
                val = (t * 100.0).round();
                changed = true;
            }
        }
        let rail_h = 4.0_f32;
        let rail = Rect::from_center_size(
            slider_rect.center(),
            egui::vec2(slider_rect.width(), rail_h),
        );
        painter.rect_filled(rail, Rounding::same(2.0), Color32::from_rgb(0x3A, 0x3B, 0x40));
        let fill_w = (rail.width() * val / 100.0).clamp(0.0, rail.width());
        let fill_rect = Rect::from_min_size(rail.left_top(), egui::vec2(fill_w, rail_h));
        painter.rect_filled(fill_rect, Rounding::same(2.0), RED);
        let active = slider_resp.hovered() || slider_resp.is_pointer_button_down_on();
        let handle_r = if active { 8.0 } else { 7.0 };
        // 让滑块手柄保持在滑轨内：在 100%（以及 0%）时它以前会
        // 越过滑轨末端并盖住百分比数字。
        let min_x = rail.left() + handle_r;
        let max_x = (rail.right() - handle_r).max(min_x);
        let handle_x = (rail.left() + fill_w).clamp(min_x, max_x);
        painter.circle_filled(
            egui::pos2(handle_x, rail.center().y),
            handle_r,
            Color32::WHITE,
        );
        if changed {
            self.brightness_percent = val;
            self.brightness_text = format!("{}%", val.round() as i32);
            // 防抖：手柄停止移动后稍作延迟再应用。
            self.brightness_apply_at = Some(Instant::now() + Duration::from_millis(200));
        }
        painter.text(
            egui::pos2(card.right() - 12.0, row1_y),
            Align2::RIGHT_CENTER,
            self.brightness_text.clone(),
            FontId::proportional(15.0),
            RED,
        );

        // ---- 双模式行：4K | 开关 | 1K ----
        let row2_y = card.top() + 34.0 + 62.0;
        painter.text(
            egui::pos2(card.left() + 14.0, row2_y),
            Align2::LEFT_CENTER,
            "4K",
            FontId::proportional(15.0),
            RED,
        );
        let switch_rect = Rect::from_center_size(egui::pos2(center_x, row2_y), egui::vec2(46.0, 22.0));
        let switch_resp = ui.interact(switch_rect, ui.id().with("dual_switch"), Sense::click());
        if switch_resp.clicked() && !self.switching_dual && self.dual_mode_known {
            self.start_dual_switch();
        }
        painter.rect(
            switch_rect,
            Rounding::same(11.0),
            Color32::TRANSPARENT,
            Stroke::new(1.5_f32, RED),
        );
        let dot_r = 6.0;
        let dot_center = if self.dual_mode_on {
            switch_rect.right_center() - egui::vec2(5.0 + dot_r, 0.0)
        } else {
            switch_rect.left_center() + egui::vec2(5.0 + dot_r, 0.0)
        };
        painter.circle(dot_center, dot_r, RED, Stroke::NONE);
        painter.text(
            egui::pos2(card.right() - 12.0, row2_y),
            Align2::RIGHT_CENTER,
            "1K",
            FontId::proportional(15.0),
            RED,
        );

        // ---- 状态行 ----
        if self.status_visible {
            painter.text(
                egui::pos2(center_x, row2_y + 32.0),
                Align2::CENTER_CENTER,
                self.status.clone(),
                FontId::proportional(11.0),
                Color32::from_gray(170),
            );
        }
    }
}

impl eframe::App for RogApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.inited {
            // 真正的 HWND 来自 eframe 的原生窗口句柄；
            // EnumWindows 启发式方法并不可靠（窗口此时尚未找到）。
            self.hwnd = frame
                .window_handle()
                .ok()
                .and_then(|h| match h.as_raw() {
                    raw_window_handle::RawWindowHandle::Win32(w) => {
                        Some(HWND(w.hwnd.get() as isize as *mut c_void))
                    }
                    _ => None,
                });
            self.init_window();
            self.inited = true;
        }

        // 若显示模式改变了控件，则重新缩放/重新定位它。此项在
        // update_region 之前完成，以便在同一帧内为新尺寸
        // 重建圆角区域。
        self.follow_display();

        // 保持圆角窗口区域与尺寸/DPI 同步。
        self.update_region(ctx);

        // 不出现在任务栏 / Alt+Tab 中：只有 tray 图标代表本应用。
        // 在此处反复设置而不是启动时只设置一次，是因为 winit 在
        // 处理缩放或 DPI 事件时会依据自身的标志位
        // 重建窗口样式，从而把 WS_EX_APPWINDOW 加回来。开销很低：
        // 样式已匹配时会提前返回。
        if let Some(hwnd) = self.hwnd {
            unsafe { win32::hide_from_taskbar(hwnd) };
        }

        // Windows 缩放在这里很关键：面板使用“推荐”缩放，
        // 因此 4K 报告 150%，而 1K 报告 100%。通过应用
        // zoom = target_ppp / native_ppp 让控件的*逻辑*
        // 尺寸保持恒定（使其布局在两种模式下看起来一致），
        // 其中 target_ppp 是捕获的每屏幕像素 ppp 乘以当前屏幕宽度。
        // 在矩形被捕获时的分辨率下，这正好是 1.0，即
        // 控件完全遵循 Windows 的缩放设置。
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        self.native_ppp = native;
        if let Some((sw, _)) = self.last_screen {
            let target = if self.config.ppp_per_px > 0.0 {
                self.config.ppp_per_px * sw as f32
            } else {
                native
            };
            if native > 0.0 {
                ctx.set_zoom_factor((target / native).clamp(0.2, 5.0));
            }
        }

        // tray 事件。
        while let Some(ev) = self.tray.as_ref().and_then(|t| t.poll()) {
            self.handle_tray_event(ctx, ev);
        }
        // 双模式后台结果。
        self.handle_dual_result();
        // 亮度启动重试。
        self.poll_brightness_retry();
        // 防抖的亮度写入。
        self.poll_brightness_apply();

        // 临时调试：指针可达性 + 状态快照（限流）。
        let now = std::time::Instant::now();
        let due = self.last_debug.map_or(true, |t| now.duration_since(t).as_millis() >= 300);
        if due {
            self.last_debug = Some(now);
            let ptr = ctx.input(|i| i.pointer.hover_pos());
            let ptr_down = ctx.input(|i| i.pointer.primary_down());
            let win = self.hwnd.map(|h| unsafe { win32::get_window_rect(h) });
            let msg = format!(
                "ptr={ptr:?} down={ptr_down} win={win:?} pinned={} transp={} blur={} b_sup={:?} b_reason={:?} b_txt={} b_pct={:.0} dual_known={} dual_on={} switching={} drag={} resize={}",
                self.pinned, self.transparent, self.blur,
                self.brightness.as_ref().map(|b| b.supported),
                self.brightness.as_ref().map(|b| b.reason.clone()),
                self.brightness_text, self.brightness_percent,
                self.dual_mode_known, self.dual_mode_on, self.switching_dual,
                self.drag.is_some(), self.resize.is_some(),
            );
            self.debug_log(&msg);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| self.ui(ui, ctx));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4] // 透明窗口：显示桌面，或 acrylic 背景
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_window_state();
    }
}

// ---- 字体（MDL2 用于 Segoe 图标字形）----
//
// 刻意不加载 9.3 MB 的 SimHei CJK 字体：它原本只为
// 三行中文状态文本而存在，却耗费约 29 MB 常驻内存（字体字节
// 加上 egui 的解析/栅格化），而这些东西几乎从不显示。现在所有 UI
// 字符串都是 ASCII，因此内置字体足以覆盖全部内容，而 tray
// 菜单（由 Windows 自行渲染）仍保留其中文标签。
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segmdl2.ttf") {
        fonts
            .font_data
            .insert("mdl2".into(), egui::FontData::from_owned(bytes));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&fam) {
                list.push("mdl2".into());
            }
        }
    }
    ctx.set_fonts(fonts);
}

// ---- 开机自启（注册表 Run 键）----

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "RogControl";

pub fn is_auto_start_enabled() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(RUN_KEY) else {
        return false;
    };
    key.get_value::<String, _>(RUN_VALUE).is_ok()
}

/// 通过每用户 Run 键启用/禁用开机自启，并返回
/// 注册表当前是否真的处于该状态。
///
/// 禁用路径必须以 `KEY_SET_VALUE` 打开该键：`open_subkey` 会
/// 以只读方式（`KEY_READ`）打开它，此时 `RegDeleteValueW` 会以
/// ACCESS_DENIED 失败。这个失败以前被 `let _ =` 吞掉，因此
/// 取消勾选 tray 项时实际上静默地保留了开机自启。
pub fn set_auto_start_enabled(enable: bool) -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    if enable {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "RogControl.exe".into());
        if let Ok(key) = hkcu.create_subkey(RUN_KEY) {
            let _ = key.0.set_value(RUN_VALUE, &format!("\"{exe}\""));
        }
    } else if let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
        // 值不存在也没关系：说明它已处于禁用状态。
        let _ = key.delete_value(RUN_VALUE);
    }

    is_auto_start_enabled() == enable
}
