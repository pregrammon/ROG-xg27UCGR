//! 显示器状态、探测、VCP 写入与异步任务队列。
//!
//! UI 线程从不直接调用 DDC：所有操作都投递到单线程 worker，
//! worker 处理完把快照发布到 `UiState`，并通过 egui Context 唤醒界面。
//! 这与 Python 版的 ThreadPoolExecutor(1) + queue.Queue + generation 保护等价。

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::ddc::{self, MonitorHandle, VcpValue};
use crate::dual_mode::{self, DualMode};
use crate::store::{self, AppSettings, Profile};
use crate::vcp::{self, CapabilityInfo, DisplayMode, Lang, ProductLine};

/// 第一阶段：让界面尽快出现的常用参数（Python FAST_CODES）。
pub const FAST_CODES: &[u8] = &[
    vcp::VCP_BRIGHTNESS,
    vcp::VCP_CONTRAST,
    vcp::VCP_COLOR_TEMP,
    vcp::VCP_INPUT,
    vcp::VCP_DISPLAY_MODE,
    vcp::VCP_VOLUME,
    vcp::VCP_BLUE_LIGHT,
    vcp::VCP_TOGGLES1,
    vcp::VCP_TOGGLES2,
    vcp::VCP_VCP_VERSION,
    vcp::VCP_FIRMWARE,
    vcp::VCP_SHADOW_BOOST,
    vcp::VCP_OVERDRIVE,
    vcp::VCP_GAMMA,
];

/// 第二阶段补齐的参数。
pub const READ_CODES: &[u8] = &[
    vcp::VCP_BRIGHTNESS,
    vcp::VCP_CONTRAST,
    vcp::VCP_COLOR_TEMP,
    vcp::VCP_INPUT,
    vcp::VCP_DISPLAY_MODE,
    vcp::VCP_VOLUME,
    vcp::VCP_BLUE_LIGHT,
    vcp::VCP_TOGGLES1,
    vcp::VCP_TOGGLES2,
    vcp::VCP_VCP_VERSION,
    vcp::VCP_FIRMWARE,
    vcp::VCP_SHADOW_BOOST,
    vcp::VCP_OVERDRIVE,
    vcp::VCP_GAMMA,
    vcp::VCP_RED_GAIN,
    vcp::VCP_GREEN_GAIN,
    vcp::VCP_BLUE_GAIN,
    vcp::VCP_SATURATION,
    vcp::VCP_HUE,
    vcp::VCP_SHARPNESS,
    vcp::VCP_CROSSHAIR,
    vcp::VCP_POWER_MODE,
    vcp::VCP_GAMEPLUS_TIMER,
    vcp::VCP_FPS_COUNTER,
    vcp::VCP_ALIGNMENT,
    vcp::VCP_GAMEPLUS_POSITION,
    vcp::VCP_REST_REMINDER,
    vcp::VCP_OSD_LANGUAGE,
];

