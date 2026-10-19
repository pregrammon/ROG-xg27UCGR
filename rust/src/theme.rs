//! 主界面主题与可复用控件（ROG 深色风格，配色取自 Python 复刻版）。

use std::sync::atomic::{AtomicBool, Ordering};

use egui::{
    Align, Align2, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Frame, Layout,
    Margin, RichText, Rounding, Sense, Stroke, Ui, Vec2,
};

pub const BG: Color32 = Color32::from_rgb(0x0C, 0x0E, 0x12);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x10, 0x13, 0x19);
pub const PANEL: Color32 = Color32::from_rgb(0x14, 0x18, 0x21);
pub const CARD: Color32 = Color32::from_rgb(0x18, 0x1C, 0x25);
pub const HOVER: Color32 = Color32::from_rgb(0x1E, 0x23, 0x30);
pub const CARD_ACTIVE: Color32 = Color32::from_rgb(0x2A, 0x11, 0x19);
pub const BORDER: Color32 = Color32::from_rgb(0x26, 0x2C, 0x38);
pub const TEXT: Color32 = Color32::from_rgb(0xE9, 0xEC, 0xF1);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xA7, 0xAF, 0xBF);
pub const MUTED: Color32 = Color32::from_rgb(0x72, 0x7C, 0x90);
pub const ACCENT: Color32 = Color32::from_rgb(0xF6, 0x36, 0x43);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x3A, 0x0A, 0x12);
pub const GREEN: Color32 = Color32::from_rgb(0x3D, 0xDC, 0x97);
pub const AMBER: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x20);
pub const RED: Color32 = Color32::from_rgb(0xFF, 0x4D, 0x5E);
pub const CYAN: Color32 = Color32::from_rgb(0x39, 0xC6, 0xFF);
pub const WHITE: Color32 = Color32::WHITE;

pub fn apply(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = PANEL;
    visuals.faint_bg_color = CARD;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, WHITE);
    visuals.hyperlink_color = ACCENT;
    visuals.window_rounding = Rounding::same(10.0);
    visuals.menu_rounding = Rounding::same(8.0);
    visuals.widgets.noninteractive.bg_fill = CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    visuals.widgets.inactive.bg_fill = PANEL;
    visuals.widgets.inactive.weak_bg_fill = PANEL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.bg_fill = HOVER;
    visuals.widgets.hovered.weak_bg_fill = HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x35, 0x3C, 0x4B));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, WHITE);
    visuals.widgets.active.bg_fill = CARD_ACTIVE;
    visuals.widgets.active.weak_bg_fill = CARD_ACTIVE;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, WHITE);
    visuals.widgets.open.bg_fill = PANEL;
    visuals.slider_trailing_fill = true;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.window_margin = Margin::same(14.0);
    style.spacing.menu_margin = Margin::same(6.0);
    style.spacing.slider_width = 190.0;
    style.spacing.interact_size.y = 24.0;
    style.text_styles = [
        (egui::TextStyle::Small, FontId::proportional(11.0)),
        (egui::TextStyle::Body, FontId::proportional(13.0)),
        (egui::TextStyle::Button, FontId::proportional(13.0)),
        (egui::TextStyle::Heading, FontId::proportional(20.0)),
        (egui::TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    ctx.set_style(style);
}

/// 启动时的字体：Segoe MDL2（图标字形）+ egui 默认字体。
pub fn setup_fonts(ctx: &Context) {
    ctx.set_fonts(build_fonts(false));
}

static CJK_LOADED: AtomicBool = AtomicBool::new(false);

/// 主界面第一次打开时才加载中文字体（约 16 MB，避免纯挂件模式占用内存）。
pub fn ensure_cjk(ctx: &Context) {
    if CJK_LOADED.load(Ordering::Relaxed) {
        return;
    }
    CJK_LOADED.store(true, Ordering::Relaxed);
    ctx.set_fonts(build_fonts(true));
}

fn build_fonts(with_cjk: bool) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segmdl2.ttf") {
        fonts
            .font_data
            .insert("mdl2".into(), FontData::from_owned(bytes));
    }
    if with_cjk {
        // 优先使用等线（Deng.ttf，纯 TTF，egui 可直接解析）；
        // msyh.ttc / simsun.ttc 是字体集合，ab_glyph 不支持，故跳过。
        for path in ["C:\\Windows\\Fonts\\Deng.ttf", "C:\\Windows\\Fonts\\simhei.ttf"] {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("cjk".into(), FontData::from_owned(bytes));
                break;
            }
        }
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            if fonts.font_data.contains_key("cjk") {
                list.push("cjk".into());
            }
            if fonts.font_data.contains_key("mdl2") {
                list.push("mdl2".into());
            }
        }
    }
    fonts
}

pub fn card_frame() -> Frame {
    Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::same(16.0))
}

