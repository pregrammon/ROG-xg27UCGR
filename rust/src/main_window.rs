//! 主界面：侧栏 + 6 个页面（概览 / 显示与色彩 / 护眼 / 游戏辅助 / 配置文件 / 设置）。
//!
//! 主界面是一个 egui 延迟视口（独立原生窗口），默认隐藏；托盘菜单
//! “显示主界面”按需创建。关闭主窗口只是停止渲染该视口并把状态置为
//! 关闭，进程与小挂件继续运行。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Color32, Context, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui, Vec2,
    ViewportBuilder, ViewportCommand, ViewportId, WindowLevel,
};

use crate::hotkeys::HotkeyRuntime;
use crate::store::{self, AppSettings, Profile};
use crate::theme as th;
use crate::vcp::{self, Lang, ProductLine};
use crate::win32;
use crate::worker::{self, Job, MonitorView, StatusKind, UiState, WorkerHandle};

pub const MAIN_TITLE: &str = "ROG Control";

// ---------------------------------------------------------------------------
// 页面
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overview,
    Display,
    EyeCare,
    GamePlus,
    Profiles,
    Widget,
    Settings,
}

impl Page {
    pub const ALL: [Page; 7] = [
        Page::Overview,
        Page::Display,
        Page::EyeCare,
        Page::GamePlus,
        Page::Profiles,
        Page::Widget,
        Page::Settings,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Page::Overview => "overview",
            Page::Display => "display",
            Page::EyeCare => "eyecare",
            Page::GamePlus => "gameplus",
            Page::Profiles => "profiles",
            Page::Widget => "widget",
            Page::Settings => "settings",
        }
    }

    pub fn from_key(key: &str) -> Page {
        match key {
            "display" => Page::Display,
            "eyecare" => Page::EyeCare,
            "gameplus" => Page::GamePlus,
            "profiles" => Page::Profiles,
            "widget" => Page::Widget,
            "settings" => Page::Settings,
            _ => Page::Overview,
        }
    }

    pub fn title(self, lang: Lang) -> &'static str {
        match self {
            Page::Overview => lang.t("概览", "Overview"),
            Page::Display => lang.t("显示与色彩", "Display & Color"),
            Page::EyeCare => lang.t("护眼", "Eye Care"),
            Page::GamePlus => lang.t("游戏辅助", "GamePlus"),
            Page::Profiles => lang.t("配置文件", "Profiles"),
            Page::Widget => lang.t("桌面组件", "Desktop Widget"),
            Page::Settings => lang.t("设置", "Settings"),
        }
    }

    pub fn subtitle(self, lang: Lang) -> &'static str {
        match self {
            Page::Overview => lang.t("显示器的实时状态与常用调节", "Live status and everyday controls"),
            Page::Display => lang.t("GameVisual 模式、画面与色彩参数", "GameVisual modes, picture and color"),
            Page::EyeCare => lang.t("滤蓝光、色彩增强与休息提醒", "Blue light, color augmentation and breaks"),
            Page::GamePlus => lang.t("准星、计时器、FPS 与对齐", "Crosshair, timer, FPS and alignment"),
            Page::Profiles => lang.t("保存、应用和交换显示配置", "Save, apply and exchange display presets"),
            Page::Widget => lang.t("勾选要显示在桌面小挂件上的调节项", "Choose the controls shown in the desktop widget"),
            Page::Settings => lang.t("语言、热键、启动与 DDC 选项", "Language, hotkeys, startup and DDC"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    None,
    SaveProfile,
    DeleteProfile(usize),
    ResetAll,
    ResetColor,
    Message(String),
}

pub struct MainUi {
    pub page: Page,
    pub selected_profile: Option<usize>,
    pub dialog: Dialog,
    pub text: String,
    pub vcp_table: bool,
    pub local: HashMap<u8, (u32, Instant)>,
    pub throttle: HashMap<u8, Instant>,
    pub title_styled: bool,
    pub focus_pending: bool,
    pub scroll_top: bool,
    pub resize_guard: Option<Instant>,
    /// 关闭帧里 ViewportInfo 可能已经没有矩形，因此每帧记录最后已知值。
    pub last_outer: Option<egui::Rect>,
    pub last_inner: Option<Vec2>,
}

impl Default for MainUi {
    fn default() -> Self {
        MainUi {
            page: Page::Overview,
            selected_profile: None,
            dialog: Dialog::None,
            text: String::new(),
            vcp_table: false,
            local: HashMap::new(),
            throttle: HashMap::new(),
            title_styled: false,
            focus_pending: true,
            scroll_top: true,
            resize_guard: None,
            last_outer: None,
            last_inner: None,
        }
    }
}

impl MainUi {
    /// 4K→1K 切换后工作区会变小：把主窗口缩到当前显示器内，避免
    /// 底部的 Frame Rate Boost 卡片被挤到屏幕外。
    fn fit_to_monitor(&mut self, ctx: &Context) {
        if self
            .resize_guard
            .is_some_and(|at| at.elapsed() < Duration::from_millis(1000))
        {
            return;
        }
        let (monitor, outer, inner) = ctx.input(|i| {
            let viewport = i.viewport();
            (viewport.monitor_size, viewport.outer_rect, viewport.inner_rect)
        });
        let (Some(monitor), Some(outer), Some(inner)) = (monitor, outer, inner) else {
            return;
        };
        let max_w = (monitor.x - 24.0).max(640.0);
        let max_h = (monitor.y - 72.0).max(480.0);
        if outer.width() <= max_w + 1.0 && outer.height() <= max_h + 1.0 {
            return;
        }
        let scale = (max_w / outer.width())
            .min(max_h / outer.height())
            .min(1.0);
        let new_inner = egui::vec2(
            (inner.width() * scale).max(900.0),
            (inner.height() * scale).max(560.0),
        );
        let x = outer
            .min
            .x
            .clamp(8.0, (monitor.x - new_inner.x - 16.0).max(8.0));
        let y = outer
            .min
            .y
            .clamp(8.0, (monitor.y - new_inner.y - 56.0).max(8.0));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(new_inner));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(x, y)));
        self.last_inner = Some(new_inner);
        self.resize_guard = Some(Instant::now());
    }

    fn display_value(&self, code: u8, fallback: u32) -> u32 {
        if let Some((value, at)) = self.local.get(&code) {
            if at.elapsed() < Duration::from_secs(2) {
                return *value;
            }
        }
        fallback
    }

    fn record_write(&mut self, code: u8, value: u32) {
        self.local.insert(code, (value, Instant::now()));
    }
}

// ---------------------------------------------------------------------------
// 主窗口共享状态
// ---------------------------------------------------------------------------
#[derive(Clone)]
pub struct MainShared {
    pub id: ViewportId,
    pub open: Arc<AtomicBool>,
    pub state: Arc<Mutex<UiState>>,
    pub ui: Arc<Mutex<MainUi>>,
    pub settings: Arc<Mutex<AppSettings>>,
    pub profiles: Arc<Mutex<Vec<Profile>>>,
    pub hotkeys: Arc<Mutex<HotkeyRuntime>>,
    pub worker: WorkerHandle,
    pub icon: Option<Arc<egui::IconData>>,
}

