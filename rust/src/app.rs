//! 基于 egui 的 RogControl 挂件：透明无边框卡片，包含亮度
//! 滑块（DDC/CI）、4K/1K 双模式开关（VCP 0xFD）、拖动与缩放。
//! 另外持有主界面（独立视口）、DDC worker、设置与全局快捷键。

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use raw_window_handle::HasWindowHandle;

use egui::{Align2, Color32, CursorIcon, FontId, Rect, Rounding, Sense, Stroke};
use windows::Win32::Foundation::HWND;

use crate::config::{self, Config};
use crate::ddc::DdcController;
use crate::dual_mode::{self, DualMode};
use crate::hotkeys::HotkeyRuntime;
use crate::main_window::{MainShared, MainUi};
use crate::store::{self, AppSettings, WidgetConfig};
use crate::theme;
use crate::tray::{Tray, TrayEvent};
use crate::vcp::{self, Lang};
use crate::win32;
use crate::worker::{self, Job, UiState};

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

// 小挂件动态布局
const WIDGET_HEADER_H: f32 = 38.0;
const WIDGET_ROW_H: f32 = 28.0;
const WIDGET_FRB_H: f32 = 30.0;
const WIDGET_SCENE_MARGIN: f32 = 15.0;
const WIDGET_PAD: f32 = 10.0;

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
    // tray
    tray: Option<Tray>,
    // 主界面 / DDC worker / 共享设置
    shared_state: Arc<Mutex<UiState>>,
    app_settings: Arc<Mutex<AppSettings>>,
    hotkeys: Arc<Mutex<HotkeyRuntime>>,
    worker: worker::WorkerHandle,
    main_win: MainShared,
    show_main_flag: Arc<AtomicBool>,
    // 桌面组件配置缓存 / 乐观值与节流
    last_widget_cfg: WidgetConfig,
    last_scene_count: usize,
    widget_height_synced: bool,
    widget_local: HashMap<u8, (u32, Instant)>,
    widget_throttle: HashMap<u8, Instant>,
    // Frame Rate Boost 切换完成后需要重新初始化亮度句柄
    was_switching_boost: bool,
    // 切换期间已解除桌面层挂接（避免 Explorer 重建桌面时被销毁）
    boost_detached: bool,
    // 上一次为其构建圆角窗口区域的 (w_px, h_px, radius_px)
    region_key: Option<(i32, i32, i32)>,
    // 上一次据此解析布局的主屏幕分辨率
    last_screen: Option<(i32, i32)>,
    // Windows 报告的原生 DPI（即 egui 每点像素数，未施加我们的缩放）
    native_ppp: f32,
    // 应用 zoom 后的实际每点像素数（用于挂件高度换算）
    effective_ppp: f32,
    // 启动后主动轮询一段时间，确保 worker 扫描完成时挂件能刷新
    startup_poll_until: Instant,
    last_startup_scan: Instant,
}

fn widget_rail_rect(rect: Rect) -> Rect {
    let x = rect.left() + 86.0;
    let width = (rect.width() - 86.0 - 58.0).max(40.0);
    Rect::from_center_size(
        egui::pos2(x + width / 2.0, rect.center().y),
        egui::vec2(width, 4.0),
    )
}

fn paint_widget_slider(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    text: &str,
    ratio: f32,
    enabled: bool,
    hovered: bool,
) {
    let label_color = if enabled { theme::TEXT_DIM } else { GRAY };
    let accent = if enabled { RED } else { GRAY };
    painter.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        label_color,
    );
    let rail = widget_rail_rect(rect);
    painter.rect_filled(rail, Rounding::same(2.0), Color32::from_rgb(0x3A, 0x3B, 0x40));
    let ratio = ratio.clamp(0.0, 1.0);
    let fill_w = rail.width() * ratio;
    painter.rect_filled(
        Rect::from_min_size(rail.left_top(), egui::vec2(fill_w, rail.height())),
        Rounding::same(2.0),
        accent,
    );
    let handle_r = if hovered && enabled { 7.0 } else { 6.0 };
    let min_x = rail.left() + handle_r;
    let max_x = (rail.right() - handle_r).max(min_x);
    let handle_x = (rail.left() + fill_w).clamp(min_x, max_x);
    painter.circle_filled(
        egui::pos2(handle_x, rail.center().y),
        handle_r,
        Color32::WHITE,
    );
    painter.text(
        rect.right_center(),
        Align2::RIGHT_CENTER,
        text,
        FontId::proportional(11.5),
        accent,
    );
}