/// 带标题的卡片；返回闭包结果。
pub fn card<R>(ui: &mut Ui, title: Option<(&str, Option<&str>)>, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame()
        .show(ui, |ui| {
            // 让卡片填满所在列/容器，而不是收缩到内容宽度。
            ui.set_min_width(ui.available_width());
            if let Some((title, subtitle)) = title {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).size(14.0).strong().color(TEXT));
                    if let Some(subtitle) = subtitle {
                        ui.label(RichText::new(subtitle).size(10.0).color(MUTED));
                    }
                });
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner
}

pub fn section_header(ui: &mut Ui, title: &str, subtitle: &str, right: Option<&str>) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(16.0).strong().color(TEXT));
            if !subtitle.is_empty() {
                ui.label(RichText::new(subtitle).size(10.0).color(MUTED));
            }
        });
        if let Some(right) = right {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(right).size(10.0).color(MUTED));
            });
        }
    });
}

pub fn stat_card(ui: &mut Ui, label: &str, value: &str, sub: &str) {
    card_frame()
        .inner_margin(Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(label).size(11.0).color(MUTED));
            ui.label(RichText::new(value).size(19.0).strong().color(TEXT));
            ui.label(RichText::new(sub).size(10.0).color(MUTED));
        });
}

pub fn pill(ui: &mut Ui, text: &str, accent: bool, enabled: bool) -> bool {
    let mut button = egui::Button::new(RichText::new(text).size(12.0).color(if accent {
        WHITE
    } else {
        TEXT
    }))
    .min_size(Vec2::new(0.0, 28.0));
    if accent {
        button = button.fill(ACCENT).stroke(Stroke::new(1.0, ACCENT));
    } else {
        button = button.fill(PANEL).stroke(Stroke::new(1.0, BORDER));
    }
    ui.add_enabled(enabled, button).clicked()
}

pub fn segmented(
    ui: &mut Ui,
    options: &[(u32, String)],
    current: Option<u32>,
    enabled: bool,
) -> Option<u32> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (value, label) in options {
            let selected = current == Some(*value);
            let button = egui::Button::new(RichText::new(label).size(12.0))
                .selected(selected)
                .min_size(Vec2::new(0.0, 26.0));
            if ui.add_enabled(enabled, button).clicked() {
                clicked = Some(*value);
            }
        }
    });
    clicked
}

pub fn toggle_row(ui: &mut Ui, label: &str, value: &mut bool, hint: Option<&str>) -> bool {
    let mut changed = false;
    // 类似 CSS :hover：整行悬停时给一层淡红底并显示手型光标。
    let start = ui.cursor().min;
    let row = egui::Rect::from_min_size(
        egui::pos2(ui.max_rect().left(), start.y - 4.0),
        Vec2::new(ui.available_width(), 30.0),
    );
    if ui.rect_contains_pointer(row) {
        // 整行浅色底 + 左侧红色指示条，类似 CSS :hover。
        ui.painter()
            .rect_filled(row, Rounding::same(8.0), HOVER);
        ui.painter().rect_filled(
            egui::Rect::from_min_size(row.min, Vec2::new(2.0, row.height())),
            Rounding::same(1.0),
            ACCENT,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(13.0).color(TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // 自绘开关更像原版：椭圆轨道 + 圆点。
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(42.0, 22.0), Sense::click());
            if resp.clicked() {
                *value = !*value;
                changed = true;
            }
            let on = *value;
            let track = if on {
                ACCENT
            } else if resp.hovered() {
                HOVER
            } else {
                Color32::from_rgb(0x2A, 0x30, 0x3C)
            };
            let painter = ui.painter();
            painter.rect(
                rect,
                Rounding::same(11.0),
                if on { ACCENT_SOFT } else { Color32::TRANSPARENT },
                Stroke::new(1.0, track),
            );
            let radius = 7.0;
            let x = if on {
                rect.right() - 11.0
            } else {
                rect.left() + 11.0
            };
            painter.circle_filled(egui::pos2(x, rect.center().y), radius, track);
        });
    });
    if let Some(hint) = hint {
        ui.label(RichText::new(hint).size(10.0).color(MUTED));
    }
    changed
}

pub fn slider_row(
    ui: &mut Ui,
    label: &str,
    value: &mut u32,
    max: u32,
    suffix: &str,
    enabled: bool,
) -> (bool, bool) {
    let mut changed = false;
    let mut released = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            [108.0, 20.0],
            egui::Label::new(RichText::new(label).size(12.0).color(RED)),
        );
        ui.add_enabled_ui(enabled, |ui| {
            let mut slider = egui::Slider::new(value, 0..=max.max(1)).show_value(true);
            if !suffix.is_empty() {
                slider = slider.suffix(suffix);
            }
            let resp = ui.add(slider);
            if resp.changed() {
                changed = true;
            }
            if resp.drag_stopped() {
                released = true;
            }
        });
    });
    (changed, released)
}