impl MainShared {
    pub fn new(
        state: Arc<Mutex<UiState>>,
        ui: Arc<Mutex<MainUi>>,
        settings: Arc<Mutex<AppSettings>>,
        profiles: Arc<Mutex<Vec<Profile>>>,
        hotkeys: Arc<Mutex<HotkeyRuntime>>,
        worker: WorkerHandle,
        icon: Option<Arc<egui::IconData>>,
    ) -> MainShared {
        MainShared {
            id: ViewportId::from_hash_of("rog_control_main"),
            open: Arc::new(AtomicBool::new(false)),
            state,
            ui,
            settings,
            profiles,
            hotkeys,
            worker,
            icon,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    pub fn request_open(&self, ctx: &Context) {
        let already = self.open.swap(true, Ordering::SeqCst);
        if !already {
            if let Ok(mut ui) = self.ui.lock() {
                ui.title_styled = false;
                ui.focus_pending = true;
                ui.scroll_top = true;
            }
            if let Ok(settings) = self.settings.lock() {
                if let Ok(mut ui) = self.ui.lock() {
                    ui.page = Page::from_key(&settings.last_page);
                }
            }
        }
        ctx.request_repaint();
        ctx.send_viewport_cmd_to(self.id, ViewportCommand::Visible(true));
        ctx.send_viewport_cmd_to(self.id, ViewportCommand::Focus);
    }

    /// 每帧调用；视口只在打开期间存在。
    pub fn show(&self, ctx: &Context) {
        if !self.is_open() {
            return;
        }
        let builder = self.builder();
        let this = self.clone();
        ctx.show_viewport_deferred(self.id, builder, move |ctx, _| {
            render(ctx, &this);
        });
    }

    fn builder(&self) -> ViewportBuilder {
        let settings = self
            .settings
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        let size = settings.main_size.unwrap_or([1200.0, 760.0]);
        let mut builder = ViewportBuilder::default()
            .with_title(MAIN_TITLE)
            .with_inner_size(size)
            .with_min_inner_size([960.0, 620.0])
            .with_resizable(true)
            .with_decorations(true)
            .with_taskbar(true)
            .with_transparent(false);
        if let Some(pos) = settings.main_pos {
            builder = builder.with_position(pos);
        }
        if settings.main_maximized {
            builder = builder.with_maximized(true);
        }
        if settings.topmost {
            builder = builder.with_window_level(WindowLevel::AlwaysOnTop);
        }
        if let Some(icon) = &self.icon {
            builder = builder.with_icon(icon.clone());
        }
        builder
    }
}

// ---------------------------------------------------------------------------
// 渲染入口
// ---------------------------------------------------------------------------
fn render(ctx: &Context, shared: &MainShared) {
    if !shared.is_open() {
        return;
    }
    if ctx.input(|i| i.viewport().close_requested()) {
        // 关闭主界面 = 隐藏，不退出进程。
        save_geometry(ctx, shared);
        shared.open.store(false, Ordering::SeqCst);
        ctx.request_repaint();
        return;
    }

    th::ensure_cjk(ctx);

    let snapshot = shared
        .state
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    let mut prefs = shared
        .ui
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut settings = shared
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut profiles = shared
        .profiles
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut hotkeys = shared
        .hotkeys
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let lang = Lang::from_code(&settings.language);
    let view = snapshot.monitor().cloned();
    let mut dirty_settings = false;
    let mut dirty_profiles = false;

    // 第一次显示：深色标题栏 + 焦点 + 窗口图标。
    if !prefs.title_styled {
        if let Some(hwnd) = unsafe { win32::find_window_by_title(MAIN_TITLE) } {
            unsafe { win32::apply_dark_title(hwnd) };
            prefs.title_styled = true;
        }
        if shared.icon.is_some() {
            ctx.send_viewport_cmd(ViewportCommand::Icon(shared.icon.clone()));
        }
    }
    if prefs.focus_pending {
        prefs.focus_pending = false;
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    // 记录最后已知的窗口矩形，供关闭帧保存几何。
    let (outer_now, inner_now) = ctx.input(|i| {
        let v = i.viewport();
        (v.outer_rect, v.inner_rect)
    });
    if outer_now.is_some() {
        prefs.last_outer = outer_now;
    }
    if let Some(inner) = inner_now {
        prefs.last_inner = Some(inner.size());
    }
    prefs.fit_to_monitor(ctx);

    // ---------------- 状态栏 ----------------
    egui::TopBottomPanel::bottom("main_status")
        .exact_height(28.0)
        .frame(
            Frame::none()
                .fill(th::SIDEBAR)
                .inner_margin(Margin {
                    left: 26.0,
                    right: 26.0,
                    top: 5.0,
                    bottom: 3.0,
                }),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                th::dot(ui, th::status_color(snapshot.status_kind));
                let text = if snapshot.status.is_empty() {
                    lang.t("就绪", "Ready").to_string()
                } else {
                    snapshot.status.clone()
                };
                ui.label(RichText::new(text).size(10.0).color(th::TEXT_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(view) = &view {
                        ui.label(
                            RichText::new(format!(
                                "{}  ·  VCP {}  ·  {}",
                                view.model,
                                view.caps.vcp_count(),
                                view.resolution
                            ))
                            .size(10.0)
                            .color(th::MUTED),
                        );
                    }
                });
            });
        });

    // ---------------- 侧栏 ----------------
    egui::SidePanel::left("main_nav")
        .exact_width(224.0)
        .resizable(false)
        .frame(
            Frame::none()
                .fill(th::SIDEBAR)
                .inner_margin(Margin {
                    left: 10.0,
                    right: 10.0,
                    top: 18.0,
                    bottom: 10.0,
                }),
        )
        .show(ctx, |ui| {
            ui.add_space(2.0);
            ui.label(RichText::new("ASUS").size(25.0).strong().color(th::ACCENT));
            ui.label(RichText::new("DisplayWidget Center").size(11.0).color(th::TEXT));
            ui.label(RichText::new("RUST EDITION").size(8.0).strong().color(th::MUTED));
            ui.add_space(14.0);
            ui.label(RichText::new(lang.t("显示器", "MONITOR")).size(8.0).strong().color(th::MUTED));

            let selected_name = view
                .as_ref()
                .map(|v| v.short_name())
                .unwrap_or_else(|| lang.t("正在扫描…", "Scanning...").to_string());
            egui::ComboBox::from_id_salt("monitor_select")
                .width(198.0)
                .selected_text(RichText::new(selected_name).size(11.0))
                .show_ui(ui, |ui| {
                    if snapshot.monitors.is_empty() {
                        ui.label(RichText::new(lang.t("未发现显示器", "No monitors")).size(11.0));
                    }
                    for (index, monitor) in snapshot.monitors.iter().enumerate() {
                        let label = if monitor.ddc_ok {
                            monitor.short_name()
                        } else {
                            format!("{} · {}", monitor.short_name(), lang.t("无 DDC", "no DDC"))
                        };
                        if ui
                            .selectable_label(snapshot.selected == index, label)
                            .clicked()
                            && snapshot.selected != index
                        {
                            if let Some(monitor) = snapshot.monitors.get(index) {
                                settings.monitor_key = monitor.key.clone();
                                dirty_settings = true;
                            }
                            shared.worker.send(Job::Select(index));
                        }
                    }
                });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let ok = view.as_ref().map(|v| v.ddc_ok).unwrap_or(false);
                th::dot(ui, if ok { th::GREEN } else { th::RED });
                ui.label(
                    RichText::new(if ok {
                        lang.t("DDC/CI 已连接", "DDC/CI connected")
                    } else if snapshot.scanning {
                        lang.t("正在扫描…", "Scanning...")
                    } else {
                        lang.t("DDC/CI 无响应", "DDC/CI unavailable")
                    })
                    .size(10.0)
                    .color(if ok { th::GREEN } else { th::RED }),
                );
            });

            ui.add_space(12.0);
            let nav_height = (ui.available_height() - 76.0).max(90.0);
            ScrollArea::vertical()
                .id_salt("nav_scroll")
                .auto_shrink([false, false])
                .max_height(nav_height)
                .show(ui, |ui| {
                    for page in Page::ALL {
                        if th::nav_item(ui, page.title(lang), prefs.page == page) {
                            prefs.page = page;
                            settings.last_page = page.key().to_string();
                            dirty_settings = true;
                        }
                    }
                });

            ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                ui.label(
                    RichText::new(format!("v{} · rust", store::app_version()))
                        .size(9.0)
                        .color(th::MUTED),
                );
                ui.add_space(6.0);
                if th::pill(ui, lang.t("重新扫描", "Rescan"), false, true) {
                    shared.worker.send(Job::Scan { force: true });
                }
            });
        });

    // ---------------- 顶栏 ----------------
    egui::TopBottomPanel::top("main_header")
        .exact_height(82.0)
        .frame(
            Frame::none()
                .fill(th::BG)
                .inner_margin(Margin {
                    left: 26.0,
                    right: 24.0,
                    top: 14.0,
                    bottom: 6.0,
                }),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(prefs.page.title(lang))
                            .size(21.0)
                            .strong()
                            .color(th::TEXT),
                    );
                    ui.label(
                        RichText::new(prefs.page.subtitle(lang))
                            .size(10.0)
                            .color(th::MUTED),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let refresh = egui::Button::new(RichText::new("↻").size(15.0))
                        .min_size(Vec2::new(34.0, 30.0))
                        .fill(th::PANEL)
                        .stroke(Stroke::new(1.0, th::BORDER));
                    if ui.add(refresh).clicked() {
                        shared.worker.send(Job::RefreshAll);
                    }
                    if let Some(view) = &view {
                        Frame::none()
                            .fill(th::PANEL)
                            .stroke(Stroke::new(1.0, th::BORDER))
                            .rounding(egui::Rounding::same(8.0))
                            .inner_margin(Margin {
                                left: 10.0,
                                right: 10.0,
                                top: 5.0,
                                bottom: 5.0,
                            })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    th::dot(
                                        ui,
                                        if view.ddc_ok { th::GREEN } else { th::RED },
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{}  ·  {}",
                                            view.short_name(),
                                            view.resolution
                                        ))
                                        .size(10.0)
                                        .color(th::TEXT_DIM),
                                    );
                                });
                            });
                    }
                });
            });
        });

    // ---------------- 内容 ----------------
    egui::CentralPanel::default()
        .frame(
            Frame::none()
                .fill(th::BG)
                .inner_margin(Margin {
                    left: 26.0,
                    right: 20.0,
                    top: 4.0,
                    bottom: 16.0,
                }),
        )
        .show(ctx, |ui| {
            let Some(view) = view.clone() else {
                empty_state(ui, lang, &snapshot, &shared.worker);
                return;
            };
            if !view.ddc_ok {
                empty_state(ui, lang, &snapshot, &shared.worker);
                return;
            }
            if snapshot.boost_one_k {
                Frame::none()
                    .fill(Color32::from_rgb(0x2A, 0x1C, 0x0A))
                    .stroke(Stroke::new(1.0, th::AMBER))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(lang.t(
                                "1K FPS 模式：面板在该模式下不响应 DDC 读取，画面/色彩等参数暂不可调；切回 4K 即可恢复。",
                                "1K FPS mode: the panel does not answer DDC reads, so picture and color controls are unavailable. Switch back to 4K to restore them.",
                            ))
                            .size(10.0)
                            .color(th::AMBER),
                        );
                    });
                ui.add_space(8.0);
            }
            let mut page_state = PageState {
                view: &view,
                lang,
                prefs: &mut prefs,
                settings: &mut settings,
                profiles: &mut profiles,
                hotkeys: &mut hotkeys,
                worker: &shared.worker,
                dirty_settings: &mut dirty_settings,
                dirty_profiles: &mut dirty_profiles,
                boost_known: snapshot.boost_known,
                boost_one_k: snapshot.boost_one_k,
                boost_switching: snapshot.boost_switching,
                ctx,
            };
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    match page_state.prefs.page {
                        Page::Overview => page_overview(ui, &mut page_state),
                        Page::Display => {
                            let enabled = !page_state.boost_one_k;
                            ui.add_enabled_ui(enabled, |ui| page_display(ui, &mut page_state));
                        }
                        Page::EyeCare => {
                            let enabled = !page_state.boost_one_k;
                            ui.add_enabled_ui(enabled, |ui| page_eyecare(ui, &mut page_state));
                        }
                        Page::GamePlus => {
                            let enabled = !page_state.boost_one_k;
                            ui.add_enabled_ui(enabled, |ui| page_gameplus(ui, &mut page_state));
                        }
                        Page::Profiles => page_profiles(ui, &mut page_state),
                        Page::Widget => page_widget(ui, &mut page_state),
                        Page::Settings => page_settings(ui, &mut page_state),
                    }
                    ui.add_space(24.0);
                });
            draw_dialogs(ctx, &mut page_state);
            draw_vcp_table(ctx, &mut page_state);
        });

    if dirty_settings {
        store::save_settings(&settings);
    }
    if dirty_profiles {
        store::save_profiles(&profiles);
    }
}

