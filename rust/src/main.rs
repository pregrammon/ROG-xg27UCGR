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
mod tray;
mod win32;

fn main() -> eframe::Result<()> {
    // 单实例：第二次启动会激活正在运行的窗口并退出。
    if !unsafe { win32::acquire_single_instance("Local\\RogControl_SingleInstance") } {
        unsafe { win32::activate_existing_instance("ROG Control") };
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ROG Control")
            .with_inner_size([340.0, 232.0])
            .with_min_inner_size([280.0, 160.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "ROG Control",
        options,
        Box::new(|cc| Ok(Box::new(app::RogApp::new(cc)))),
    )
}
