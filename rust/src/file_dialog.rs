//! Win32 通用打开/保存文件对话框（comdlg32），供配置文件导入导出使用。

use std::path::PathBuf;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OPENFILENAMEW, OPEN_FILENAME_FLAGS, OFN_EXPLORER,
    OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
};
use windows::core::{PCWSTR, PWSTR};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if (c as u32) < 0x20 => '-',
            c => c,
        })
        .collect()
}

fn dialog(title: &str, save: bool, default_name: &str, owner: HWND) -> Option<PathBuf> {
    let default_name = sanitize_file_name(default_name);
    let default_name = default_name.as_str();
    let mut buffer = vec![0u16; 1024];
    if !default_name.is_empty() {
        for (i, unit) in default_name.encode_utf16().enumerate().take(buffer.len() - 1) {
            buffer[i] = unit;
        }
    }

    // 过滤器必须以双 NUL 结尾。
    let mut filter: Vec<u16> = Vec::new();
    for part in [
        "JSON 文件 (*.json)",
        "*.json",
        "所有文件 (*.*)",
        "*.*",
    ] {
        filter.extend(part.encode_utf16());
        filter.push(0);
    }
    filter.push(0);

    let title_w = wide(title);
    let def_ext = wide("json");

    let flags = OPEN_FILENAME_FLAGS(
        OFN_EXPLORER.0
            | OFN_PATHMUSTEXIST.0
            | OFN_NOCHANGEDIR.0
            | if save {
                OFN_OVERWRITEPROMPT.0
            } else {
                OFN_FILEMUSTEXIST.0
            },
    );

    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title_w.as_ptr()),
        Flags: flags,
        lpstrDefExt: PCWSTR(def_ext.as_ptr()),
        ..Default::default()
    };

    let ok = unsafe {
        if save {
            GetSaveFileNameW(&mut ofn)
        } else {
            GetOpenFileNameW(&mut ofn)
        }
    };
    if !ok.as_bool() {
        return None;
    }
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..end]);
    (!text.is_empty()).then(|| PathBuf::from(text))
}

pub fn open_json(title: &str, owner: HWND) -> Option<PathBuf> {
    dialog(title, false, "", owner)
}

pub fn save_json(title: &str, default_name: &str, owner: HWND) -> Option<PathBuf> {
    dialog(title, true, default_name, owner)
}