fn empty_state(ui: &mut Ui, lang: Lang, snapshot: &UiState, worker: &WorkerHandle) {
    ui.vertical_centered(|ui| {
        ui.add_space(120.0);
        if snapshot.scanning {
            ui.spinner();
            ui.add_space(10.0);
            ui.label(
                RichText::new(lang.t("正在读取显示器信息…", "Reading monitor information..."))
                    .size(14.0)
                    .color(th::TEXT),
            );
            ui.label(
                RichText::new(lang.t("DDC/CI 通常需要 1–2 秒", "DDC/CI usually takes 1-2 seconds"))
                    .size(10.0)
                    .color(th::MUTED),
            );
        } else {
            ui.label(RichText::new("⚠").size(30.0).color(th::AMBER));
            ui.label(
                RichText::new(if snapshot.monitors.is_empty() {
                    lang.t("未发现显示器", "No monitors found")
                } else {
                    lang.t("此显示器不支持 DDC/CI", "This monitor does not expose DDC/CI")
                })
                .size(14.0)
                .strong()
                .color(th::TEXT),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(lang.t(
                    "请检查显示器连接，或重新扫描。",
                    "Check the monitor connection and try again.",
                ))
                .size(10.0)
                .color(th::MUTED),
            );
            if th::pill(ui, lang.t("重新扫描", "Rescan"), true, true) {
                worker.send(Job::Scan { force: true });
            }
        }
    });
}

fn save_geometry(ctx: &Context, shared: &MainShared) {
    let (outer_now, inner_now, maximized) = ctx.input(|i| {
        let v = i.viewport();
        (v.outer_rect, v.inner_rect, v.maximized.unwrap_or(false))
    });
    let mut prefs = shared.ui.lock().unwrap_or_else(|e| e.into_inner());
    let outer = outer_now.or(prefs.last_outer);
    let inner = inner_now.map(|r| r.size()).or(prefs.last_inner);
    drop(prefs);

    if let Ok(mut settings) = shared.settings.lock() {
        if let Some(outer) = outer {
            settings.main_pos = Some([outer.min.x, outer.min.y]);
        }
        if let Some(inner) = inner {
            settings.main_size = Some([inner.x, inner.y]);
        }
        settings.main_maximized = maximized;
        store::save_settings(&settings);
    }
}

// ---------------------------------------------------------------------------
// 页面状态与辅助
// ---------------------------------------------------------------------------
struct PageState<'a> {
    view: &'a MonitorView,
    lang: Lang,
    prefs: &'a mut MainUi,
    settings: &'a mut AppSettings,
    profiles: &'a mut Vec<Profile>,
    hotkeys: &'a mut HotkeyRuntime,
    worker: &'a WorkerHandle,
    dirty_settings: &'a mut bool,
    dirty_profiles: &'a mut bool,
    boost_known: bool,
    boost_one_k: bool,
    boost_switching: bool,
    ctx: &'a Context,
}

impl PageState<'_> {
    fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        self.lang.t(zh, en)
    }

    fn value(&self, code: u8) -> Option<u32> {
        self.view.value(code)
    }

    fn max(&self, code: u8) -> u32 {
        self.view.maximum(code).max(1)
    }

    fn supported(&self, code: u8) -> bool {
        self.view.supported(code)
    }

    /// 滑块写：拖动时限流，松手时强制发送最终值。
    fn write(&mut self, code: u8, value: u32, label: String, force: bool) {
        let now = Instant::now();
        let due = force
            || self
                .prefs
                .throttle
                .get(&code)
                .map_or(true, |t| now.duration_since(*t) >= Duration::from_millis(140));
        if due {
            self.prefs.throttle.insert(code, now);
            self.prefs.record_write(code, value);
            self.worker.send(Job::Write {
                code,
                value,
                label,
            });
        } else {
            self.prefs.record_write(code, value);
        }
    }

    fn slider(&mut self, ui: &mut Ui, code: u8, zh: &'static str, en: &'static str, suffix: &str) {
        let label = self.t(zh, en);
        let maximum = self.max(code);
        let mut value = self.prefs.display_value(code, self.value(code).unwrap_or(0));
        let (changed, released) =
            th::slider_row(ui, label, &mut value, maximum, suffix, self.supported(code));
        if changed || released {
            self.write(code, value, label.to_string(), released);
        }
    }
}

