//! tipshow 桌面悬浮窗：置顶查看 CPU、内存、进程和实时流量。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod guard;
mod monitor;
mod net;
mod security;

use std::sync::Arc;

use eframe::egui;

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut start_expanded = false;
    let mut start_tab = "总览";
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "\
tipshow 悬浮窗

用法:
  tipshow-float [--expanded] [--tab 总览|进程|流量|端口|安全]

收起时显示 CPU、内存和实时上下行，拖拽移动，点击展开。
展开后可查看进程、每个应用的上传下载、端口、磁盘、杀毒和服务状态。
文档目录、进程注入迹象和启动项会实时监视，发现异常立刻提示。窗口默认置顶。

  --expanded    启动时直接展开详情
  --tab 安全    打开指定页：总览、进程、流量、端口、安全
  Esc           收起
  ⌘Q / Ctrl+Q   退出
"
                );
                return Ok(());
            }
            "--expanded" => start_expanded = true,
            "--tab" => {
                if let Some(name) = args.next() {
                    start_tab = name.leak();
                }
            }
            other => {
                eprintln!("未知参数: {other}（可用 --help）");
                std::process::exit(2);
            }
        }
    }

    let shared = Arc::new(monitor::Shared::new());
    let shared_bg = Arc::clone(&shared);
    let viewport = viewport(start_expanded);
    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "tipshow",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            install_style(&cc.egui_ctx);
            monitor::spawn(Arc::clone(&shared_bg), cc.egui_ctx.clone());
            net::spawn(
                Arc::clone(&shared_bg.net),
                Arc::clone(&shared_bg.nettop_pid),
                cc.egui_ctx.clone(),
            );
            security::spawn(Arc::clone(&shared_bg), cc.egui_ctx.clone());
            guard::spawn(Arc::clone(&shared_bg), cc.egui_ctx.clone());
            Ok(Box::new(app::FloatApp::new(
                cc,
                shared,
                start_expanded,
                start_tab,
            )))
        }),
    )
}

fn viewport(start_expanded: bool) -> egui::ViewportBuilder {
    let size = if start_expanded {
        app::EXPANDED
    } else {
        app::COMPACT
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("tipshow")
        .with_inner_size(size)
        .with_min_inner_size(app::COMPACT)
        .with_transparent(true)
        .with_has_shadow(false)
        .with_resizable(start_expanded)
        .with_always_on_top()
        .with_taskbar(true);
    #[cfg(target_os = "macos")]
    {
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_titlebar_buttons_shown(false);
    }
    #[cfg(not(target_os = "macos"))]
    {
        viewport = viewport.with_decorations(false);
    }
    viewport
}

fn install_fonts(ctx: &egui::Context) {
    let Some(bytes) = load_cjk_font() else {
        eprintln!("tipshow: 未找到中文字体，界面中文可能显示为方框");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.insert(0, "cjk".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

fn load_cjk_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJK-VF.ttc",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() {
                eprintln!("tipshow: 使用字体 {path}");
                return Some(bytes);
            }
        }
    }
    None
}

fn install_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(app::TEXT);
    visuals.window_fill = egui::Color32::TRANSPARENT;
    visuals.panel_fill = egui::Color32::TRANSPARENT;
    visuals.extreme_bg_color = egui::Color32::from_rgb(14, 16, 22);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_white_alpha(16);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_white_alpha(28);
    visuals.widgets.active.bg_fill = egui::Color32::from_white_alpha(40);
    visuals.widgets.inactive.fg_stroke.color = app::TEXT;
    visuals.widgets.hovered.fg_stroke.color = app::TEXT;
    visuals.widgets.noninteractive.fg_stroke.color = app::TEXT;
    visuals.selection.bg_fill = egui::Color32::from_rgb(16, 120, 92);
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    });
}