/// 配置捕获/应用白名单（Python PROFILE_CODES）。
pub const PROFILE_CODES: &[u8] = &[
    vcp::VCP_DISPLAY_MODE,
    vcp::VCP_BRIGHTNESS,
    vcp::VCP_CONTRAST,
    vcp::VCP_COLOR_TEMP,
    vcp::VCP_RED_GAIN,
    vcp::VCP_GREEN_GAIN,
    vcp::VCP_BLUE_GAIN,
    vcp::VCP_RED_SAT,
    vcp::VCP_YELLOW_SAT,
    vcp::VCP_GREEN_SAT,
    vcp::VCP_CYAN_SAT,
    vcp::VCP_BLUE_SAT,
    vcp::VCP_MAGENTA_SAT,
    vcp::VCP_VOLUME,
    vcp::VCP_SHARPNESS,
    vcp::VCP_SATURATION,
    vcp::VCP_HUE,
    vcp::VCP_GAMMA,
    vcp::VCP_OVERDRIVE,
    vcp::VCP_SHADOW_BOOST,
    vcp::VCP_BLUE_LIGHT,
    vcp::VCP_INPUT,
    vcp::VCP_TOGGLES1,
    vcp::VCP_TOGGLES2,
    vcp::VCP_CROSSHAIR,
    vcp::VCP_GAMEPLUS_TIMER,
    vcp::VCP_FPS_COUNTER,
    vcp::VCP_ALIGNMENT,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StatusKind {
    #[default]
    Ok,
    Warn,
    Error,
    Busy,
}

// ---------------------------------------------------------------------------
// UI 侧快照
// ---------------------------------------------------------------------------
#[derive(Clone, Debug, Default)]
pub struct MonitorView {
    pub key: String,
    pub model: String,
    pub line: ProductLine,
    pub device: String,
    pub description: String,
    pub resolution: String,
    pub primary: bool,
    pub ddc_ok: bool,
    pub probed: bool,
    pub extra_loaded: bool,
    pub error: String,
    pub caps: CapabilityInfo,
    pub values: BTreeMap<u8, VcpValue>,
}

impl MonitorView {
    pub fn value(&self, code: u8) -> Option<u32> {
        self.values.get(&code).map(|v| v.current)
    }

    pub fn maximum(&self, code: u8) -> u32 {
        self.values.get(&code).map(|v| v.maximum).unwrap_or(0)
    }

    pub fn vcp_type(&self, code: u8) -> u32 {
        self.values.get(&code).map(|v| v.vcp_type).unwrap_or(0)
    }

    pub fn supported(&self, code: u8) -> bool {
        self.caps.supports(code) || self.maximum(code) > 0
    }

    pub fn caps_values(&self, code: u8) -> &[u32] {
        self.caps.values(code)
    }

    pub fn is_on(&self, code: u8, bit: u32) -> bool {
        (self.value(code).unwrap_or(0) & bit) != 0
    }

    pub fn is_new_vcp(&self) -> bool {
        let max = self.maximum(vcp::VCP_VCP_VERSION);
        if max == 0 {
            return true;
        }
        (max >> 8) == 2
    }

    pub fn short_name(&self) -> String {
        if !self.model.is_empty() {
            self.model.clone()
        } else if !self.description.is_empty() {
            self.description.clone()
        } else {
            "Display".into()
        }
    }

    pub fn display_name(&self) -> String {
        let short = self.device.rsplit('\\').next().unwrap_or(&self.device);
        if short.is_empty() {
            self.short_name()
        } else {
            format!("{}  ({})", self.short_name(), short)
        }
    }

    pub fn device_short(&self) -> String {
        self.device.rsplit('\\').next().unwrap_or(&self.device).to_string()
    }

    pub fn firmware(&self) -> String {
        vcp::firmware_string(
            self.value(vcp::VCP_FIRMWARE).unwrap_or(0),
            self.maximum(vcp::VCP_FIRMWARE),
        )
    }

    pub fn vcp_version(&self) -> String {
        vcp::vcp_version_string(self.maximum(vcp::VCP_VCP_VERSION))
    }

    pub fn current_mode_value(&self) -> Option<u32> {
        let raw = self.value(vcp::display_mode_code(self.line))?;
        if self.line == ProductLine::ProArt {
            Some(raw)
        } else {
            Some(raw & 0xFF)
        }
    }

    pub fn current_mode(&self) -> Option<&'static DisplayMode> {
        let current = self.current_mode_value()?;
        vcp::mode_set(self.line)
            .iter()
            .find(|m| m.value == current)
    }

    pub fn mode_supported(&self, value: u32) -> bool {
        vcp::mode_supported(&self.caps, self.line, value)
    }

    pub fn input_name(&self) -> String {
        match self.value(vcp::VCP_INPUT) {
            Some(value) => vcp::input_name(value),
            None => "—".into(),
        }
    }

    pub fn color_temp_name(&self, lang: Lang) -> String {
        let Some(value) = self.value(vcp::VCP_COLOR_TEMP) else {
            return "—".into();
        };
        vcp::color_temps(self.line, &self.model)
            .iter()
            .find(|t| t.value == value)
            .map(|t| t.label(lang).to_string())
            .unwrap_or_else(|| format!("0x{value:02X}"))
    }

    pub fn temp_options(&self, lang: Lang) -> Vec<(u32, String)> {
        let advertised = self.caps_values(vcp::VCP_COLOR_TEMP);
        let mut items: Vec<&vcp::ColorTemp> =
            vcp::color_temps(self.line, &self.model).iter().collect();
        if !advertised.is_empty() {
            items.retain(|t| advertised.contains(&t.value));
        }
        items
            .into_iter()
            .map(|t| (t.value, t.label(lang).to_string()))
            .collect()
    }

    pub fn gamma_options(&self) -> Vec<(u32, String)> {
        let advertised = self.caps_values(vcp::VCP_GAMMA);
        let known: Vec<u32> = vcp::GAMMA_STEPS.iter().map(|(v, _)| *v).collect();
        let use_filter = !advertised.is_empty() && advertised.iter().any(|v| known.contains(v));
        vcp::GAMMA_STEPS
            .iter()
            .filter(|(value, _)| !use_filter || advertised.contains(value))
            .map(|(value, label)| (*value, (*label).to_string()))
            .collect()
    }

    pub fn input_options(&self) -> Vec<(u32, String)> {
        let values: Vec<u32> = self
            .caps_values(vcp::VCP_INPUT)
            .to_vec()
            .into_iter()
            .chain(if self.caps_values(vcp::VCP_INPUT).is_empty() {
                vec![15, 17, 18, 26]
            } else {
                vec![]
            })
            .collect();
        values
            .into_iter()
            .map(|v| (v, vcp::input_name(v)))
            .collect()
    }

    pub fn osd_language_options(&self) -> Vec<(u32, String)> {
        let advertised = self.caps_values(vcp::VCP_OSD_LANGUAGE);
        let values: Vec<u32> = if advertised.is_empty() {
            vcp::OSD_LANGUAGES.iter().map(|(v, _)| *v).collect()
        } else {
            advertised.to_vec()
        };
        values
            .into_iter()
            .map(|v| (v, vcp::osd_language_name(v)))
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct UiState {
    pub monitors: Vec<MonitorView>,
    pub selected: usize,
    pub scanning: bool,
    pub busy: bool,
    pub status: String,
    pub status_kind: StatusKind,
    pub generation: u64,
    pub progress: (usize, usize),
    /// Frame Rate Boost（4K / 1K 双模）状态。
    pub boost_known: bool,
    pub boost_one_k: bool,
    pub boost_switching: bool,
}

impl UiState {
    pub fn monitor(&self) -> Option<&MonitorView> {
        self.monitors.get(self.selected)
    }
}

// ---------------------------------------------------------------------------
// 任务
// ---------------------------------------------------------------------------
#[derive(Debug)]
pub enum Job {
    Scan { force: bool },
    Select(usize),
    Read(u8),
    Write { code: u8, value: u32, label: String },
    ToggleBit { code: u8, bit: u32, on: bool, label: String },
    SetMode { value: u32, label: String },
    WriteMany { pairs: Vec<(u8, u32)>, label: String },
    ApplyProfile { profile: Profile },
    RefreshAll,
    BrightnessDelta(i32),
    ContrastDelta(i32),
    ToggleBlueLight,
    CycleMode,
    ToggleMute,
    FactoryReset { color_only: bool },
    /// Frame Rate Boost：true = 1K 高刷，false = 4K。
    SetBoost { one_k: bool },
    Stop,
}

#[derive(Clone)]
pub struct WorkerHandle {
    pub tx: Sender<Job>,
}

impl WorkerHandle {
    pub fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }
}

// ---------------------------------------------------------------------------
// worker 内部状态
// ---------------------------------------------------------------------------
struct MonitorState {
    handle: MonitorHandle,
    view: MonitorView,
}

impl MonitorState {
    fn new(handle: MonitorHandle) -> MonitorState {
        let view = MonitorView {
            key: format!(
                "{}|{}|{}",
                handle.device_name, handle.description, handle.index
            ),
            device: handle.device_name.clone(),
            description: handle.description.clone(),
            resolution: handle.resolution(),
            primary: handle.primary,
            ..Default::default()
        };
        MonitorState { handle, view }
    }

    fn cache_key(&self) -> String {
        format!("{}|{}", self.view.device, self.view.description)
    }

    fn probe(&mut self, force_caps: bool) {
        let cache_key = self.cache_key();
        let mut caps_text = if force_caps {
            None
        } else {
            store::cached_caps(&cache_key)
        };
        if caps_text.is_none() {
            let queried = self.handle.query_capabilities(3);
            if let Some(text) = &queried {
                let model = vcp::parse_capabilities(text).model;
                store::save_caps_entry(&cache_key, text, &model);
            }
            caps_text = queried.or_else(|| store::cached_caps(&cache_key));
        }
        if let Some(text) = caps_text {
            self.view.caps = vcp::parse_capabilities(&text);
        }
        self.view.model = if !self.view.caps.model.is_empty() {
            self.view.caps.model.clone()
        } else {
            self.view.description.trim().to_string()
        };
        self.view.line = vcp::product_line(&self.view.model);

        // 没有能力字符串时只探测少量常用项：虚拟屏 / KVM 幽灵屏否则会在
        // 失败的 I2C 事务上耗掉好几秒。
        let codes: &[u8] = if self.view.caps.vcp_count() > 0 {
            FAST_CODES
        } else {
            &[
                vcp::VCP_BRIGHTNESS,
                vcp::VCP_CONTRAST,
                vcp::VCP_INPUT,
                vcp::VCP_VOLUME,
                vcp::VCP_COLOR_TEMP,
                vcp::VCP_DISPLAY_MODE,
            ]
        };
        let mut successes = 0;
        for code in codes {
            if self.read(*code).is_some() {
                successes += 1;
            }
        }
        self.view.ddc_ok = successes > 0;
        self.view.probed = true;
        self.view.error = if self.view.ddc_ok {
            String::new()
        } else {
            "DDC/CI 无响应（显示器可能不支持或已休眠）".into()
        };
    }

    fn probe_remaining(&mut self) {
        if self.view.caps.vcp_count() == 0 {
            return;
        }
        for code in READ_CODES {
            if self.view.values.contains_key(code) || !self.view.supported(*code) {
                continue;
            }
            self.read(*code);
        }
        for code in [
            vcp::VCP_RED_SAT,
            vcp::VCP_YELLOW_SAT,
            vcp::VCP_GREEN_SAT,
            vcp::VCP_CYAN_SAT,
            vcp::VCP_BLUE_SAT,
            vcp::VCP_MAGENTA_SAT,
            vcp::VCP_COLOR_WEAKNESS,
        ] {
            if !self.view.values.contains_key(&code) && self.view.supported(code) {
                self.read(code);
            }
        }
        self.view.extra_loaded = true;
    }

    fn refresh(&mut self) {
        for code in READ_CODES {
            if self.view.supported(*code) {
                self.read(*code);
            }
        }
        for code in [
            vcp::VCP_RED_SAT,
            vcp::VCP_YELLOW_SAT,
            vcp::VCP_GREEN_SAT,
            vcp::VCP_BLUE_SAT,
            vcp::VCP_CYAN_SAT,
            vcp::VCP_MAGENTA_SAT,
        ] {
            if self.view.supported(code) {
                self.read(code);
            }
        }
    }

    fn read(&mut self, code: u8) -> Option<VcpValue> {
        let value = self.handle.get_vcp(code, 2)?;
        self.view.values.insert(code, value);
        Some(value)
    }

    fn write(&mut self, code: u8, value: u32) -> bool {
        if !self.handle.set_vcp(code, value, 2) {
            return false;
        }
        let existing = self.view.values.get(&code).copied();
        self.view.values.insert(
            code,
            VcpValue {
                code,
                current: value,
                maximum: existing.map(|v| v.maximum).unwrap_or(0),
                vcp_type: existing.map(|v| v.vcp_type).unwrap_or(0),
            },
        );
        true
    }

    fn write_toggle_bit(&mut self, code: u8, bit: u32, enabled: bool) -> bool {
        let current = self
            .view
            .value(code)
            .or_else(|| self.read(code).map(|v| v.current))
            .unwrap_or(0);
        let new_value = if enabled {
            current | bit
        } else {
            current & !bit
        };
        self.write(code, new_value)
    }

    fn set_mode(&mut self, value: u32) -> bool {
        let code = vcp::display_mode_code(self.view.line);
        let encoded = vcp::encode_display_mode(self.view.line, value);
        self.write(code, encoded)
    }
}

struct Worker {
    states: Vec<MonitorState>,
    selected: usize,
    shared: Arc<Mutex<UiState>>,
    settings: Arc<Mutex<AppSettings>>,
    ctx: egui::Context,
}

impl Worker {
    fn lang(&self) -> Lang {
        let code = self
            .settings
            .lock()
            .map(|s| s.language.clone())
            .unwrap_or_else(|e| e.into_inner().language.clone());
        Lang::from_code(&code)
    }

    fn t(&self, zh: &'static str, en: &'static str) -> String {
        self.lang().t(zh, en).to_string()
    }

    fn status(&self, text: impl Into<String>, kind: StatusKind) {
        if let Ok(mut state) = self.shared.lock() {
            state.status = text.into();
            state.status_kind = kind;
            state.generation += 1;
        }
        self.ctx.request_repaint();
    }

    fn publish(&self) {
        if let Ok(mut state) = self.shared.lock() {
            state.monitors = self.states.iter().map(|s| s.view.clone()).collect();
            state.selected = self.selected;
            state.generation += 1;
        }
        self.ctx.request_repaint();
    }

    fn set_busy(&self, busy: bool, scanning: bool) {
        if let Ok(mut state) = self.shared.lock() {
            state.busy = busy;
            state.scanning = scanning;
            state.generation += 1;
        }
        self.ctx.request_repaint();
    }

    fn set_progress(&self, index: usize, total: usize) {
        if let Ok(mut state) = self.shared.lock() {
            state.progress = (index, total);
        }
    }

    fn selected_mut(&mut self) -> Option<&mut MonitorState> {
        self.states.get_mut(self.selected)
    }

    fn restore_selection(&mut self) {
        let key = self
            .settings
            .lock()
            .map(|s| s.monitor_key.clone())
            .unwrap_or_default();
        if key.is_empty() {
            return;
        }
        if let Some(index) = self.states.iter().position(|s| s.view.key == key) {
            self.selected = index;
        }
    }

    fn scan(&mut self, force: bool) {
        // 这块面板在 1K/485 Hz 模式下 VCP 读取会阻塞，
        // 因此该模式只做轻量枚举，绝不发起 VCP 读。
        if dual_mode::get_current_mode() == DualMode::Mode1K {
            self.scan_shell();
            return;
        }
        self.set_busy(true, true);
        self.status(self.t("正在枚举显示器…", "Enumerating monitors..."), StatusKind::Busy);
        self.states.clear();
        self.selected = 0;
        self.publish();

        let handles = ddc::enumerate_monitors();
        if handles.is_empty() {
            self.set_busy(false, false);
            self.status(self.t("未发现显示器", "No monitors found"), StatusKind::Error);
            self.publish();
            return;
        }
        self.states = handles.into_iter().map(MonitorState::new).collect();
        self.restore_selection();
        self.publish();

        let total = self.states.len();
        for index in 0..total {
            self.set_progress(index, total);
            self.states[index].probe(force);
            self.publish();
        }
        self.set_busy(false, false);
        self.publish();

        // 第二阶段：后台补齐其余参数。
        self.set_busy(true, false);
        for index in 0..total {
            self.states[index].probe_remaining();
            self.publish();
        }
        self.set_busy(false, false);
        self.refresh_boost();
        self.status(self.t("显示器已就绪", "Monitor ready"), StatusKind::Ok);
        self.publish();
    }

    /// 1K FPS 模式下的轻量扫描：只枚举显示器，不做任何 VCP 读取。
    fn scan_shell(&mut self) {
        self.set_busy(true, true);
        self.status(
            self.t("正在枚举显示器…", "Enumerating monitors..."),
            StatusKind::Busy,
        );
        self.states.clear();
        self.selected = 0;
        self.publish();

        let handles = ddc::enumerate_monitors();
        if handles.is_empty() {
            self.set_busy(false, false);
            self.status(self.t("未发现显示器", "No monitors found"), StatusKind::Error);
            self.publish();
            return;
        }
        self.states = handles.into_iter().map(MonitorState::new).collect();
        self.restore_selection();
        for state in &mut self.states {
            // 没有能力字符串可用：用设备描述充当型号，控件保持可见但不可调。
            state.view.model = state.view.description.clone();
            state.view.line = vcp::product_line(&state.view.model);
            state.view.ddc_ok = true;
            state.view.probed = true;
            state.view.extra_loaded = true;
            state.view.error = String::new();
        }
        self.set_busy(false, false);
        self.refresh_boost();
        self.status(
            self.t(
                "1K FPS 模式：DDC 读取受限，切回 4K 可调参数",
                "1K FPS mode: DDC reads are unavailable; switch back to 4K to adjust",
            ),
            StatusKind::Warn,
        );
        self.publish();
    }

    /// 从 Windows 当前分辨率读取双模状态（这块屏的 VCP 回读不可靠）。
    fn refresh_boost(&self) {
        let mode = dual_mode::get_current_mode();
        if let Ok(mut state) = self.shared.lock() {
            state.boost_known = mode != DualMode::Unknown;
            if mode != DualMode::Unknown {
                state.boost_one_k = mode == DualMode::Mode1K;
            }
        }
    }

    fn set_boost(&mut self, one_k: bool) {
        store::append_log("lifecycle.log", &format!("boost start one_k={one_k}"));
        {
            let Ok(mut state) = self.shared.lock() else {
                return;
            };
            if state.boost_switching {
                return;
            }
            state.boost_switching = true;
            state.generation += 1;
        }
        self.ctx.request_repaint();
        self.status(
            self.t("正在切换 Frame Rate Boost…", "Switching Frame Rate Boost..."),
            StatusKind::Busy,
        );
        self.publish();

        let target = if one_k {
            DualMode::Mode1K
        } else {
            DualMode::Mode4K
        };
        let result = dual_mode::switch_to(target);
        store::append_log("lifecycle.log", &format!("boost switch_to result={result:?}"));
        match result {
            Ok(()) => {
                // 面板重新握手后旧句柄失效：重新枚举并补齐参数。
                self.scan(false);
                let mode = dual_mode::get_current_mode();
                self.refresh_boost();
                if let Ok(mut state) = self.shared.lock() {
                    state.boost_switching = false;
                }
                let label = if mode == DualMode::Mode1K {
                    self.t(
                        "Frame Rate Boost：1K 高刷已启用（DDC 读取受限）",
                        "Frame Rate Boost: 1K high refresh enabled (DDC reads unavailable)",
                    )
                } else if mode == DualMode::Mode4K {
                    self.t("Frame Rate Boost：已切回 4K", "Frame Rate Boost: back to 4K")
                } else {
                    self.t("Frame Rate Boost 已切换", "Frame Rate Boost switched")
                };
                self.status(label, StatusKind::Ok);
            }
            Err(error) => {
                if let Ok(mut state) = self.shared.lock() {
                    state.boost_switching = false;
                }
                self.status(
                    format!("{}: {error}", self.t("切换失败", "Switch failed")),
                    StatusKind::Error,
                );
            }
        }
        self.publish();
    }

    fn handle(&mut self, job: Job) -> bool {
        // 1K FPS 模式下 DDC 读会阻塞，所有需要读取/写入的操作都直接拒绝。
        if dual_mode::get_current_mode() == DualMode::Mode1K {
            let blocked = match &job {
                Job::Read(_)
                | Job::Write { .. }
                | Job::ToggleBit { .. }
                | Job::SetMode { .. }
                | Job::WriteMany { .. }
                | Job::ApplyProfile { .. }
                | Job::RefreshAll
                | Job::BrightnessDelta(_)
                | Job::ContrastDelta(_)
                | Job::ToggleBlueLight
                | Job::CycleMode
                | Job::ToggleMute
                | Job::FactoryReset { .. } => true,
                _ => false,
            };
            if blocked {
                self.status(
                    self.t(
                        "1K FPS 模式下 DDC 不可用，请先切回 4K",
                        "DDC is unavailable in 1K FPS mode; switch back to 4K first",
                    ),
                    StatusKind::Warn,
                );
                self.publish();
                return true;
            }
        }
        match job {
            Job::Stop => return false,
            Job::Scan { force } => self.scan(force),
            Job::Select(index) => {
                if index < self.states.len() {
                    self.selected = index;
                    let key = self.states[index].view.key.clone();
                    if let Ok(mut settings) = self.settings.lock() {
                        settings.monitor_key = key;
                    }
                    self.publish();
                }
            }
            Job::Read(code) => {
                if let Some(state) = self.selected_mut() {
                    state.read(code);
                }
                self.publish();
            }
            Job::Write { code, value, label } => self.write_one(code, value, &label),
            Job::ToggleBit {
                code,
                bit,
                on,
                label,
            } => {
                let ok = self
                    .selected_mut()
                    .map(|s| s.write_toggle_bit(code, bit, on))
                    .unwrap_or(false);
                let text = if ok {
                    format!("{} → {}", label, if on { "ON" } else { "OFF" })
                } else {
                    format!("{} ✕", label)
                };
                self.status(text, if ok { StatusKind::Ok } else { StatusKind::Error });
                self.publish();
            }
            Job::SetMode { value, label } => self.set_mode(value, &label),
            Job::WriteMany { pairs, label } => self.write_many(&pairs, &label),
            Job::ApplyProfile { profile } => self.apply_profile(&profile),
            Job::RefreshAll => {
                self.set_busy(true, false);
                if let Some(state) = self.selected_mut() {
                    state.refresh();
                }
                self.set_busy(false, false);
                self.status(self.t("参数已刷新", "Values refreshed"), StatusKind::Ok);
                self.publish();
            }
            Job::BrightnessDelta(delta) => self.delta(vcp::VCP_BRIGHTNESS, delta),
            Job::ContrastDelta(delta) => self.delta(vcp::VCP_CONTRAST, delta),
            Job::ToggleBlueLight => self.toggle_blue_light(),
            Job::CycleMode => self.cycle_mode(),
            Job::ToggleMute => self.toggle_mute(),
            Job::SetBoost { one_k } => self.set_boost(one_k),
            Job::FactoryReset { color_only } => {
                let code = if color_only {
                    vcp::VCP_RESTORE_COLOR
                } else {
                    vcp::VCP_RESTORE_FACTORY
                };
                let ok = self.selected_mut().map(|s| s.write(code, 1)).unwrap_or(false);
                thread::sleep(Duration::from_millis(800));
                if let Some(state) = self.selected_mut() {
                    state.refresh();
                }
                self.status(
                    if ok {
                        self.t("恢复出厂设置已发送", "Factory reset sent")
                    } else {
                        self.t("恢复出厂设置失败", "Factory reset failed")
                    },
                    if ok { StatusKind::Ok } else { StatusKind::Error },
                );
                self.publish();
            }
        }
        true
    }

    fn write_one(&mut self, code: u8, value: u32, label: &str) {
        let Some(state) = self.selected_mut() else {
            return;
        };
        let ok = state.write(code, value);
        if ok {
            // 回读一次，让固件钳制后的真实值立刻显示出来。
            state.read(code);
            self.status(format!("{label} → {value}"), StatusKind::Ok);
        } else {
            self.status(format!("{label} ✕"), StatusKind::Error);
        }
        self.publish();
    }

    fn set_mode(&mut self, value: u32, label: &str) {
        let Some(state) = self.selected_mut() else {
            return;
        };
        let ok = state.set_mode(value);
        if ok {
            // 显示器会按模式恢复自己的预设，稍等再回读。
            thread::sleep(Duration::from_millis(350));
            for code in [
                vcp::VCP_DISPLAY_MODE,
                vcp::VCP_COLOR_TEMP,
                vcp::VCP_GAMMA,
                vcp::VCP_BLUE_LIGHT,
                vcp::VCP_SHADOW_BOOST,
                vcp::VCP_OVERDRIVE,
                vcp::VCP_TOGGLES2,
            ] {
                state.read(code);
            }
            self.status(format!("{label}"), StatusKind::Ok);
        } else {
            self.status(self.t("切换显示模式失败", "Failed to switch display mode"), StatusKind::Error);
        }
        self.publish();
    }

    fn write_many(&mut self, pairs: &[(u8, u32)], label: &str) {
        let Some(state) = self.selected_mut() else {
            return;
        };
        let mut count = 0;
        for (code, value) in pairs {
            if state.view.supported(*code) && state.write(*code, *value) {
                count += 1;
            }
            thread::sleep(Duration::from_millis(30));
        }
        // 回读这次批量写入涉及的码，让挂件/主界面马上显示新模式下的值。
        for (code, _) in pairs {
            state.read(*code);
        }
        self.status(format!("{label} ({count})"), StatusKind::Ok);
        self.publish();
    }

    fn apply_profile(&mut self, profile: &Profile) {
        self.status(
            self.t("正在应用配置…", "Applying profile..."),
            StatusKind::Busy,
        );
        let Some(state) = self.selected_mut() else {
            return;
        };
        let mut entries: Vec<(u8, u32)> = profile
            .vcp
            .iter()
            .filter_map(|(key, value)| key.parse::<u8>().ok().map(|code| (code, *value)))
            .collect();
        // 显示模式优先写入，其余按码值排序。
        entries.sort_by_key(|(code, _)| {
            if *code == vcp::VCP_DISPLAY_MODE || *code == vcp::VCP_CROSSHAIR {
                0
            } else {
                1
            }
        });
        let mut applied = 0;
        let mut failed = 0;
        for (code, value) in entries {
            // ProArt 的模式码是 0xE3；普通机型是 0xDC。
            if (code == vcp::VCP_DISPLAY_MODE && state.view.line != ProductLine::ProArt)
                || (code == vcp::VCP_CROSSHAIR && state.view.line == ProductLine::ProArt)
            {
                if state.set_mode(value) {
                    applied += 1;
                } else {
                    failed += 1;
                }
            } else if state.write(code, value) {
                applied += 1;
            } else {
                failed += 1;
            }
            thread::sleep(Duration::from_millis(20));
        }
        state.refresh();
        let text = if failed > 0 {
            format!(
                "{} {applied}, {failed} {}",
                self.t("应用完成：", "Applied"),
                self.t("项失败", "failed")
            )
        } else {
            format!(
                "{}（{applied} {}）",
                self.t("配置已应用", "Profile applied"),
                self.t("项", "values")
            )
        };
        self.status(text, if failed > 0 { StatusKind::Warn } else { StatusKind::Ok });
        self.publish();
    }

    fn delta(&mut self, code: u8, delta: i32) {
        let label = vcp::vcp_name(code, self.lang()).to_string();
        let Some(state) = self.selected_mut() else {
            return;
        };
        let current = state
            .view
            .value(code)
            .or_else(|| state.read(code).map(|v| v.current));
        let Some(current) = current else {
            return;
        };
        let maximum = state.view.maximum(code).max(1) as i32;
        let value = (current as i32 + delta).clamp(0, maximum) as u32;
        state.write(code, value);
        self.status(format!("{label} → {value}"), StatusKind::Ok);
        self.publish();
    }

    fn toggle_blue_light(&mut self) {
        let Some(state) = self.selected_mut() else {
            return;
        };
        let current = state.view.value(vcp::VCP_BLUE_LIGHT).unwrap_or(0);
        let target = if current == 0 { 3 } else { 0 };
        let ok = state.write(vcp::VCP_BLUE_LIGHT, target);
        let label = vcp::vcp_name(vcp::VCP_BLUE_LIGHT, self.lang()).to_string();
        self.status(
            if ok {
                format!("{label} → {target}")
            } else {
                format!("{label} ✕")
            },
            if ok { StatusKind::Ok } else { StatusKind::Error },
        );
        self.publish();
    }

    fn cycle_mode(&mut self) {
        let Some(state) = self.selected_mut() else {
            return;
        };
        let modes: Vec<u32> = vcp::mode_set(state.view.line)
            .iter()
            .filter(|m| state.view.mode_supported(m.value))
            .map(|m| m.value)
            .collect();
        if modes.is_empty() {
            return;
        }
        let current = state.view.current_mode_value();
        let index = current
            .and_then(|c| modes.iter().position(|v| *v == c))
            .map(|i| (i + 1) % modes.len())
            .unwrap_or(0);
        let target = modes[index];
        let ok = state.set_mode(target);
        if ok {
            thread::sleep(Duration::from_millis(350));
            for code in [
                vcp::VCP_DISPLAY_MODE,
                vcp::VCP_COLOR_TEMP,
                vcp::VCP_GAMMA,
                vcp::VCP_BLUE_LIGHT,
                vcp::VCP_TOGGLES2,
            ] {
                state.read(code);
            }
        }
        let mode = vcp::mode_set(state.view.line)
            .iter()
            .find(|m| m.value == target);
        let name = mode
            .map(|m| m.label(self.lang()).to_string())
            .unwrap_or_else(|| target.to_string());
        self.status(
            if ok {
                format!("{} → {name}", self.t("显示模式", "Display mode"))
            } else {
                self.t("切换显示模式失败", "Failed to switch display mode")
            },
            if ok { StatusKind::Ok } else { StatusKind::Error },
        );
        self.publish();
    }

    fn toggle_mute(&mut self) {
        let Some(state) = self.selected_mut() else {
            return;
        };
        let on = !state.view.is_on(vcp::VCP_TOGGLES1, vcp::MUTE_BIT);
        let ok = state.write_toggle_bit(vcp::VCP_TOGGLES1, vcp::MUTE_BIT, on);
        let label = self.t("静音", "Mute");
        self.status(
            if ok {
                format!("{label} → {}", if on { "ON" } else { "OFF" })
            } else {
                format!("{label} ✕")
            },
            if ok { StatusKind::Ok } else { StatusKind::Error },
        );
        self.publish();
    }
}

// ---------------------------------------------------------------------------
// 启动
// ---------------------------------------------------------------------------
pub fn spawn(shared: Arc<Mutex<UiState>>, settings: Arc<Mutex<AppSettings>>, ctx: egui::Context) -> WorkerHandle {
    let (tx, rx): (Sender<Job>, Receiver<Job>) = mpsc::channel();
    thread::spawn(move || {
        let mut worker = Worker {
            states: Vec::new(),
            selected: 0,
            shared,
            settings,
            ctx,
        };
        // 启动即扫描：隐藏的主界面可以在打开时立刻有数据，全局快捷键也可用。
        worker.scan(false);
        while let Ok(job) = rx.recv() {
            if !worker.handle(job) {
                break;
            }
        }
        worker.states.clear();
    });
    WorkerHandle { tx }
}

/// 快速场景的 VCP 组合，主界面与桌面小挂件共用。
pub fn scene_pairs(view: &MonitorView, key: &str) -> Vec<(u8, u32)> {
    let mut pairs: Vec<(u8, u32)> = Vec::new();
    let mode_code = vcp::display_mode_code(view.line);
    let find = |name: &str| {
        vcp::mode_set(view.line)
            .iter()
            .find(|m| m.key == name)
            .map(|m| m.value)
    };
    match key {
        "movie" => {
            if let Some(value) = find("cinema").or_else(|| find("theater")) {
                pairs.push((mode_code, vcp::encode_display_mode(view.line, value)));
            }
            pairs.push((vcp::VCP_BLUE_LIGHT, 1));
        }
        "game" => {
            if let Some(value) = find("fps")
                .or_else(|| find("racing"))
                .or_else(|| find("game"))
            {
                pairs.push((mode_code, vcp::encode_display_mode(view.line, value)));
            }
            pairs.push((vcp::VCP_BLUE_LIGHT, 0));
            if view.supported(vcp::VCP_SHADOW_BOOST) {
                pairs.push((vcp::VCP_SHADOW_BOOST, 2));
            }
        }
        "reading" => {
            if let Some(value) = find("reading") {
                pairs.push((mode_code, vcp::encode_display_mode(view.line, value)));
            }
            pairs.push((vcp::VCP_BLUE_LIGHT, 3));
        }
        _ => {
            if let Some(value) = find("night").or_else(|| find("darkroom")) {
                pairs.push((mode_code, vcp::encode_display_mode(view.line, value)));
            }
            pairs.push((vcp::VCP_BLUE_LIGHT, 4));
        }
    }
    pairs
}

/// 从当前快照抓一份配置（Python capture_profile）。
pub fn capture_profile(view: &MonitorView, name: &str) -> Profile {
    let mut vcp_values = BTreeMap::new();
    for code in PROFILE_CODES {
        if !view.supported(*code) {
            continue;
        }
        if let Some(value) = view.value(*code) {
            vcp_values.insert(code.to_string(), value);
        }
    }
    Profile {
        name: name.to_string(),
        model: view.model.clone(),
        line: view.line.as_str().to_string(),
        created: store::timestamp(),
        vcp: vcp_values,
    }
}