// ---------------------------------------------------------------------------
// 概览
// ---------------------------------------------------------------------------
fn page_overview(ui: &mut Ui, st: &mut PageState) {
    let mode = st
        .view
        .current_mode()
        .map(|m| m.label(st.lang).to_string())
        .unwrap_or_else(|| "—".into());
    let input = st.view.input_name();
    let temp = st.view.color_temp_name(st.lang);
    let firmware = st.view.firmware();

    ui.columns(4, |cols| {
        th::stat_card(&mut cols[0], st.t("当前模式", "Current mode"), &mode, "GameVisual");
        th::stat_card(&mut cols[1], st.t("输入源", "Input"), &input, "MCCS 0x60");
        th::stat_card(&mut cols[2], st.t("色温", "Color temp"), &temp, "MCCS 0x14");
        th::stat_card(&mut cols[3], st.t("固件", "Firmware"), &firmware, "MCCS 0xFF");
    });

    ui.add_space(12.0);
    ui.columns(2, |cols| {
        th::card(
            &mut cols[0],
            Some((
                st.t("常用调节", "Quick adjustments"),
                Some(st.t("实时写入显示器", "Written to the monitor live")),
            )),
            |ui| {
                st.slider(ui, vcp::VCP_BRIGHTNESS, "亮度", "Brightness", "%");
                st.slider(ui, vcp::VCP_CONTRAST, "对比度", "Contrast", "%");
                st.slider(ui, vcp::VCP_VOLUME, "音量", "Volume", "%");
            },
        );

        th::card(
            &mut cols[1],
            Some((st.t("快速场景", "Quick scenes"), Some(st.t("一键组合", "One click")))),
            |ui| {
                for (key, zh, en, dzh, den) in [
                    ("movie", "观影", "Movie", "影院模式 + 暖色", "Cinema + warm"),
                    ("game", "游戏", "Gaming", "FPS 模式 + 响应", "FPS mode + overdrive"),
                    ("reading", "阅读", "Reading", "滤蓝光 + 阅读", "Blue light + reading"),
                    ("night", "夜间", "Night", "低蓝光 + 夜间", "Low blue light + night"),
                ] {
                    ui.horizontal(|ui| {
                        if th::pill(ui, st.t(zh, en), false, true) {
                            apply_scene(st, key);
                        }
                        ui.label(RichText::new(st.t(dzh, den)).size(10.0).color(th::MUTED));
                    });
                }
            },
        );
    });

    ui.add_space(12.0);
    th::card(
        ui,
        Some((
            "Frame Rate Boost",
            Some(st.t(
                "4K 画质 ↔ 1K 高刷 · MCCS 0xFD",
                "4K quality ↔ 1K high refresh · MCCS 0xFD",
            )),
        )),
        |ui| {
            let enabled = st.boost_known && !st.boost_switching;
            ui.columns(2, |cols| {
                if th::mode_card(
                    &mut cols[0],
                    "4K",
                    st.t("3840 × 2160 · 162 Hz", "3840 × 2160 · 162 Hz"),
                    !st.boost_one_k,
                    enabled,
                ) {
                    st.worker.send(Job::SetBoost { one_k: false });
                }
                if th::mode_card(
                    &mut cols[1],
                    "1K FPS",
                    st.t("1920 × 1080 · 485 Hz", "1920 × 1080 · 485 Hz"),
                    st.boost_one_k,
                    enabled,
                ) {
                    st.worker.send(Job::SetBoost { one_k: true });
                }
            });
            ui.add_space(6.0);
            if st.boost_switching {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(st.t(
                            "正在切换，显示器会短暂黑屏…",
                            "Switching; the screen blanks briefly...",
                        ))
                        .size(10.0)
                        .color(th::AMBER),
                    );
                });
            } else if !st.boost_known {
                ui.label(
                    RichText::new(st.t(
                        "未检测到双模显示器。",
                        "No dual-mode panel detected.",
                    ))
                    .size(10.0)
                    .color(th::MUTED),
                );
            } else {
                ui.label(
                    RichText::new(st.t(
                        "4K 优先画质；1K FPS 提升刷新率。切换后显示器重新握手，约 3–5 秒。",
                        "4K favours detail; 1K FPS boosts refresh. The link re-handshakes for about 3-5 seconds.",
                    ))
                    .size(9.0)
                    .color(th::MUTED),
                );
            }
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("显示器信息", "Monitor information"), None)),
        |ui| {
            egui::Grid::new("monitor_info_grid")
                .num_columns(4)
                .spacing([28.0, 10.0])
                .show(ui, |ui| {
                    info_cell(ui, st.t("型号", "Model"), &st.view.model);
                    info_cell(
                        ui,
                        st.t("设备", "Device"),
                        &st.view.device_short(),
                    );
                    ui.end_row();
                    info_cell(ui, st.t("分辨率", "Resolution"), &st.view.resolution);
                    info_cell(
                        ui,
                        st.t("主显示器", "Primary"),
                        if st.view.primary { st.t("是", "Yes") } else { st.t("否", "No") },
                    );
                    ui.end_row();
                    info_cell(ui, st.t("VCP 版本", "VCP version"), &st.view.vcp_version());
                    info_cell(ui, st.t("固件", "Firmware"), &st.view.firmware());
                    ui.end_row();
                    info_cell(
                        ui,
                        st.t("支持命令", "Commands"),
                        &format!("{} VCP", st.view.caps.vcp_count()),
                    );
                    info_cell(
                        ui,
                        st.t("DDC/CI", "DDC/CI"),
                        if st.view.ddc_ok { st.t("正常", "OK") } else { st.t("无响应", "No response") },
                    );
                    ui.end_row();
                });
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("输入源", "Input source"), Some("MCCS 0x60"))),
        |ui| {
            let options = st.view.input_options();
            let current = st.value(vcp::VCP_INPUT);
            if let Some(value) =
                th::segmented(ui, &options, current, st.supported(vcp::VCP_INPUT))
            {
                st.write(
                    vcp::VCP_INPUT,
                    value,
                    st.t("输入源", "Input source").to_string(),
                    true,
                );
            }
            ui.label(
                RichText::new(st.t(
                    "切换输入源会让画面短暂黑屏，这是正常现象。",
                    "Switching input blanks the screen briefly; this is normal.",
                ))
                .size(9.0)
                .color(th::MUTED),
            );
        },
    );
}

fn info_cell(ui: &mut Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.set_min_width(150.0);
        ui.add(
            egui::Label::new(RichText::new(label).size(9.0).color(th::MUTED))
                .wrap_mode(egui::TextWrapMode::Extend),
        );
        ui.add(
            egui::Label::new(RichText::new(value).size(12.0).color(th::TEXT))
                .wrap_mode(egui::TextWrapMode::Extend),
        );
    });
}

fn apply_scene(st: &mut PageState, key: &str) {
    st.worker.send(Job::WriteMany {
        pairs: worker::scene_pairs(st.view, key),
        label: st.t("应用场景", "Apply scene").to_string(),
    });
}

// ---------------------------------------------------------------------------
// 显示与色彩
// ---------------------------------------------------------------------------
fn page_display(ui: &mut Ui, st: &mut PageState) {
    th::section_header(
        ui,
        st.t("GameVisual 模式", "GameVisual modes"),
        st.t("写入 MCCS 0xDC / 0xE3", "Written through MCCS 0xDC / 0xE3"),
        Some(st.t("点击卡片切换", "Click a card to switch")),
    );
    ui.add_space(8.0);

    let current = st.view.current_mode_value();
    let modes = vcp::mode_set(st.view.line);
    for chunk in modes.chunks(4) {
        ui.columns(4, |cols| {
            for (index, mode) in chunk.iter().enumerate() {
                let active = current == Some(mode.value);
                if th::mode_card(
                    &mut cols[index],
                    mode.label(st.lang),
                    mode.en,
                    active,
                    st.view.mode_supported(mode.value),
                ) {
                    st.worker.send(Job::SetMode {
                        value: mode.value,
                        label: mode.label(st.lang).to_string(),
                    });
                }
            }
        });
        ui.add_space(6.0);
    }

    ui.add_space(8.0);
    ui.columns(2, |cols| {
        th::card(
            &mut cols[0],
            Some((st.t("画面", "Picture"), None)),
            |ui| {
                st.slider(ui, vcp::VCP_BRIGHTNESS, "亮度", "Brightness", "%");
                st.slider(ui, vcp::VCP_CONTRAST, "对比度", "Contrast", "%");
                st.slider(ui, vcp::VCP_SHARPNESS, "锐度", "Sharpness", "%");
                st.slider(ui, vcp::VCP_SATURATION, "饱和度", "Saturation", "%");
                st.slider(ui, vcp::VCP_HUE, "色调", "Hue", "%");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(st.t("伽马", "Gamma")).size(12.0).color(th::TEXT_DIM));
                    let options = st.view.gamma_options();
                    let current_index = vcp::gamma_index(st.value(vcp::VCP_GAMMA).unwrap_or(0));
                    let current_value =
                        current_index.map(|i| vcp::GAMMA_STEPS[i].0).unwrap_or(0);
                    let selected = options
                        .iter()
                        .find(|(v, _)| *v == current_value)
                        .map(|(_, label)| label.clone())
                        .unwrap_or_else(|| "—".into());
                    egui::ComboBox::from_id_salt("gamma_combo")
                        .selected_text(RichText::new(selected).size(11.0))
                        .show_ui(ui, |ui| {
                            for (value, label) in options {
                                if ui
                                    .selectable_label(current_value == value, label.clone())
                                    .clicked()
                                {
                                    st.write(
                                        vcp::VCP_GAMMA,
                                        value,
                                        st.t("伽马", "Gamma").to_string(),
                                        true,
                                    );
                                }
                            }
                        });
                });
                ui.label(
                    RichText::new(st.t(
                        "ASUS 16 位伽马值（0x5000 – 0xA000）",
                        "ASUS 16-bit gamma values (0x5000 - 0xA000)",
                    ))
                    .size(9.0)
                    .color(th::MUTED),
                );
            },
        );

        th::card(
            &mut cols[1],
            Some((st.t("色彩", "Color"), None)),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(st.t("色温", "Color temperature"))
                            .size(12.0)
                            .color(th::TEXT_DIM),
                    );
                    let options = st.view.temp_options(st.lang);
                    let current_temp = st.value(vcp::VCP_COLOR_TEMP).unwrap_or(0);
                    let selected = options
                        .iter()
                        .find(|(v, _)| *v == current_temp)
                        .map(|(_, label)| label.clone())
                        .unwrap_or_else(|| "—".into());
                    egui::ComboBox::from_id_salt("temp_combo")
                        .selected_text(RichText::new(selected).size(11.0))
                        .show_ui(ui, |ui| {
                            for (value, label) in options {
                                if ui
                                    .selectable_label(current_temp == value, label.clone())
                                    .clicked()
                                {
                                    st.write(
                                        vcp::VCP_COLOR_TEMP,
                                        value,
                                        st.t("色温", "Color temperature").to_string(),
                                        true,
                                    );
                                }
                            }
                        });
                });
                let user_temp = st.value(vcp::VCP_COLOR_TEMP) == Some(11);
                ui.add_enabled_ui(user_temp, |ui| {
                    st.slider(ui, vcp::VCP_RED_GAIN, "红色增益", "Red gain", "%");
                    st.slider(ui, vcp::VCP_GREEN_GAIN, "绿色增益", "Green gain", "%");
                    st.slider(ui, vcp::VCP_BLUE_GAIN, "蓝色增益", "Blue gain", "%");
                });
                ui.label(
                    RichText::new(st.t(
                        "仅在色温为“用户”时可调",
                        "Editable when color temperature is User",
                    ))
                    .size(9.0)
                    .color(th::MUTED),
                );
            },
        );
    });

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("画面增强", "Enhancement"), None)),
        |ui| {
            ui.horizontal(|ui| {
                let mut overdrive = st
                    .prefs
                    .display_value(vcp::VCP_OVERDRIVE, st.value(vcp::VCP_OVERDRIVE).unwrap_or(0));
                let (changed, released) = th::slider_row(
                    ui,
                    st.t("响应速度 (OverDrive)", "OverDrive"),
                    &mut overdrive,
                    st.max(vcp::VCP_OVERDRIVE),
                    "",
                    st.supported(vcp::VCP_OVERDRIVE),
                );
                if changed || released {
                    st.write(
                        vcp::VCP_OVERDRIVE,
                        overdrive,
                        st.t("响应速度", "OverDrive").to_string(),
                        released,
                    );
                }
                ui.add_space(16.0);
                ui.label(
                    RichText::new(st.t("暗部增强", "Shadow Boost"))
                        .size(12.0)
                        .color(th::TEXT_DIM),
                );
                let options: Vec<(u32, String)> = vcp::SHADOW_BOOST_VALUES
                    .iter()
                    .map(|(v, zh, en)| (*v, st.t(zh, en).to_string()))
                    .collect();
                if let Some(value) = th::segmented(
                    ui,
                    &options,
                    st.value(vcp::VCP_SHADOW_BOOST),
                    st.supported(vcp::VCP_SHADOW_BOOST),
                ) {
                    st.write(
                        vcp::VCP_SHADOW_BOOST,
                        value,
                        st.t("暗部增强", "Shadow Boost").to_string(),
                        true,
                    );
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let new_vcp = st.view.is_new_vcp();
                let mut ascr = if new_vcp {
                    st.view.is_on(vcp::VCP_TOGGLES2, vcp::TOGGLE_ASCR)
                } else {
                    st.value(vcp::VCP_POWER_SAVING).unwrap_or(0) > 0
                };
                let ascr_enabled = if new_vcp {
                    st.supported(vcp::VCP_TOGGLES2)
                } else {
                    st.supported(vcp::VCP_POWER_SAVING)
                };
                if th::toggle_row(
                    ui,
                    st.t("ASCR 动态对比度", "ASCR dynamic contrast"),
                    &mut ascr,
                    Some(st.t(
                        "新 VCP：0xFD bit 1；旧 VCP：0xE1",
                        "New VCP: 0xFD bit 1; legacy: 0xE1",
                    )),
                ) && ascr_enabled
                {
                    if new_vcp {
                        st.worker.send(Job::ToggleBit {
                            code: vcp::VCP_TOGGLES2,
                            bit: vcp::TOGGLE_ASCR,
                            on: ascr,
                            label: "ASCR".into(),
                        });
                    } else {
                        st.worker.send(Job::Write {
                            code: vcp::VCP_POWER_SAVING,
                            value: if ascr { 1 } else { 0 },
                            label: "ASCR".into(),
                        });
                    }
                }

                let power_code = if new_vcp {
                    vcp::VCP_POWER_SAVING
                } else {
                    vcp::VCP_POWER_SAVING_LEGACY
                };
                let mut power = st.value(power_code).unwrap_or(0) > 0;
                if th::toggle_row(
                    ui,
                    st.t("省电模式", "Power saving"),
                    &mut power,
                    None,
                ) && st.supported(power_code)
                {
                    st.worker.send(Job::Write {
                        code: power_code,
                        value: if power { 1 } else { 0 },
                        label: st.t("省电模式", "Power saving").to_string(),
                    });
                }
            });
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("输入与音频", "Input & audio"), None)),
        |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(st.t("输入源", "Input source"))
                        .size(12.0)
                        .color(th::TEXT_DIM),
                );
                let options = st.view.input_options();
                let current_input = st.value(vcp::VCP_INPUT).unwrap_or(0);
                let selected = options
                    .iter()
                    .find(|(v, _)| *v == current_input)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_else(|| "—".into());
                egui::ComboBox::from_id_salt("input_combo")
                    .selected_text(RichText::new(selected).size(11.0))
                    .show_ui(ui, |ui| {
                        for (value, label) in options {
                            if ui
                                .selectable_label(current_input == value, label.clone())
                                .clicked()
                            {
                                st.write(
                                    vcp::VCP_INPUT,
                                    value,
                                    st.t("输入源", "Input source").to_string(),
                                    true,
                                );
                            }
                        }
                    });
                ui.add_space(20.0);
                let mut volume = st
                    .prefs
                    .display_value(vcp::VCP_VOLUME, st.value(vcp::VCP_VOLUME).unwrap_or(0));
                let (changed, released) = th::slider_row(
                    ui,
                    st.t("音量", "Volume"),
                    &mut volume,
                    st.max(vcp::VCP_VOLUME),
                    "%",
                    st.supported(vcp::VCP_VOLUME),
                );
                if changed || released {
                    st.write(
                        vcp::VCP_VOLUME,
                        volume,
                        st.t("音量", "Volume").to_string(),
                        released,
                    );
                }
            });
            ui.add_space(6.0);
            let mut mute = st.view.is_on(vcp::VCP_TOGGLES1, vcp::MUTE_BIT);
            if th::toggle_row(
                ui,
                st.t("静音", "Mute"),
                &mut mute,
                Some(st.t("MCCS 0xFC 的 bit 3", "Bit 3 of MCCS 0xFC")),
            ) && st.supported(vcp::VCP_TOGGLES1)
            {
                st.worker.send(Job::ToggleBit {
                    code: vcp::VCP_TOGGLES1,
                    bit: vcp::MUTE_BIT,
                    on: mute,
                    label: st.t("静音", "Mute").to_string(),
                });
            }
        },
    );
}