fn widget_scene_slot(width_points: f32) -> f32 {
    // 内容区宽 = 窗口宽 - 左右内边距（14 + 12）；每行固定 4 列。
    let content = (width_points - 26.0).max(80.0);
    content / 4.0
}

fn widget_scene_cell(width_points: f32) -> f32 {
    // 每个方格四周各留 15px 外边距，因此边长 = 列宽 - 30。
    (widget_scene_slot(width_points) - 2.0 * WIDGET_SCENE_MARGIN).max(16.0)
}

fn scene_rows(count: usize) -> usize {
    count.div_ceil(4)
}

impl RogApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        store::append_log("lifecycle.log", "app_new");
        theme::setup_fonts(&cc.egui_ctx);
        // 小挂件现在可以显示中文标签（亮度/对比度/场景…），启动即加载 CJK 字体。
        theme::ensure_cjk(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);

        let config = config::load();
        let auto_start = store::is_auto_start_enabled();

        let app_settings = Arc::new(Mutex::new(store::load_settings()));
        let profiles = Arc::new(Mutex::new(store::load_profiles()));
        let shared_state = Arc::new(Mutex::new(UiState::default()));
        let worker = worker::spawn(
            shared_state.clone(),
            app_settings.clone(),
            cc.egui_ctx.clone(),
        );

        let hotkeys = Arc::new(Mutex::new(HotkeyRuntime::default()));
        let hotkeys_enabled = app_settings
            .lock()
            .map(|settings| settings.hotkeys)
            .unwrap_or(true);
        if hotkeys_enabled {
            if let Ok(mut runtime) = hotkeys.lock() {
                runtime.start(&worker);
            }
        }

        let icon = crate::tray::icon_rgba().map(|(rgba, width, height)| {
            Arc::new(egui::IconData {
                rgba,
                width,
                height,
            })
        });
        let main_ui = Arc::new(Mutex::new(MainUi::default()));
        let main_win = MainShared::new(
            shared_state.clone(),
            main_ui,
            app_settings.clone(),
            profiles.clone(),
            hotkeys.clone(),
            worker.clone(),
            icon,
        );
        let show_main_flag = Arc::new(AtomicBool::new(false));
        win32::spawn_show_listener(
            win32::SHOW_EVENT_NAME,
            show_main_flag.clone(),
            cc.egui_ctx.clone(),
        );

        let tray = Tray::new(
            config.transparent,
            config.blur,
            auto_start,
            cc.egui_ctx.clone(),
        );

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
            tray,
            shared_state,
            app_settings,
            hotkeys,
            worker,
            main_win,
            show_main_flag,
            last_widget_cfg: WidgetConfig::default(),
            last_scene_count: 0,
            widget_height_synced: false,
            widget_local: HashMap::new(),
            widget_throttle: HashMap::new(),
            was_switching_boost: false,
            boost_detached: false,
            region_key: None,
            last_screen: None,
            native_ppp: 1.0,
            effective_ppp: 1.0,
            startup_poll_until: Instant::now() + Duration::from_secs(60),
            last_startup_scan: Instant::now(),
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
        store::append_log(
            "lifecycle.log",
            &format!("follow_display {:?} -> {}x{}", self.last_screen, sw, sh),
        );
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
        // 1K/485 Hz 模式下这块面板不响应 DDC 读取，硬读会阻塞 UI 线程。
        if dual_mode::get_current_mode() == DualMode::Mode1K {
            self.brightness = None;
            self.brightness_text = "--".into();
            self.brightness_retry = None;
            return;
        }
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
        if dual_mode::get_current_mode() == DualMode::Mode1K {
            return;
        }
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
        if dual_mode::get_current_mode() == DualMode::Mode1K {
            return;
        }
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
                // 让主界面/worker 快照里的 VCP 0x10 同步更新。
                self.worker.send(Job::Read(crate::vcp::VCP_BRIGHTNESS));
            }
            Err(e) => {
                self.status = format!("Brightness failed: {e}");
                self.status_visible = true;
            }
        }
    }

    // ---- Frame Rate Boost ----

    fn boost_status(&self) -> (bool, bool, bool) {
        self.shared_state
            .lock()
            .map(|state| {
                (
                    state.boost_known,
                    state.boost_one_k,
                    state.boost_switching,
                )
            })
            .unwrap_or((false, false, false))
    }

    // ---- tray ----

    fn handle_tray_event(&mut self, ctx: &egui::Context, ev: TrayEvent) {
        match ev {
            TrayEvent::ShowMain => {
                self.main_win.request_open(ctx);
            }
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
                if store::set_auto_start_enabled(wanted) {
                    self.auto_start = wanted;
                } else {
                    // 注册表写入失败：显示真实状态，而不是
                    // 一个与 Windows 实际行为不符的勾选标记。
                    self.auto_start = store::is_auto_start_enabled();
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
        // 场景格子是正方形：宽度变化会改变所需的最小高度。
        let ppp = self.effective_ppp.max(0.25);
        let cfg = self.widget_cfg();
        let scene_count = self.scene_mode_count(&cfg);
        let min_height = self.widget_min_height_px_for(&cfg, w as f32 / ppp, scene_count);
        if state.dir.contains('B') {
            h = (state.h + dy).max(min_height);
        } else if state.dir.contains('T') {
            h = (state.h - dy).max(min_height);
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

    // ---- 桌面组件 ----

    fn lang(&self) -> Lang {
        self.app_settings
            .lock()
            .map(|settings| Lang::from_code(&settings.language))
            .unwrap_or_default()
    }

    fn widget_cfg(&self) -> WidgetConfig {
        self.app_settings
            .lock()
            .map(|settings| settings.widget.clone())
            .unwrap_or_default()
    }

    fn widget_content_height(
        &self,
        cfg: &WidgetConfig,
        width_points: f32,
        scene_count: usize,
    ) -> f32 {
        let mut height = WIDGET_HEADER_H + cfg.slider_count() as f32 * WIDGET_ROW_H;
        if cfg.frame_rate_boost {
            height += WIDGET_FRB_H;
        }
        if cfg.scenes && scene_count > 0 {
            let cell = widget_scene_cell(width_points);
            height += 4.0 + scene_rows(scene_count) as f32 * (cell + 2.0 * WIDGET_SCENE_MARGIN);
        }
        height + WIDGET_PAD
    }

    fn scene_mode_count(&self, cfg: &WidgetConfig) -> usize {
        let Ok(state) = self.shared_state.lock() else {
            return 0;
        };
        let Some(view) = state.monitor() else {
            return 0;
        };
        let all_keys: Vec<&'static str> = vcp::mode_set(view.line)
            .iter()
            .filter(|mode| view.mode_supported(mode.value))
            .map(|mode| mode.key)
            .collect();
        cfg.ordered_scene_keys(&all_keys).len()
    }

    fn widget_min_height_px_for(
        &self,
        cfg: &WidgetConfig,
        width_points: f32,
        scene_count: usize,
    ) -> i32 {
        let ppp = self.effective_ppp.max(0.25);
        ((self.widget_content_height(cfg, width_points, scene_count) * ppp).round() as i32)
            .max((MIN_H * ppp).round() as i32)
    }

    /// 勾选项变化时调整挂件高度，让所有行都放得下。
    fn sync_widget_height(&mut self, ctx: &egui::Context) {
        let cfg = self.widget_cfg();
        let scene_count = self.scene_mode_count(&cfg);
        let first_sync = !self.widget_height_synced;
        if !first_sync && cfg == self.last_widget_cfg && scene_count == self.last_scene_count {
            return;
        }
        self.last_widget_cfg = cfg.clone();
        self.last_scene_count = scene_count;
        self.widget_height_synced = true;
        if self.resize.is_some() {
            return;
        }
        let Some(hwnd) = self.hwnd else {
            return;
        };
        let Some(rect) = (unsafe { win32::get_window_rect(hwnd) }) else {
            return;
        };
        let ppp = self.effective_ppp.max(0.25);
        let width_points = rect.width() as f32 / ppp;
        let target = self.widget_min_height_px_for(&cfg, width_points, scene_count);
        // 启动时只放大不缩小，保留用户手动调过的高度；之后按勾选项精确贴合。
        if first_sync && rect.height() >= target {
            return;
        }
        if (rect.height() - target).abs() > 2 {
            unsafe {
                win32::resize_window(hwnd, rect.left, rect.top, rect.width(), target);
            }
            self.config.h = (target as f32 / ppp).round() as i32;
            config::save(&self.config);
            self.region_key = None;
            ctx.request_repaint();
        }
    }

    fn widget_value(&self, code: u8, view: &worker::MonitorView) -> u32 {
        if let Some((value, at)) = self.widget_local.get(&code) {
            if at.elapsed() < Duration::from_secs(2) {
                return *value;
            }
        }
        view.value(code).unwrap_or(0)
    }

    fn send_widget_write(&mut self, code: u8, value: u32, label: &str, force: bool) {
        self.widget_local.insert(code, (value, Instant::now()));
        let due = force
            || self
                .widget_throttle
                .get(&code)
                .is_none_or(|at| at.elapsed() >= Duration::from_millis(140));
        if due {
            self.widget_throttle.insert(code, Instant::now());
            self.worker.send(Job::Write {
                code,
                value,
                label: label.to_string(),
            });
        }
    }

    fn widget_brightness_slider(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        label: &str,
        enabled: bool,
    ) {
        let rail = widget_rail_rect(rect);
        let sense = if enabled { Sense::drag() } else { Sense::hover() };
        let response = ui.interact(rect, ui.id().with("w_brightness"), sense);
        let mut value = self.brightness_percent.clamp(0.0, 100.0);
        let mut changed = false;
        if enabled && response.is_pointer_button_down_on() {
            if let Some(pos) = response.interact_pointer_pos() {
                let t = ((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
                value = (t * 100.0).round();
                changed = true;
            }
        }
        paint_widget_slider(
            painter,
            rect,
            label,
            &self.brightness_text,
            value / 100.0,
            enabled,
            response.hovered(),
        );
        if changed {
            self.brightness_percent = value;
            self.brightness_text = format!("{}%", value.round() as i32);
            self.brightness_apply_at = Some(Instant::now() + Duration::from_millis(200));
        }
        if enabled && response.drag_stopped() {
            self.brightness_apply_at = Some(Instant::now() + Duration::from_millis(40));
        }
    }

    fn widget_vcp_slider(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        code: u8,
        label: &str,
        view: &worker::MonitorView,
        enabled: bool,
    ) {
        let rail = widget_rail_rect(rect);
        let max = view.maximum(code).max(1);
        let mut value = self.widget_value(code, view).min(max);
        let sense = if enabled { Sense::drag() } else { Sense::hover() };
        let response = ui.interact(rect, ui.id().with(("w_slider", code)), sense);
        if enabled && response.is_pointer_button_down_on() {
            if let Some(pos) = response.interact_pointer_pos() {
                let t = ((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
                value = (t * max as f32).round() as u32;
            }
        }
        paint_widget_slider(
            painter,
            rect,
            label,
            &value.to_string(),
            value as f32 / max as f32,
            enabled,
            response.hovered(),
        );
        if enabled {
            if response.drag_stopped() {
                self.send_widget_write(code, value, label, true);
            } else if response.is_pointer_button_down_on() {
                self.send_widget_write(code, value, label, false);
            }
        }
    }

    fn widget_temp_slider(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        label: &str,
        view: &worker::MonitorView,
        enabled: bool,
        lang: Lang,
    ) {
        let options = view.temp_options(lang);
        if options.len() < 2 {
            paint_widget_slider(painter, rect, label, "—", 0.0, false, false);
            return;
        }
        let rail = widget_rail_rect(rect);
        let current = self.widget_value(vcp::VCP_COLOR_TEMP, view);
        let mut index = options
            .iter()
            .position(|(value, _)| *value == current)
            .unwrap_or(0);
        let sense = if enabled { Sense::drag() } else { Sense::hover() };
        let response = ui.interact(rect, ui.id().with("w_temp"), sense);
        if enabled && response.is_pointer_button_down_on() {
            if let Some(pos) = response.interact_pointer_pos() {
                let t = ((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
                index = ((t * (options.len() - 1) as f32).round() as usize).min(options.len() - 1);
            }
        }
        let (value, text) = options[index].clone();
        paint_widget_slider(
            painter,
            rect,
            label,
            &text,
            index as f32 / (options.len() - 1) as f32,
            enabled,
            response.hovered(),
        );
        if enabled {
            if response.drag_stopped() {
                self.send_widget_write(vcp::VCP_COLOR_TEMP, value, label, true);
            } else if response.is_pointer_button_down_on() {
                self.send_widget_write(vcp::VCP_COLOR_TEMP, value, label, false);
            }
        }
    }

    fn widget_scene_button(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        value: u32,
        label: &str,
        enabled: bool,
        selected: bool,
    ) {
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let response = ui.interact(rect, ui.id().with(("w_scene", value)), sense);
        let border = if !enabled {
            GRAY
        } else if selected {
            RED
        } else if response.hovered() {
            Color32::from_rgb(0x3A, 0x3B, 0x40)
        } else {
            // 未选中：深灰边框。
            Color32::from_rgb(0x21, 0x22, 0x23)
        };
        // 透明底、1px 边框；当前模式红框，其余 #212223。
        painter.rect(
            rect,
            Rounding::same(6.0),
            Color32::TRANSPARENT,
            Stroke::new(1.0, border),
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.5),
            if enabled { RED } else { GRAY },
        );
        if response.clicked() && enabled {
            self.worker.send(Job::SetMode {
                value,
                label: label.to_string(),
            });
        }
    }

    fn widget_boost_row(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        known: bool,
        one_k: bool,
        switching: bool,
    ) {
        painter.text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            "4K",
            FontId::proportional(14.0),
            if known { RED } else { GRAY },
        );
        let switch_rect = Rect::from_center_size(rect.center(), egui::vec2(46.0, 22.0));
        let sense = if known && !switching {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(switch_rect, ui.id().with("boost_switch"), sense);
        if response.clicked() {
            self.worker.send(Job::SetBoost { one_k: !one_k });
        }
        let track = if !known || switching { GRAY } else { RED };
        painter.rect(
            switch_rect,
            Rounding::same(11.0),
            Color32::TRANSPARENT,
            Stroke::new(1.5_f32, track),
        );
        let dot_r = 6.0;
        let dot_center = if one_k {
            switch_rect.right_center() - egui::vec2(5.0 + dot_r, 0.0)
        } else {
            switch_rect.left_center() + egui::vec2(5.0 + dot_r, 0.0)
        };
        painter.circle(dot_center, dot_r, track, Stroke::NONE);
        painter.text(
            rect.right_center(),
            Align2::RIGHT_CENTER,
            "1K",
            FontId::proportional(13.0),
            if known { RED } else { GRAY },
        );
        if switching {
            painter.text(
                egui::pos2(rect.center().x, rect.center().y + 16.0),
                Align2::CENTER_CENTER,
                "switching…",
                FontId::proportional(9.0),
                GRAY,
            );
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

        // 打开主界面（也可以从托盘菜单/左键单击打开）。
        let gear_rect = Rect::from_min_size(
            egui::pos2(card.right() - 64.0, card.top() + 6.0),
            egui::vec2(26.0, 24.0),
        );
        let gear_resp = ui.interact(gear_rect, ui.id().with("open_main"), Sense::click());
        if gear_resp.clicked() {
            self.main_win.request_open(ctx);
        }
        painter.text(
            gear_rect.center(),
            Align2::CENTER_CENTER,
            '\u{E713}'.to_string(),
            FontId::proportional(13.0),
            if gear_resp.hovered() { RED } else { GRAY },
        );

        painter.text(
            egui::pos2(card.left() + 12.0, card.top() + 17.0),
            Align2::LEFT_CENTER,
            "ROG",
            FontId::proportional(12.0),
            RED,
        );

        // ---- 可配置的桌面组件 ----
        let cfg = self.widget_cfg();
        let lang = self.lang();
        let snapshot = self
            .shared_state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default();
        let view = snapshot.monitor().cloned();
        let ddc_ready =
            !snapshot.boost_one_k && view.as_ref().is_some_and(|monitor| monitor.ddc_ok);

        let left = card.left() + 14.0;
        let right = card.right() - 12.0;
        let content_w = (right - left).max(80.0);
        let mut y = card.top() + WIDGET_HEADER_H;

        if cfg.brightness {
            let rect =
                Rect::from_min_size(egui::pos2(left, y), egui::vec2(content_w, WIDGET_ROW_H));
            self.widget_brightness_slider(
                ui,
                &painter,
                rect,
                lang.t("亮度", "Brightness"),
                ddc_ready,
            );
            y += WIDGET_ROW_H;
        }
        for (enabled, code, zh, en) in [
            (cfg.contrast, vcp::VCP_CONTRAST, "对比度", "Contrast"),
            (cfg.overdrive, vcp::VCP_OVERDRIVE, "OD", "OD"),
            (cfg.sharpness, vcp::VCP_SHARPNESS, "锐度", "Sharpness"),
        ] {
            if !enabled {
                continue;
            }
            if let Some(view) = &view {
                let rect =
                    Rect::from_min_size(egui::pos2(left, y), egui::vec2(content_w, WIDGET_ROW_H));
                let label = lang.t(zh, en);
                self.widget_vcp_slider(
                    ui,
                    &painter,
                    rect,
                    code,
                    label,
                    view,
                    ddc_ready && view.supported(code),
                );
            }
            y += WIDGET_ROW_H;
        }
        if cfg.color_temp {
            if let Some(view) = &view {
                let rect =
                    Rect::from_min_size(egui::pos2(left, y), egui::vec2(content_w, WIDGET_ROW_H));
                self.widget_temp_slider(
                    ui,
                    &painter,
                    rect,
                    lang.t("色温", "Color temp"),
                    view,
                    ddc_ready && view.supported(vcp::VCP_COLOR_TEMP),
                    lang,
                );
            }
            y += WIDGET_ROW_H;
        }
        if cfg.eye_care {
            if let Some(view) = &view {
                let rect =
                    Rect::from_min_size(egui::pos2(left, y), egui::vec2(content_w, WIDGET_ROW_H));
                self.widget_vcp_slider(
                    ui,
                    &painter,
                    rect,
                    vcp::VCP_BLUE_LIGHT,
                    lang.t("护眼", "Eye care"),
                    view,
                    ddc_ready && view.supported(vcp::VCP_BLUE_LIGHT),
                );
            }
            y += WIDGET_ROW_H;
        }

        if cfg.frame_rate_boost {
            let rect =
                Rect::from_min_size(egui::pos2(left, y), egui::vec2(content_w, WIDGET_FRB_H));
            self.widget_boost_row(
                ui,
                &painter,
                rect,
                snapshot.boost_known,
                snapshot.boost_one_k,
                snapshot.boost_switching,
            );
            y += WIDGET_FRB_H;
        }

        if cfg.scenes {
            if let Some(view) = &view {
                let all_keys: Vec<&'static str> = vcp::mode_set(view.line)
                    .iter()
                    .filter(|mode| view.mode_supported(mode.value))
                    .map(|mode| mode.key)
                    .collect();
                // 按用户勾选顺序排列：先勾选的在前。
                let modes: Vec<(u32, &'static str)> = cfg
                    .ordered_scene_keys(&all_keys)
                    .into_iter()
                    .filter_map(|key| {
                        vcp::mode_set(view.line)
                            .iter()
                            .find(|mode| mode.key == key)
                            .map(|mode| (mode.value, mode.label(lang)))
                    })
                    .collect();
                if !modes.is_empty() {
                    y += 4.0;
                    let slot = widget_scene_slot(card.width());
                    let cell = widget_scene_cell(card.width());
                    let row_height = cell + 2.0 * WIDGET_SCENE_MARGIN;
                    let current = view.current_mode_value();
                    for (index, (value, label)) in modes.iter().enumerate() {
                        let column = (index % 4) as f32;
                        let row = (index / 4) as f32;
                        let rect = Rect::from_min_size(
                            egui::pos2(
                                left + column * slot + WIDGET_SCENE_MARGIN,
                                y + row * row_height + WIDGET_SCENE_MARGIN,
                            ),
                            egui::vec2(cell, cell),
                        );
                        self.widget_scene_button(
                            ui,
                            &painter,
                            rect,
                            *value,
                            label,
                            ddc_ready,
                            current == Some(*value),
                        );
                    }
                    y += scene_rows(modes.len()) as f32 * row_height;
                }
            }
        }

        if self.status_visible {
            painter.text(
                egui::pos2(card.center().x, card.bottom() - 9.0),
                Align2::CENTER_CENTER,
                self.status.clone(),
                FontId::proportional(9.0),
                Color32::from_gray(170),
            );
        }
    }
}

impl Drop for RogApp {
    fn drop(&mut self) {
        // 即使没有走正常的 on_exit 路径，也把界面/桌面组件配置落盘。
        if let Ok(settings) = self.app_settings.lock() {
            store::save_settings(&settings);
        }
        store::append_log("lifecycle.log", "RogApp dropped");
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
        // 布局换算使用应用 zoom 后的实际 ppp。
        self.effective_ppp = (native * ctx.zoom_factor()).max(0.25);

        // 桌面组件勾选项变化后按内容调整小挂件高度。
        self.sync_widget_height(ctx);

        // tray 事件。
        while let Some(ev) = self.tray.as_ref().and_then(|t| t.poll()) {
            self.handle_tray_event(ctx, ev);
        }
        // 第二实例启动时通过命名事件请求显示主界面。
        if self.show_main_flag.swap(false, Ordering::SeqCst) {
            self.main_win.request_open(ctx);
        }
        // 主界面（独立视口，只在打开期间存在）。
        self.main_win.show(ctx);

        // Frame Rate Boost 切换期间解除桌面层挂接，避免 Explorer
        // 重建桌面时把 owned window 一起销毁。
        let (_, boost_one_k, boost_switching) = self.boost_status();
        if boost_switching && !self.boost_detached {
            if let Some(hwnd) = self.hwnd {
                unsafe { win32::detach_from_desktop(hwnd) };
            }
            self.boost_detached = true;
        } else if !boost_switching && self.boost_detached {
            if let Some(hwnd) = self.hwnd {
                unsafe { win32::reattach_to_desktop(hwnd) };
            }
            self.boost_detached = false;
        }

        // 切换完成：面板重新握手会让亮度句柄失效；1K 模式下不读 DDC。
        if self.was_switching_boost && !boost_switching {
            self.brightness_retry = None;
            if boost_one_k {
                self.brightness = None;
                self.brightness_text = "--".into();
                self.status_visible = false;
            } else {
                self.init_brightness();
            }
        }
        self.was_switching_boost = boost_switching;
        // 亮度启动重试。
        self.poll_brightness_retry();
        // 防抖的亮度写入。
        self.poll_brightness_apply();

        // 小挂件是常驻的根视口：开机时 worker 的扫描/repaint 可能早于
        // 视口创建而丢失，导致界面一直停在“未连接、不能调”。
        // 启动阶段主动轮询，并给亮度重试/防抖写入安排下一帧，
        // 这样即使不打开主界面，挂件也会自动恢复可用。
        let (scanning, ready) = self
            .shared_state
            .lock()
            .map(|state| {
                (
                    state.scanning,
                    state.monitor().map(|monitor| monitor.ddc_ok).unwrap_or(false),
                )
            })
            .unwrap_or((false, false));
        // 开机自启时显示器/DDC 可能还没就绪：启动阶段发现未就绪就自动重扫，
        // 避免挂件一直停留在“不可调”状态。
        if !ready
            && !scanning
            && Instant::now() < self.startup_poll_until
            && self.last_startup_scan.elapsed() >= Duration::from_secs(3)
        {
            self.last_startup_scan = Instant::now();
            self.worker.send(Job::Scan { force: true });
        }
        if scanning || !ready {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if let Some(retry) = &self.brightness_retry {
            let wait = retry.at.saturating_duration_since(Instant::now());
            ctx.request_repaint_after(wait.max(Duration::from_millis(16)));
        }
        if let Some(at) = self.brightness_apply_at {
            let wait = at.saturating_duration_since(Instant::now());
            ctx.request_repaint_after(wait.max(Duration::from_millis(16)));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| self.ui(ui, ctx));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4] // 透明窗口：显示桌面，或 acrylic 背景
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        store::append_log("lifecycle.log", "on_exit");
        self.save_window_state();
        if let Ok(settings) = self.app_settings.lock() {
            store::save_settings(&settings);
        }
        if let Ok(mut hotkeys) = self.hotkeys.lock() {
            hotkeys.stop();
        }
        let _ = self.worker.tx.send(Job::Stop);
    }
}


