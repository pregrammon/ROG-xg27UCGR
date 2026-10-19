//! RogControl (Rust) — 单实例透明无边框 ROG 控制
//! 控件：DDC/CI 亮度 + 4K/1K 双模切换 + tray 菜单。

// GUI 子系统：没有它，二进制文件就是控制台应用，Windows 会在控件旁边
// 打开一个控制台窗口。诊断功能依然可用——它们写入
// %APPDATA%\RogControl\debug.log，而不是 stdout。
#![windows_subsystem = "windows"]

mod app;
mod config;
mod ddc;
mod dual_mode;
mod file_dialog;
mod hotkeys;
mod main_window;
mod store;
mod theme;
mod tray;
mod vcp;
mod win32;
mod worker;

fn main() -> eframe::Result<()> {
    store::install_panic_hook();
    store::append_log("lifecycle.log", "main start");
    // 必须在创建任何窗口之前设置。
    win32::make_system_dpi_aware();
    // 单实例：第二次启动通过命名事件让已有实例显示主界面。
    if !unsafe { win32::acquire_single_instance("Local\\RogControl_SingleInstance") } {
        if !unsafe { win32::signal_show_event(win32::SHOW_EVENT_NAME) } {
            unsafe { win32::activate_existing_instance("ROG Control") };
        }
        return Ok(());
    }

    // 先创建“显示主界面”事件，避免第二个实例抢在监听线程之前启动。
    let _show_event = unsafe { win32::create_show_event(win32::SHOW_EVENT_NAME) };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ROG Control Widget")
            .with_inner_size([340.0, 232.0])
            .with_min_inner_size([280.0, 160.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_taskbar(false)
            .with_resizable(true),
        ..Default::default()
    };

    let result = eframe::run_native(
        "ROG Control Widget",
        options,
        Box::new(|cc| Ok(Box::new(app::RogApp::new(cc)))),
    );
    store::append_log(
        "lifecycle.log",
        &match &result {
            Ok(()) => "run_native -> Ok".to_string(),
            Err(error) => format!("run_native -> Err({error})"),
        },
    );
    result
}