// ---------------------------------------------------------------------------
// 护眼
// ---------------------------------------------------------------------------
fn page_eyecare(ui: &mut Ui, st: &mut PageState) {
    th::card(
        ui,
        Some((st.t("滤蓝光", "Blue light filter"), Some("MCCS 0xE6"))),
        |ui| {
            let value = st.value(vcp::VCP_BLUE_LIGHT).unwrap_or(0);
            let maximum = st.max(vcp::VCP_BLUE_LIGHT).clamp(1, 10);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{value}")).size(42.0).strong().color(th::ACCENT));
                ui.label(
                    RichText::new(format!("/ {maximum}"))
                        .size(13.0)
                        .color(th::MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(st.t(
                            "级别越高，蓝光越少，画面越暖。",
                            "Higher levels remove more blue light and warm the picture.",
                        ))
                        .size(9.0)
                        .color(th::MUTED),
                    );
                });
            });
            let mut level = st
                .prefs
                .display_value(vcp::VCP_BLUE_LIGHT, value);
            let (changed, released) = th::slider_row(
                ui,
                st.t("级别", "Level"),
                &mut level,
                maximum,
                "",
                st.supported(vcp::VCP_BLUE_LIGHT),
            );
            if changed || released {
                st.write(
                    vcp::VCP_BLUE_LIGHT,
                    level,
                    st.t("滤蓝光", "Blue light").to_string(),
                    released,
                );
            }
            let presets: Vec<(u32, String)> = [
                (0, st.t("关闭", "Off")),
                (1, st.t("低", "Low")),
                (2, st.t("中", "Mid")),
                (3, st.t("高", "High")),
                (4, st.t("最高", "Max")),
            ]
            .into_iter()
            .map(|(v, label)| (v, label.to_string()))
            .collect();
            if let Some(target) =
                th::segmented(ui, &presets, Some(value), st.supported(vcp::VCP_BLUE_LIGHT))
            {
                st.write(
                    vcp::VCP_BLUE_LIGHT,
                    target,
                    st.t("滤蓝光", "Blue light").to_string(),
                    true,
                );
            }
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((
            st.t("色彩增强", "Color augmentation"),
            Some(st.t("六轴饱和度 · MCCS 0x59 – 0x5E", "Six-axis saturation · MCCS 0x59 - 0x5E")),
        )),
        |ui| {
            st.slider(ui, vcp::VCP_RED_SAT, "红色", "Red", "%");
            st.slider(ui, vcp::VCP_YELLOW_SAT, "黄色", "Yellow", "%");
            st.slider(ui, vcp::VCP_GREEN_SAT, "绿色", "Green", "%");
            st.slider(ui, vcp::VCP_BLUE_SAT, "蓝色", "Blue", "%");
            ui.label(
                RichText::new(st.t(
                    "青色 / 洋红通道在部分机型上不开放，未在界面中提供。",
                    "Cyan and magenta channels are not exposed by every panel.",
                ))
                .size(9.0)
                .color(th::MUTED),
            );
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("休息提醒", "Rest reminder"), Some("MCCS 0xED"))),
        |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(st.t("提醒间隔", "Reminder interval"))
                        .size(12.0)
                        .color(th::TEXT_DIM),
                );
                let current = st.value(vcp::VCP_REST_REMINDER).unwrap_or(0);
                let mut options: Vec<(u32, String)> =
                    vec![(0, st.t("关闭", "Off").to_string())];
                for index in 1..=12u32 {
                    let minutes = index * 5;
                    options.push((
                        index,
                        format!("{minutes} {}", st.t("分钟", "min")),
                    ));
                }
                let selected = options
                    .iter()
                    .find(|(v, _)| *v == current)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_default();
                let supported = st.supported(vcp::VCP_REST_REMINDER)
                    && st.view.maximum(vcp::VCP_REST_REMINDER) > 0;
                ui.add_enabled_ui(supported, |ui| {
                    egui::ComboBox::from_id_salt("rest_combo")
                        .selected_text(RichText::new(selected).size(11.0))
                        .show_ui(ui, |ui| {
                            for (value, label) in options {
                                if ui
                                    .selectable_label(current == value, label.clone())
                                    .clicked()
                                {
                                    st.write(
                                        vcp::VCP_REST_REMINDER,
                                        value,
                                        st.t("休息提醒", "Rest reminder").to_string(),
                                        true,
                                    );
                                }
                            }
                        });
                });
            });
            ui.label(
                RichText::new(st.t(
                    "由显示器固件计时并弹出提醒，不依赖电脑运行。",
                    "Timed by the monitor firmware, independent of the PC.",
                ))
                .size(9.0)
                .color(th::MUTED),
            );
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("使用建议", "Tips"), None)),
        |ui| {
            ui.label(
                RichText::new(st.t(
                    "滤蓝光会改变白点，色彩工作请关闭。若显示器处于 HDR 模式或 sRGB 模式，\
                     部分厂商选项可能被固件锁定，属正常现象。",
                    "Blue light filtering shifts the white point - turn it off for color work. \
                     Some vendor options are locked by firmware in HDR or sRGB mode.",
                ))
                .size(10.0)
                .color(th::TEXT_DIM),
            );
        },
    );
}

