//! 全局快捷键：RegisterHotKey + 独立消息循环线程。
//!
//! 原版安装低级键盘钩子；RegisterHotKey 是等价且更安全的做法
//! （Python 复刻版同样采用它）。快捷键动作直接投递到 DDC worker。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE};

use crate::worker::{Job, WorkerHandle};

const WM_HOTKEY: u32 = 0x0312;

const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_B: u32 = 0x42;
const VK_M: u32 = 0x4D;
const VK_V: u32 = 0x56;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    BrightnessUp,
    BrightnessDown,
    ContrastUp,
    ContrastDown,
    ToggleBlueLight,
    CycleMode,
    ToggleMute,
}

pub struct Binding {
    pub action: HotkeyAction,
    pub vk: u32,
    pub zh: &'static str,
    pub en: &'static str,
    pub keys: &'static str,
}

pub const BINDINGS: &[Binding] = &[
    Binding {
        action: HotkeyAction::BrightnessUp,
        vk: VK_UP,
        zh: "亮度 +5",
        en: "Brightness +5",
        keys: "Ctrl + Alt + ↑",
    },
    Binding {
        action: HotkeyAction::BrightnessDown,
        vk: VK_DOWN,
        zh: "亮度 -5",
        en: "Brightness -5",
        keys: "Ctrl + Alt + ↓",
    },
    Binding {
        action: HotkeyAction::ContrastUp,
        vk: VK_RIGHT,
        zh: "对比度 +5",
        en: "Contrast +5",
        keys: "Ctrl + Alt + →",
    },
    Binding {
        action: HotkeyAction::ContrastDown,
        vk: VK_LEFT,
        zh: "对比度 -5",
        en: "Contrast -5",
        keys: "Ctrl + Alt + ←",
    },
    Binding {
        action: HotkeyAction::ToggleBlueLight,
        vk: VK_B,
        zh: "切换滤蓝光",
        en: "Toggle blue light filter",
        keys: "Ctrl + Alt + B",
    },
    Binding {
        action: HotkeyAction::CycleMode,
        vk: VK_M,
        zh: "切换显示模式",
        en: "Cycle display mode",
        keys: "Ctrl + Alt + M",
    },
    Binding {
        action: HotkeyAction::ToggleMute,
        vk: VK_V,
        zh: "静音切换",
        en: "Toggle mute",
        keys: "Ctrl + Alt + V",
    },
];

pub struct HotkeyManager {
    stop: Arc<AtomicBool>,
    statuses: Arc<Mutex<Vec<bool>>>,
    thread: Option<JoinHandle<()>>,
}

impl HotkeyManager {
    pub fn start(worker: WorkerHandle) -> HotkeyManager {
        let stop = Arc::new(AtomicBool::new(false));
        let statuses = Arc::new(Mutex::new(vec![false; BINDINGS.len()]));
        let stop_thread = stop.clone();
        let status_thread = statuses.clone();
        // 等待注册完成后再返回，否则 UI 第一帧会显示 0/7。
        let ready = Arc::new((Mutex::new(false), Condvar::new()));
        let ready_thread = ready.clone();
        let thread = thread::spawn(move || {
            let mut registered: Vec<i32> = Vec::new();
            for (index, binding) in BINDINGS.iter().enumerate() {
                let id = index as i32 + 1;
                let mods = HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0);
                let ok = unsafe { RegisterHotKey(HWND::default(), id, mods, binding.vk) }.is_ok();
                if ok {
                    registered.push(id);
                }
                if let Ok(mut status) = status_thread.lock() {
                    if index < status.len() {
                        status[index] = ok;
                    }
                }
            }
            {
                let (lock, cvar) = &*ready_thread;
                if let Ok(mut done) = lock.lock() {
                    *done = true;
                }
                cvar.notify_all();
            }

            let mut msg = MSG::default();
            while !stop_thread.load(Ordering::Relaxed) {
                loop {
                    let has = unsafe {
                        PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE)
                    }
                    .as_bool();
                    if !has {
                        break;
                    }
                    if msg.message == WM_HOTKEY {
                        let index = msg.wParam.0 as usize;
                        if index >= 1 && index <= BINDINGS.len() {
                            let job = match BINDINGS[index - 1].action {
                                HotkeyAction::BrightnessUp => Job::BrightnessDelta(5),
                                HotkeyAction::BrightnessDown => Job::BrightnessDelta(-5),
                                HotkeyAction::ContrastUp => Job::ContrastDelta(5),
                                HotkeyAction::ContrastDown => Job::ContrastDelta(-5),
                                HotkeyAction::ToggleBlueLight => Job::ToggleBlueLight,
                                HotkeyAction::CycleMode => Job::CycleMode,
                                HotkeyAction::ToggleMute => Job::ToggleMute,
                            };
                            worker.send(job);
                        }
                    }
                }
                thread::sleep(Duration::from_millis(40));
            }

            for id in registered {
                let _ = unsafe { UnregisterHotKey(HWND::default(), id) };
            }
        });

        {
            let (lock, cvar) = &*ready;
            if let Ok(guard) = lock.lock() {
                let _ = cvar.wait_timeout_while(guard, Duration::from_secs(2), |done| !*done);
            }
        }

        HotkeyManager {
            stop,
            statuses,
            thread: Some(thread),
        }
    }

    pub fn statuses(&self) -> Vec<bool> {
        self.statuses
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 快捷键的运行时状态，可被主窗口读写。
pub struct HotkeyRuntime {
    pub enabled: bool,
    pub statuses: Vec<bool>,
    manager: Option<HotkeyManager>,
}

impl Default for HotkeyRuntime {
    fn default() -> Self {
        HotkeyRuntime {
            enabled: false,
            statuses: vec![false; BINDINGS.len()],
            manager: None,
        }
    }
}

impl HotkeyRuntime {
    pub fn start(&mut self, worker: &WorkerHandle) {
        if self.manager.is_some() {
            self.enabled = true;
            return;
        }
        let manager = HotkeyManager::start(worker.clone());
        self.statuses = manager.statuses();
        self.manager = Some(manager);
        self.enabled = true;
    }

    pub fn stop(&mut self) {
        if let Some(mut manager) = self.manager.take() {
            manager.stop();
        }
        self.statuses = vec![false; BINDINGS.len()];
        self.enabled = false;
    }

    pub fn set_enabled(&mut self, enabled: bool, worker: &WorkerHandle) {
        if enabled {
            self.start(worker);
        } else {
            self.stop();
        }
    }

    pub fn active_count(&self) -> usize {
        self.statuses.iter().filter(|s| **s).count()
    }

    /// 供 UI 在注册完成后刷新状态。
    pub fn refresh_statuses(&mut self) {
        if let Some(manager) = &self.manager {
            self.statuses = manager.statuses();
        }
    }
}

/// 无 GUI 场景下方便地构造一个只发送动作的通道（保留给诊断工具）。
pub fn action_channel() -> (mpsc::Sender<HotkeyAction>, mpsc::Receiver<HotkeyAction>) {
    mpsc::channel()
}