pub fn nav_item(ui: &mut Ui, label: &str, active: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 40.0),
        Sense::click(),
    );
    let fill = if active {
        Color32::from_rgb(0x1E, 0x12, 0x18)
    } else if resp.hovered() {
        CARD
    } else {
        SIDEBAR
    };
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::ZERO, fill);
    if active {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height())),
            Rounding::ZERO,
            ACCENT,
        );
    }
    painter.text(
        rect.left_center() + Vec2::new(18.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        if active { WHITE } else { TEXT_DIM },
    );
    resp.clicked()
}

pub fn mode_card(
    ui: &mut Ui,
    title: &str,
    subtitle: &str,
    active: bool,
    enabled: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, 58.0),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let fill = if active {
        CARD_ACTIVE
    } else if resp.hovered() && enabled {
        HOVER
    } else {
        CARD
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        Rounding::same(8.0),
        fill,
        Stroke::new(1.0, if active { ACCENT } else { BORDER }),
    );
    painter.text(
        rect.left_top() + Vec2::new(12.0, 12.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(13.0),
        if active {
            WHITE
        } else if enabled {
            TEXT
        } else {
            MUTED
        },
    );
    painter.text(
        rect.left_top() + Vec2::new(12.0, 34.0),
        Align2::LEFT_TOP,
        subtitle.to_uppercase(),
        FontId::proportional(9.0),
        MUTED,
    );
    enabled && resp.clicked()
}

/// 准星预览 + 名称按钮。
pub fn crosshair_card(
    ui: &mut Ui,
    value: u32,
    label: &str,
    active: bool,
    enabled: bool,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(96.0, 66.0), if enabled { Sense::click() } else { Sense::hover() });
    let fill = if active {
        CARD_ACTIVE
    } else if resp.hovered() && enabled {
        HOVER
    } else {
        PANEL
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        Rounding::same(8.0),
        fill,
        Stroke::new(1.0, if active { ACCENT } else { BORDER }),
    );
    let color = match value {
        1 | 3 | 5 => Color32::from_rgb(0xFF, 0x4D, 0x5E),
        2 | 4 | 6 | 8 | 10 | 12 => GREEN,
        7 | 9 | 11 => CYAN,
        _ => MUTED,
    };
    let cx = rect.center().x;
    let cy = rect.top() + 24.0;
    match value {
        0 => {
            painter.line_segment(
                [egui::pos2(cx - 8.0, cy), egui::pos2(cx + 8.0, cy)],
                Stroke::new(1.0, MUTED),
            );
        }
        1 | 2 | 7 | 8 => {
            painter.circle_filled(egui::pos2(cx, cy), 2.5, color);
        }
        3 | 4 => {
            painter.circle_stroke(egui::pos2(cx, cy), 7.0, Stroke::new(2.0, color));
        }
        5 | 6 => {
            painter.line_segment(
                [egui::pos2(cx - 9.0, cy), egui::pos2(cx - 3.0, cy)],
                Stroke::new(2.0, color),
            );
            painter.line_segment(
                [egui::pos2(cx + 3.0, cy), egui::pos2(cx + 9.0, cy)],
                Stroke::new(2.0, color),
            );
            painter.line_segment(
                [egui::pos2(cx, cy - 9.0), egui::pos2(cx, cy - 3.0)],
                Stroke::new(2.0, color),
            );
            painter.line_segment(
                [egui::pos2(cx, cy + 3.0), egui::pos2(cx, cy + 9.0)],
                Stroke::new(2.0, color),
            );
        }
        9 | 10 => {
            painter.circle_stroke(egui::pos2(cx, cy), 8.0, Stroke::new(2.0, color));
            painter.circle_filled(egui::pos2(cx, cy), 2.5, color);
        }
        11 | 12 => {
            painter.circle_stroke(egui::pos2(cx, cy), 8.0, Stroke::new(1.0, color));
            painter.line_segment(
                [egui::pos2(cx - 9.0, cy), egui::pos2(cx + 9.0, cy)],
                Stroke::new(2.0, color),
            );
            painter.line_segment(
                [egui::pos2(cx, cy - 9.0), egui::pos2(cx, cy + 9.0)],
                Stroke::new(2.0, color),
            );
        }
        _ => {}
    }
    painter.text(
        egui::pos2(cx, rect.bottom() - 14.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.0),
        if active { WHITE } else { TEXT_DIM },
    );
    enabled && resp.clicked()
}

pub fn status_color(kind: crate::worker::StatusKind) -> Color32 {
    use crate::worker::StatusKind;
    match kind {
        StatusKind::Ok => GREEN,
        StatusKind::Warn => AMBER,
        StatusKind::Error => RED,
        StatusKind::Busy => CYAN,
    }
}

pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}