// ---------------------------------------------------------------------------
// 游戏辅助
// ---------------------------------------------------------------------------
fn page_gameplus(ui: &mut Ui, st: &mut PageState) {
    th::card(
        ui,
        Some((st.t("准星", "Crosshair"), Some("MCCS 0xE3"))),
        |ui| {
            if st.view.line == ProductLine::ProArt {
                ui.label(
                    RichText::new(st.t(
                        "ProArt 机型没有游戏准星功能（0xE3 用于专业显示模式）。",
                        "ProArt panels have no crosshair (0xE3 selects pro display modes).",
                    ))
                    .size(10.0)
                    .color(th::MUTED),
                );
                return;
            }
            let styles = vcp::crosshair_styles(st.view.caps_values(vcp::VCP_CROSSHAIR));
            let current = st.value(vcp::VCP_CROSSHAIR).unwrap_or(0);
            ui.horizontal_wrapped(|ui| {
                for value in styles {
                    let label = vcp::crosshair_name(value, st.lang);
                    if th::crosshair_card(ui, value, label, current == value, true) {
                        st.write(
                            vcp::VCP_CROSSHAIR,
                            value,
                            st.t("准星", "Crosshair").to_string(),
                            true,
                        );
                    }
                }
            });
        },
    );

    ui.add_space(12.0);
    ui.columns(2, |cols| {
        th::card(
            &mut cols[0],
            Some((st.t("计时器", "Timer"), Some("MCCS 0xE4"))),
            |ui| {
                let options: Vec<(u32, String)> = (0..=5u32)
                    .map(|v| (v, vcp::timer_label(v, st.lang).to_string()))
                    .collect();
                if let Some(value) = th::segmented(
                    ui,
                    &options,
                    st.value(vcp::VCP_GAMEPLUS_TIMER),
                    st.supported(vcp::VCP_GAMEPLUS_TIMER),
                ) {
                    st.write(
                        vcp::VCP_GAMEPLUS_TIMER,
                        value,
                        st.t("计时器", "Timer").to_string(),
                        true,
                    );
                }
            },
        );
        th::card(
            &mut cols[1],
            Some((st.t("FPS 计数器", "FPS counter"), Some("MCCS 0xEA"))),
            |ui| {
                let options: Vec<(u32, String)> = vcp::FPS_VALUES
                    .iter()
                    .map(|(v, zh, en)| (*v, st.t(zh, en).to_string()))
                    .collect();
                if let Some(value) = th::segmented(
                    ui,
                    &options,
                    st.value(vcp::VCP_FPS_COUNTER),
                    st.supported(vcp::VCP_FPS_COUNTER),
                ) {
                    st.write(
                        vcp::VCP_FPS_COUNTER,
                        value,
                        st.t("FPS 计数器", "FPS counter").to_string(),
                        true,
                    );
                }
            },
        );
    });

    ui.add_space(12.0);
    th::card(
        ui,
        Some((
            st.t("显示对齐", "Display alignment"),
            Some("MCCS 0xE7 / 0xE8"),
        )),
        |ui| {
            let mut alignment = st.value(vcp::VCP_ALIGNMENT).unwrap_or(0) > 0;
            if th::toggle_row(
                ui,
                st.t("对齐辅助线", "Alignment grid"),
                &mut alignment,
                None,
            ) && st.supported(vcp::VCP_ALIGNMENT)
            {
                st.write(
                    vcp::VCP_ALIGNMENT,
                    if alignment { 1 } else { 0 },
                    st.t("显示对齐", "Display alignment").to_string(),
                    true,
                );
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(st.t("辅助位置", "Position"))
                        .size(12.0)
                        .color(th::TEXT_DIM),
                );
                let options: Vec<(u32, String)> = vcp::POSITION_VALUES
                    .iter()
                    .map(|(v, zh, en)| (*v, st.t(zh, en).to_string()))
                    .collect();
                if let Some(value) = th::segmented(
                    ui,
                    &options,
                    st.value(vcp::VCP_GAMEPLUS_POSITION),
                    st.supported(vcp::VCP_GAMEPLUS_POSITION),
                ) {
                    st.write(
                        vcp::VCP_GAMEPLUS_POSITION,
                        value,
                        st.t("游戏辅助位置", "GamePlus position").to_string(),
                        true,
                    );
                }
            });
        },
    );
}

// ---------------------------------------------------------------------------
// 配置文件
// ---------------------------------------------------------------------------
fn page_profiles(ui: &mut Ui, st: &mut PageState) {
    ui.horizontal(|ui| {
        if th::pill(ui, st.t("保存当前设置", "Save current"), true, true) {
            st.prefs.text = format!(
                "{} {}",
                if st.view.model.is_empty() { "Display" } else { &st.view.model },
                store::timestamp()
            );
            st.prefs.dialog = Dialog::SaveProfile;
        }
        if th::pill(ui, st.t("导入", "Import"), false, true) {
            import_profile(st);
        }
        if th::pill(
            ui,
            st.t("导出所选", "Export selected"),
            false,
            st.prefs.selected_profile.is_some(),
        ) {
            export_profile(st);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(store::profiles_path().display().to_string())
                    .size(9.0)
                    .color(th::MUTED),
            );
        });
    });

    ui.add_space(12.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(320.0);
            th::card(
                ui,
                Some((st.t("已保存配置", "Saved profiles"), None)),
                |ui| {
                    ScrollArea::vertical()
                        .id_salt("profile_list")
                        .max_height(430.0)
                        .show(ui, |ui| {
                            if st.profiles.is_empty() {
                                ui.label(
                                    RichText::new(st.t(
                                        "还没有配置。保存当前设置或导入 JSON。",
                                        "No profiles yet. Save the current state or import JSON.",
                                    ))
                                    .size(10.0)
                                    .color(th::MUTED),
                                );
                            }
                            for (index, profile) in st.profiles.iter().enumerate() {
                                let selected = st.prefs.selected_profile == Some(index);
                                let button = egui::Button::new(
                                    RichText::new(&profile.name).size(12.0),
                                )
                                .selected(selected)
                                .min_size(Vec2::new(ui.available_width(), 26.0));
                                if ui.add(button).clicked() {
                                    st.prefs.selected_profile = Some(index);
                                }
                            }
                        });
                },
            );
        });
        ui.add_space(14.0);
        ui.vertical(|ui| {
            th::card(
                ui,
                Some((st.t("配置详情", "Profile details"), None)),
                |ui| {
                    let selected = st
                        .prefs
                        .selected_profile
                        .and_then(|index| st.profiles.get(index))
                        .cloned();
                    match selected {
                        None => {
                            ui.label(
                                RichText::new(st.t("未选择", "Nothing selected"))
                                    .size(15.0)
                                    .strong()
                                    .color(th::TEXT),
                            );
                            ui.label(
                                RichText::new(st.t(
                                    "保存当前设置或导入一个 JSON 配置。",
                                    "Save the current state or import a JSON profile.",
                                ))
                                .size(10.0)
                                .color(th::MUTED),
                            );
                        }
                        Some(profile) => {
                            ui.label(
                                RichText::new(&profile.name)
                                    .size(16.0)
                                    .strong()
                                    .color(th::TEXT),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}\n{}: {}\n{}: {}",
                                    st.t("型号", "Model"),
                                    profile.model,
                                    st.t("创建时间", "Created"),
                                    profile.created,
                                    st.t("参数数量", "Values"),
                                    profile.value_count(),
                                ))
                                .size(10.0)
                                .color(th::MUTED),
                            );
                            ui.add_space(10.0);
                            if th::pill(ui, st.t("应用配置", "Apply"), true, true) {
                                st.worker.send(Job::ApplyProfile {
                                    profile: profile.clone(),
                                });
                            }
                            ui.add_space(6.0);
                            if th::pill(ui, st.t("用当前状态覆盖", "Overwrite with current"), false, true)
                            {
                                let mut updated =
                                    worker::capture_profile(st.view, &profile.name);
                                updated.created = profile.created.clone();
                                if let Some(index) = st.prefs.selected_profile {
                                    if index < st.profiles.len() {
                                        st.profiles[index] = updated;
                                        *st.dirty_profiles = true;
                                    }
                                }
                            }
                            ui.add_space(6.0);
                            if th::pill(ui, st.t("删除", "Delete"), false, true) {
                                if let Some(index) = st.prefs.selected_profile {
                                    st.prefs.dialog = Dialog::DeleteProfile(index);
                                }
                            }
                        }
                    }
                },
            );
        });
    });
}

fn import_profile(st: &mut PageState) {
    let owner = unsafe { win32::find_window_by_title(MAIN_TITLE) }.unwrap_or_default();
    let Some(path) = crate::file_dialog::open_json(st.t("导入配置", "Import profile"), owner)
    else {
        return;
    };
    match store::read_profile_file(&path) {
        Ok(mut profile) => {
            if profile.name.is_empty() {
                profile.name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "profile".into());
            }
            st.profiles.push(profile);
            *st.dirty_profiles = true;
            st.prefs.selected_profile = Some(st.profiles.len() - 1);
        }
        Err(error) => {
            st.prefs.dialog = Dialog::Message(format!(
                "{}: {error}",
                st.t("导入失败", "Import failed")
            ));
        }
    }
}

fn export_profile(st: &mut PageState) {
    let Some(index) = st.prefs.selected_profile else {
        return;
    };
    let Some(profile) = st.profiles.get(index).cloned() else {
        return;
    };
    let owner = unsafe { win32::find_window_by_title(MAIN_TITLE) }.unwrap_or_default();
    let default_name = format!("{}.json", profile.name);
    let Some(path) =
        crate::file_dialog::save_json(st.t("导出配置", "Export profile"), &default_name, owner)
    else {
        return;
    };
    if let Err(error) = store::write_profile_file(&path, &profile) {
        st.prefs.dialog = Dialog::Message(format!(
            "{}: {error}",
            st.t("导出失败", "Export failed")
        ));
    }
}

// ---------------------------------------------------------------------------
// 设置
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 桌面组件
// ---------------------------------------------------------------------------
fn page_widget(ui: &mut Ui, st: &mut PageState) {
    let mut changed = false;
    th::card(
        ui,
        Some((
            st.t("显示在小挂件上的控件", "Controls shown in the desktop widget"),
            Some(st.t("修改立即生效", "Applied immediately")),
        )),
        |ui| {
            ui.label(
                RichText::new(st.t(
                    "拖动条（从上到下依次排列）",
                    "Sliders (stacked top to bottom)",
                ))
                .size(10.0)
                .color(th::MUTED),
            );
            ui.add_space(4.0);
            if th::toggle_row(
                ui,
                st.t("亮度", "Brightness"),
                &mut st.settings.widget.brightness,
                None,
            ) {
                changed = true;
            }
            if th::toggle_row(
                ui,
                st.t("对比度", "Contrast"),
                &mut st.settings.widget.contrast,
                None,
            ) {
                changed = true;
            }
            if th::toggle_row(
                ui,
                st.t("OD（响应速度）", "OD (OverDrive)"),
                &mut st.settings.widget.overdrive,
                None,
            ) {
                changed = true;
            }
            if th::toggle_row(
                ui,
                st.t("锐度", "Sharpness"),
                &mut st.settings.widget.sharpness,
                None,
            ) {
                changed = true;
            }
            if th::toggle_row(
                ui,
                st.t("色温", "Color temperature"),
                &mut st.settings.widget.color_temp,
                None,
            ) {
                changed = true;
            }
            if th::toggle_row(
                ui,
                st.t("护眼（滤蓝光）", "Eye care (blue light)"),
                &mut st.settings.widget.eye_care,
                None,
            ) {
                changed = true;
            }
            if th::toggle_row(
                ui,
                st.t(
                    "Frame Rate Boost（4K / 1K 开关）",
                    "Frame Rate Boost (4K / 1K switch)",
                ),
                &mut st.settings.widget.frame_rate_boost,
                None,
            ) {
                changed = true;
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            if th::toggle_row(
                ui,
                st.t(
                    "快速场景（按勾选顺序排列，每行最多 4 个）",
                    "Quick scenes (ordered by check time, max 4 per row)",
                ),
                &mut st.settings.widget.scenes,
                None,
            ) {
                changed = true;
            }
            if st.settings.widget.scenes {
                let all_keys: Vec<&'static str> = vcp::mode_set(st.view.line)
                    .iter()
                    .filter(|mode| st.view.mode_supported(mode.value))
                    .map(|mode| mode.key)
                    .collect();
                ui.indent("widget_scene_options", |ui| {
                    for mode in vcp::mode_set(st.view.line) {
                        if !st.view.mode_supported(mode.value) {
                            continue;
                        }
                        let label = mode.label(st.lang);
                        let mut on = st.settings.widget.scene_visible(mode.key);
                        if th::toggle_row(ui, label, &mut on, None) {
                            st.settings.widget.set_scene_visible(mode.key, on, &all_keys);
                            changed = true;
                        }
                    }
                });
                let visible = vcp::mode_set(st.view.line)
                    .iter()
                    .filter(|mode| {
                        st.view.mode_supported(mode.value)
                            && st.settings.widget.scene_visible(mode.key)
                    })
                    .count();
                if visible == 0 {
                    ui.label(
                        RichText::new(st.t(
                            "至少勾选一个场景，否则挂件底部留空。",
                            "Select at least one scene; otherwise the bottom stays empty.",
                        ))
                        .size(9.0)
                        .color(th::AMBER),
                    );
                }
            }
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("布局与操作", "Layout & behaviour"), None)),
        |ui| {
            ui.label(
                RichText::new(st.t(
                    "挂件高度会随勾选项自动调整；拖动条按上面的顺序从上往下排列，\
                     Frame Rate Boost 可选。快速场景列出所有 GameVisual 模式，按勾选顺序排列，每行最多 4 个，\
                     每个选项四周留 15px 外边距，高度为宽度的 2/3，当前模式红框。响应速度在小挂件里显示为 OD。",
                    "The widget height follows your selection. Sliders are stacked in order; \
                     Frame Rate Boost is optional. Quick scenes list every GameVisual mode in the order you check them, max 4 per row, \
                     with a 15px margin around each button (height = 2/3 of width); the current mode gets a red border. \
                     OverDrive is labelled OD in the widget.",
                ))
                .size(10.0)
                .color(th::TEXT_DIM),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if th::pill(ui, st.t("全部显示", "Show all"), false, true) {
                    st.settings.widget = store::WidgetConfig {
                        brightness: true,
                        contrast: true,
                        overdrive: true,
                        sharpness: true,
                        color_temp: true,
                        eye_care: true,
                        frame_rate_boost: true,
                        scenes: true,
                        scene_modes: None,
                    };
                    changed = true;
                }
                if th::pill(ui, st.t("恢复默认", "Reset to default"), false, true) {
                    st.settings.widget = store::WidgetConfig::default();
                    changed = true;
                }
            });
        },
    );

    if changed {
        *st.dirty_settings = true;
        // 勾选后立刻落盘，即使随后直接结束进程也不会丢失。
        store::save_settings(st.settings);
        // 主界面与桌面挂件是两个视口，必须显式唤醒根视口。
        st.ctx.request_repaint_of(egui::ViewportId::ROOT);
        st.ctx.request_repaint();
    }
}

fn page_settings(ui: &mut Ui, st: &mut PageState) {
    th::card(
        ui,
        Some((st.t("外观", "Appearance"), None)),
        |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(st.t("界面语言", "Language"))
                        .size(12.0)
                        .color(th::TEXT_DIM),
                );
                let options = vec![
                    (0u32, "简体中文".to_string()),
                    (1u32, "English".to_string()),
                ];
                let current = if st.settings.language == "en" { 1 } else { 0 };
                if let Some(value) = th::segmented(ui, &options, Some(current), true) {
                    st.settings.language = if value == 1 { "en".into() } else { "zh".into() };
                    *st.dirty_settings = true;
                    st.ctx.request_repaint();
                }
            });
            let mut topmost = st.settings.topmost;
            if th::toggle_row(ui, st.t("窗口置顶", "Always on top"), &mut topmost, None) {
                st.settings.topmost = topmost;
                *st.dirty_settings = true;
                st.ctx
                    .send_viewport_cmd(ViewportCommand::WindowLevel(if topmost {
                        WindowLevel::AlwaysOnTop
                    } else {
                        WindowLevel::Normal
                    }));
            }
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("启动", "Startup"), None)),
        |ui| {
            let mut autostart = store::is_auto_start_enabled();
            if th::toggle_row(
                ui,
                st.t("开机自动启动", "Start with Windows"),
                &mut autostart,
                Some(st.t(
                    "写入 HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                    "Writes HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                )),
            ) {
                if !store::set_auto_start_enabled(autostart) {
                    st.prefs.dialog = Dialog::Message(
                        st.t("无法修改启动项", "Could not update startup entry").to_string(),
                    );
                }
            }
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((
            st.t("全局快捷键", "Global hotkeys"),
            Some("RegisterHotKey"),
        )),
        |ui| {
            let mut enabled = st.hotkeys.enabled;
            if th::toggle_row(
                ui,
                st.t("启用全局快捷键", "Enable global hotkeys"),
                &mut enabled,
                None,
            ) {
                st.hotkeys.set_enabled(enabled, st.worker);
                st.settings.hotkeys = enabled;
                *st.dirty_settings = true;
            }
            ui.add_space(6.0);
            for binding in crate::hotkeys::BINDINGS {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(st.t(binding.zh, binding.en))
                            .size(10.0)
                            .color(th::TEXT_DIM),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(binding.keys).size(10.0).color(th::MUTED),
                        );
                    });
                });
            }
            let active = st.hotkeys.active_count();
            ui.label(
                RichText::new(if enabled {
                    format!(
                        "{} {active}/{}",
                        st.t("已注册", "Registered"),
                        crate::hotkeys::BINDINGS.len()
                    )
                } else {
                    st.t("快捷键未启用", "Hotkeys disabled").to_string()
                })
                .size(9.0)
                .color(th::MUTED),
            );
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("显示器与 DDC/CI", "Monitor & DDC/CI"), None)),
        |ui| {
            ui.label(
                RichText::new(format!(
                    "{}: {}    {}: {}    {}: {}    VCP: {}",
                    st.t("设备", "Device"),
                    st.view.device_short(),
                    st.t("型号", "Model"),
                    st.view.model,
                    st.t("命令数", "Commands"),
                    st.view.caps.vcp_count(),
                    st.view.vcp_version(),
                ))
                .size(10.0)
                .color(th::TEXT_DIM),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if th::pill(ui, st.t("重新扫描显示器", "Rescan monitors"), true, true) {
                    st.worker.send(Job::Scan { force: true });
                }
                if th::pill(ui, st.t("刷新全部参数", "Refresh all values"), false, true) {
                    st.worker.send(Job::RefreshAll);
                }
                if th::pill(ui, st.t("查看 VCP 命令表", "View VCP table"), false, true) {
                    st.prefs.vcp_table = true;
                }
            });
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("恢复出厂", "Factory reset"), Some("MCCS 0x04 / 0x08"))),
        |ui| {
            ui.label(
                RichText::new(st.t(
                    "这些命令会覆盖显示器内部的所有画面设置，请谨慎使用。",
                    "These commands overwrite every picture setting stored in the monitor.",
                ))
                .size(9.0)
                .color(th::MUTED),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if th::pill(ui, st.t("恢复全部设置", "Reset all"), false, true) {
                    st.prefs.dialog = Dialog::ResetAll;
                }
                if th::pill(ui, st.t("仅恢复颜色", "Reset color"), false, true) {
                    st.prefs.dialog = Dialog::ResetColor;
                }
            });
        },
    );

    ui.add_space(12.0);
    th::card(
        ui,
        Some((st.t("关于", "About"), None)),
        |ui| {
            ui.label(
                RichText::new(format!(
                    "ASUS DisplayWidget Center · Rust 复刻版 {}\n{}",
                    store::app_version(),
                    st.t(
                        "基于 DisplayWidgetCenter 1.5.1.3（.NET/WPF）的逆向分析实现；\
                         DDC/CI 通过 dxva2!GetVCPFeatureAndVCPFeatureReply / SetVCPFeature 访问。",
                        "Reimplemented from DisplayWidgetCenter 1.5.1.3 (.NET/WPF); \
                         DDC/CI via dxva2!GetVCPFeatureAndVCPFeatureReply / SetVCPFeature.",
                    )
                ))
                .size(10.0)
                .color(th::TEXT_DIM),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if th::pill(ui, st.t("打开数据目录", "Open data folder"), false, true) {
                    store::open_path(&store::data_dir());
                }
            });
        },
    );
}

// ---------------------------------------------------------------------------
// 对话框
// ---------------------------------------------------------------------------
fn draw_dialogs(ctx: &Context, st: &mut PageState) {
    match st.prefs.dialog.clone() {
        Dialog::None => {}
        Dialog::SaveProfile => {
            let mut open = true;
            let mut close = false;
            let mut save = false;
            egui::Window::new(st.t("保存配置", "Save profile"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(st.t("配置名称", "Profile name"));
                    ui.add(
                        egui::TextEdit::singleline(&mut st.prefs.text).desired_width(340.0),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if th::pill(ui, st.t("取消", "Cancel"), false, true) {
                            close = true;
                        }
                        if th::pill(ui, st.t("确定", "OK"), true, true) {
                            save = true;
                        }
                    });
                });
            if save {
                let name = st.prefs.text.trim().to_string();
                if !name.is_empty() {
                    let profile = worker::capture_profile(st.view, &name);
                    st.profiles.push(profile);
                    *st.dirty_profiles = true;
                    st.prefs.selected_profile = Some(st.profiles.len() - 1);
                }
                st.prefs.dialog = Dialog::None;
            } else if close || !open {
                st.prefs.dialog = Dialog::None;
            }
        }
        Dialog::DeleteProfile(index) => {
            let name = st
                .profiles
                .get(index)
                .map(|profile| profile.name.clone())
                .unwrap_or_default();
            let mut open = true;
            let mut confirm = false;
            let mut close = false;
            egui::Window::new(st.t("删除配置", "Delete profile"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{} \"{name}\"?",
                        st.t("确定删除", "Delete")
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if th::pill(ui, st.t("取消", "Cancel"), false, true) {
                            close = true;
                        }
                        if th::pill(ui, st.t("删除", "Delete"), true, true) {
                            confirm = true;
                        }
                    });
                });
            if confirm {
                if index < st.profiles.len() {
                    st.profiles.remove(index);
                    *st.dirty_profiles = true;
                    st.prefs.selected_profile = st
                        .profiles
                        .len()
                        .checked_sub(1)
                        .map(|last| st.prefs.selected_profile.unwrap_or(0).min(last));
                }
                st.prefs.dialog = Dialog::None;
            } else if close || !open {
                st.prefs.dialog = Dialog::None;
            }
        }
        Dialog::ResetAll | Dialog::ResetColor => {
            let color_only = st.prefs.dialog == Dialog::ResetColor;
            let mut open = true;
            let mut confirm = false;
            let mut close = false;
            egui::Window::new(st.t("确认恢复", "Confirm reset"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(st.t(
                        "确定要恢复出厂设置吗？",
                        "Reset the monitor to factory defaults?",
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if th::pill(ui, st.t("取消", "Cancel"), false, true) {
                            close = true;
                        }
                        if th::pill(ui, st.t("确定", "Confirm"), true, true) {
                            confirm = true;
                        }
                    });
                });
            if confirm {
                st.worker.send(Job::FactoryReset { color_only });
                st.prefs.dialog = Dialog::None;
            } else if close || !open {
                st.prefs.dialog = Dialog::None;
            }
        }
        Dialog::Message(message) => {
            let mut open = true;
            egui::Window::new(st.t("提示", "Notice"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(RichText::new(message).size(11.0));
                    ui.add_space(8.0);
                    if th::pill(ui, st.t("确定", "OK"), true, true) {
                        st.prefs.dialog = Dialog::None;
                    }
                });
            if !open {
                st.prefs.dialog = Dialog::None;
            }
        }
    }
}

fn draw_vcp_table(ctx: &Context, st: &mut PageState) {
    if !st.prefs.vcp_table {
        return;
    }
    let mut open = true;
    egui::Window::new(st.t("VCP 命令表", "VCP command table"))
        .collapsible(false)
        .resizable(true)
        .default_size([720.0, 560.0])
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!(
                    "{} · {} VCP",
                    st.view.short_name(),
                    st.view.caps.vcp_count()
                ))
                .size(12.0)
                .color(th::TEXT),
            );
            ui.add_space(6.0);
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("vcp_table_grid")
                    .num_columns(5)
                    .striped(true)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        for header in ["CODE", "NAME", "CURRENT", "MAX", "TYPE"] {
                            ui.label(
                                RichText::new(header)
                                    .size(10.0)
                                    .strong()
                                    .color(th::MUTED),
                            );
                        }
                        ui.end_row();
                        for code in st.view.caps.vcp.keys() {
                            ui.label(
                                RichText::new(format!("0x{code:02X}"))
                                    .size(10.0)
                                    .monospace()
                                    .color(th::TEXT_DIM),
                            );
                            ui.label(
                                RichText::new(vcp::vcp_name(*code, st.lang))
                                    .size(10.0)
                                    .color(th::TEXT),
                            );
                            let value = st.view.value(*code);
                            ui.label(
                                RichText::new(
                                    value
                                        .map(|v| v.to_string())
                                        .unwrap_or_else(|| "-".into()),
                                )
                                .size(10.0)
                                .monospace(),
                            );
                            ui.label(
                                RichText::new(st.view.maximum(*code).to_string())
                                    .size(10.0)
                                    .monospace(),
                            );
                            ui.label(
                                RichText::new(st.view.vcp_type(*code).to_string())
                                    .size(10.0)
                                    .monospace(),
                            );
                            ui.end_row();
                        }
                    });
            });
        });
    st.prefs.vcp_table = open;
}

// 供 app.rs 判断主界面是否获得焦点等（保留给后续扩展）。
#[allow(dead_code)]
pub fn status_label(kind: StatusKind) -> Color32 {
    th::status_color(kind)
}
