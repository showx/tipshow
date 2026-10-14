//! 悬浮窗界面：收起为网速胶囊，展开后看进程和流量。

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, PointerButton, RichText, Sense,
    Stroke, UiBuilder, Vec2,
};

use crate::monitor::{self, Snapshot};

pub const COMPACT: Vec2 = vec2(318.0, 78.0);
pub const EXPANDED: Vec2 = vec2(560.0, 680.0);
const EXPANDED_MIN: Vec2 = vec2(480.0, 520.0);

pub const TEXT: Color32 = Color32::from_rgb(236, 240, 245);
const MUTED: Color32 = Color32::from_rgb(148, 158, 173);
const BG: Color32 = Color32::from_rgba_unmultiplied_const(18, 22, 30, 242);
const CARD: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 14);
const STROKE: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 32);
const DOWN: Color32 = Color32::from_rgb(52, 211, 153);
const UP: Color32 = Color32::from_rgb(96, 165, 250);
const CPU: Color32 = Color32::from_rgb(251, 191, 36);
const MEM: Color32 = Color32::from_rgb(196, 181, 253);
const DISK: Color32 = Color32::from_rgb(251, 146, 60);

const COL_CPU: f32 = 72.0;
const COL_MEM: f32 = 76.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Procs,
    Net,
    Ports,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppSort {
    Down,
    Up,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PortFilter {
    Active,
    Listen,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Cpu,
    Mem,
    Name,
}

pub struct FloatApp {
    shared: Arc<monitor::Shared>,
    snap: Option<Snapshot>,
    seen: u64,
    expanded: bool,
    pinned: bool,
    apply_size: bool,
    apply_level: bool,
    placed: bool,
    expanded_size: Vec2,
    tab: Tab,
    sort: SortKey,
    query: String,
    net_query: String,
    app_sort: AppSort,
    port_filter: PortFilter,
}

impl FloatApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        shared: Arc<monitor::Shared>,
        start_expanded: bool,
        start_tab: &str,
    ) -> Self {
        let (pinned, placed) = restore(cc);
        let tab = match start_tab {
            "进程" | "procs" => Tab::Procs,
            "流量" | "net" => Tab::Net,
            "端口" | "ports" => Tab::Ports,
            _ => Tab::Overview,
        };
        Self {
            shared,
            snap: None,
            seen: 0,
            expanded: start_expanded,
            pinned,
            apply_size: true,
            apply_level: true,
            placed,
            expanded_size: EXPANDED,
            tab,
            sort: SortKey::Cpu,
            query: String::new(),
            net_query: String::new(),
            app_sort: AppSort::Down,
            port_filter: PortFilter::Active,
        }
    }

    fn sync(&mut self) {
        let guard = self
            .shared
            .latest
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if let Some(snap) = guard.as_ref() {
            if snap.seq != self.seen {
                self.seen = snap.seq;
                self.snap = Some(snap.clone());
            }
        }
    }

    fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
        self.apply_size = true;
    }

    fn toggle_pinned(&mut self) {
        self.pinned = !self.pinned;
        self.apply_level = true;
    }
}

impl eframe::App for FloatApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let pid = self
            .shared
            .nettop_pid
            .load(std::sync::atomic::Ordering::Relaxed);
        crate::net::kill_nettop(pid);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("placed", if self.placed { "1" } else { "0" }.into());
        storage.set_string("pinned", if self.pinned { "1" } else { "0" }.into());
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync();
        if self.snap.is_none() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        let quit = ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::Q));
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let collapse = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        if collapse && self.expanded {
            self.expanded = false;
            self.apply_size = true;
        }

        if self.apply_size {
            let size = if self.expanded {
                self.expanded_size
            } else {
                COMPACT
            };
            let min = if self.expanded { EXPANDED_MIN } else { COMPACT };
            let max = if self.expanded {
                vec2(1100.0, 1400.0)
            } else {
                COMPACT
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min));
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(max));
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(self.expanded));
            self.apply_size = false;
        }
        if self.apply_level {
            let level = if self.pinned {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            self.apply_level = false;
        }
        if self.expanded && !self.apply_size {
            if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
                let size = rect.size();
                if size.x >= EXPANDED_MIN.x && size.y >= EXPANDED_MIN.y {
                    self.expanded_size = size;
                }
            }
        }
        if !self.placed {
            if let Some(monitor) = ctx.input(|input| input.viewport().monitor_size) {
                let width = if self.expanded {
                    self.expanded_size.x
                } else {
                    COMPACT.x
                };
                let x = (monitor.x - width - 28.0).max(16.0);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos2(x, 72.0)));
                self.placed = true;
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let rect = ui.max_rect();
        let radius = if self.expanded { 16 } else { 39 };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(radius), BG);
        painter.rect_stroke(
            rect,
            CornerRadius::same(radius),
            Stroke::new(1.0, STROKE),
            egui::StrokeKind::Inside,
        );

        let inner = rect.shrink(if self.expanded { 12.0 } else { 0.0 });
        if self.expanded {
            let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
            self.expanded_ui(&mut child);
        } else {
            self.compact_ui(ui, rect);
        }
        ui.expand_to_include_rect(rect);
    }
}

impl FloatApp {
    fn compact_ui(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let response = ui.interact(rect, Id::new("compact-body"), Sense::click_and_drag());
        if response.drag_started_by(PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if response.clicked() {
            self.toggle_expanded();
        }
        response.context_menu(|ui| self.context_menu(ui));
        response
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text("点击展开详情，拖拽移动。右键可置顶或退出");

        let painter = ui.painter();
        let center_y = rect.center().y;
        if let Some(snap) = &self.snap {
            painter.circle_filled(pos2(rect.left() + 22.0, center_y), 4.0, DOWN);
            painter.text(
                pos2(rect.left() + 36.0, center_y - 12.0),
                Align2::LEFT_CENTER,
                format!("↓ {}", monitor::fmt_rate(snap.rx_bps)),
                FontId::proportional(16.0),
                DOWN,
            );
            painter.text(
                pos2(rect.left() + 36.0, center_y + 12.0),
                Align2::LEFT_CENTER,
                format!("↑ {}", monitor::fmt_rate(snap.tx_bps)),
                FontId::proportional(16.0),
                UP,
            );
            let divider_x = rect.right() - 108.0;
            painter.vline(
                divider_x,
                (center_y - 16.0)..=(center_y + 16.0),
                Stroke::new(1.0, STROKE),
            );
            painter.text(
                pos2(rect.right() - 16.0, center_y - 12.0),
                Align2::RIGHT_CENTER,
                format!("CPU {:.0}%", snap.cpu),
                FontId::proportional(13.0),
                usage_color(snap.cpu, CPU),
            );
            let mem = monitor::percent(snap.mem_used, snap.mem_total);
            painter.text(
                pos2(rect.right() - 16.0, center_y + 12.0),
                Align2::RIGHT_CENTER,
                format!("内存 {mem:.0}%"),
                FontId::proportional(13.0),
                usage_color(mem, MEM),
            );
        } else {
            painter.text(
                pos2(rect.left() + 20.0, center_y - 10.0),
                Align2::LEFT_CENTER,
                "tipshow",
                FontId::proportional(16.0),
                TEXT,
            );
            painter.text(
                pos2(rect.left() + 20.0, center_y + 12.0),
                Align2::LEFT_CENTER,
                "正在采集…",
                FontId::proportional(13.0),
                MUTED,
            );
        }
    }

    fn expanded_ui(&mut self, ui: &mut egui::Ui) {
        self.title_bar(ui);
        self.subtitle(ui);
        ui.add_space(4.0);
        self.tab_bar(ui);
        ui.add_space(8.0);
        match self.tab {
            Tab::Overview => self.overview(ui),
            Tab::Procs => self.procs(ui),
            Tab::Net => self.traffic(ui),
            Tab::Ports => self.ports(ui),
        }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let pin_label = if self.pinned { "已置顶" } else { "置顶" };
            let actions = 168.0;
            let drag_width = (ui.available_width() - actions).max(40.0);
            let (drag_rect, drag) =
                ui.allocate_exact_size(vec2(drag_width, 28.0), Sense::click_and_drag());
            if drag.drag_started_by(PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                self.toggle_expanded();
            }
            drag.context_menu(|ui| self.context_menu(ui));
            ui.painter().text(
                drag_rect.left_center(),
                Align2::LEFT_CENTER,
                "tipshow",
                FontId::proportional(16.0),
                TEXT,
            );
            if tiny_button(ui, pin_label, self.pinned).clicked() {
                self.toggle_pinned();
            }
            if tiny_button(ui, "收起", false).clicked() {
                self.toggle_expanded();
            }
            if tiny_button(ui, "关闭", false).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn subtitle(&self, ui: &mut egui::Ui) {
        let Some(snap) = &self.snap else {
            ui.label(RichText::new("正在采集系统数据…").color(MUTED).size(12.0));
            return;
        };
        let line = format!(
            "{}  ·  {}  ·  已运行 {}  ·  负载 {:.2} {:.2} {:.2}",
            snap.hostname,
            snap.os,
            monitor::fmt_uptime(snap.uptime_secs),
            snap.load[0],
            snap.load[1],
            snap.load[2]
        );
        ui.add(egui::Label::new(RichText::new(line).color(MUTED).size(11.5)).truncate());
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (tab, label) in [
                (Tab::Overview, "总览"),
                (Tab::Procs, "进程"),
                (Tab::Net, "流量"),
                (Tab::Ports, "端口"),
            ] {
                if tab_button(ui, label, self.tab == tab).clicked() {
                    self.tab = tab;
                }
            }
        });
    }

    fn overview(&self, ui: &mut egui::Ui) {
        let Some(snap) = &self.snap else {
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt("overview")
            .auto_shrink([false, false])
            .max_height(ui.available_height())
            .show(ui, |ui| {
                stat_row(ui, snap);
                ui.add_space(8.0);
                section_label(
                    ui,
                    &format!(
                        "CPU 核心  ·  {} 逻辑 / {} 物理",
                        snap.logical, snap.physical
                    ),
                );
                core_bars(ui, &snap.cores);
                cpu_sparkline(ui, &snap.cpu_hist);
                if !snap.cpu_brand.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&snap.cpu_brand).color(MUTED).size(11.0))
                            .truncate(),
                    );
                }
                ui.add_space(8.0);
                section_label(ui, "实时流量");
                speed_hero(ui, snap);
                dual_sparkline(ui, &snap.rx_hist, &snap.tx_hist);
                ui.add_space(6.0);
                section_label(ui, "应用上传下载");
                top_apps(ui, snap, 5);
                ui.add_space(8.0);
                section_label(ui, "磁盘");
                if snap.disks.is_empty() {
                    ui.label(RichText::new("没有读到磁盘").color(MUTED));
                }
                for disk in &snap.disks {
                    meter_row(
                        ui,
                        &disk.mount,
                        disk.percent,
                        &format!(
                            "{} / {}",
                            monitor::fmt_bytes(disk.used),
                            monitor::fmt_bytes(disk.total)
                        ),
                        usage_color(disk.percent, DISK),
                    );
                }
                if snap.swap_total > 0 {
                    let pct = monitor::percent(snap.swap_used, snap.swap_total);
                    meter_row(
                        ui,
                        "Swap",
                        pct,
                        &format!(
                            "{} / {}",
                            monitor::fmt_bytes(snap.swap_used),
                            monitor::fmt_bytes(snap.swap_total)
                        ),
                        MEM,
                    );
                }
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!("每秒刷新  ·  Esc 收起  ·  {} 退出", quit_hint()))
                        .color(MUTED)
                        .size(11.0),
                );
            });
    }

    fn procs(&mut self, ui: &mut egui::Ui) {
        let Some(snap) = &self.snap else {
            ui.label(RichText::new("正在采集进程…").color(MUTED));
            return;
        };
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 168.0).max(80.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("搜索名称、路径或 PID")
                    .desired_width(width),
            );
            if sort_button(ui, "CPU", self.sort == SortKey::Cpu).clicked() {
                self.sort = SortKey::Cpu;
            }
            if sort_button(ui, "内存", self.sort == SortKey::Mem).clicked() {
                self.sort = SortKey::Mem;
            }
            if sort_button(ui, "名称", self.sort == SortKey::Name).clicked() {
                self.sort = SortKey::Name;
            }
        });

        let mut rows: Vec<&monitor::ProcRow> = snap
            .procs
            .iter()
            .filter(|row| proc_matches(&self.query, row))
            .collect();
        rows.sort_by(|a, b| match self.sort {
            SortKey::Cpu => b.cpu.total_cmp(&a.cpu).then(a.pid.cmp(&b.pid)),
            SortKey::Mem => b.mem.cmp(&a.mem).then(a.pid.cmp(&b.pid)),
            SortKey::Name => a
                .name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
                .then(a.pid.cmp(&b.pid)),
        });
        ui.label(
            RichText::new(format!("显示 {} / {}", rows.len(), snap.procs.len()))
                .color(MUTED)
                .size(11.0),
        );
        proc_header(ui);

        let row_h = 28.0;
        egui::ScrollArea::vertical()
            .id_salt("procs")
            .auto_shrink([false, false])
            .max_height(ui.available_height())
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                for idx in range {
                    proc_row(ui, rows[idx], row_h);
                }
            });
    }

    fn traffic(&mut self, ui: &mut egui::Ui) {
        let Some(snap) = &self.snap else {
            ui.label(RichText::new("正在采集流量…").color(MUTED));
            return;
        };
        speed_hero(ui, snap);
        ui.add_space(4.0);
        if let Some(err) = &snap.net_error {
            ui.label(
                RichText::new(err)
                    .color(Color32::from_rgb(248, 113, 113))
                    .size(12.0),
            );
        }
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 120.0).max(80.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.net_query)
                    .hint_text("搜索应用或 PID")
                    .desired_width(width),
            );
            if sort_button(ui, "下载", self.app_sort == AppSort::Down).clicked() {
                self.app_sort = AppSort::Down;
            }
            if sort_button(ui, "上传", self.app_sort == AppSort::Up).clicked() {
                self.app_sort = AppSort::Up;
            }
        });
        let rows = visible_apps(snap, &self.net_query, self.app_sort);
        ui.label(
            RichText::new(format!("有流量的应用 {}", rows.len()))
                .color(MUTED)
                .size(11.0),
        );
        let row_h = 30.0;
        let list_h = (ui.available_height() - 150.0).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("apps")
            .auto_shrink([false, false])
            .max_height(list_h)
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                for idx in range {
                    app_row(ui, snap, rows[idx], row_h);
                }
            });
        if rows.is_empty() {
            ui.label(
                RichText::new("这一秒没有应用在传数据，或仍在等待 nettop 的第二次采样。")
                    .color(MUTED)
                    .size(12.0),
            );
        }
        ui.add_space(6.0);
        section_label(ui, "网卡");
        egui::ScrollArea::vertical()
            .id_salt("net")
            .auto_shrink([false, false])
            .max_height(ui.available_height())
            .show(ui, |ui| {
                for iface in &snap.ifaces {
                    iface_row(ui, iface);
                }
            });
    }

    fn ports(&mut self, ui: &mut egui::Ui) {
        let Some(snap) = &self.snap else {
            ui.label(RichText::new("正在采集端口…").color(MUTED));
            return;
        };
        if let Some(err) = &snap.net_error {
            ui.label(
                RichText::new(err)
                    .color(Color32::from_rgb(248, 113, 113))
                    .size(12.0),
            );
        }
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 180.0).max(80.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.net_query)
                    .hint_text("搜索程序、端口或地址")
                    .desired_width(width),
            );
            if sort_button(ui, "活跃", self.port_filter == PortFilter::Active).clicked() {
                self.port_filter = PortFilter::Active;
            }
            if sort_button(ui, "监听", self.port_filter == PortFilter::Listen).clicked() {
                self.port_filter = PortFilter::Listen;
            }
            if sort_button(ui, "全部", self.port_filter == PortFilter::All).clicked() {
                self.port_filter = PortFilter::All;
            }
        });
        let rows = visible_ports(snap, &self.net_query, self.port_filter);
        ui.label(
            RichText::new(format!("显示 {} 条连接 / 端口", rows.len()))
                .color(MUTED)
                .size(11.0),
        );
        let row_h = 46.0;
        egui::ScrollArea::vertical()
            .id_salt("ports")
            .auto_shrink([false, false])
            .max_height(ui.available_height())
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                for idx in range {
                    port_row(ui, snap, rows[idx], row_h);
                }
            });
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        let expand_label = if self.expanded { "收起" } else { "展开" };
        if ui.button(expand_label).clicked() {
            self.toggle_expanded();
            ui.close();
        }
        let pin_label = if self.pinned {
            "取消置顶"
        } else {
            "置顶"
        };
        if ui.button(pin_label).clicked() {
            self.toggle_pinned();
            ui.close();
        }
        if ui.button("退出").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            ui.close();
        }
    }
}

fn restore(cc: &eframe::CreationContext<'_>) -> (bool, bool) {
    let Some(storage) = cc.storage else {
        return (true, false);
    };
    let pinned = storage
        .get_string("pinned")
        .map(|value| value == "1")
        .unwrap_or(true);
    let placed = storage.get_string("placed").as_deref() == Some("1");
    (pinned, placed)
}

fn tiny_button(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
    let color = if active { DOWN } else { MUTED };
    ui.add(
        egui::Button::new(RichText::new(text).size(12.5).color(color))
            .frame(false)
            .min_size(vec2(36.0, 24.0)),
    )
}

fn tab_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let fill = if selected {
        Color32::from_rgba_unmultiplied(52, 211, 153, 40)
    } else {
        Color32::TRANSPARENT
    };
    let color = if selected { DOWN } else { MUTED };
    ui.add(
        egui::Button::new(RichText::new(text).size(13.0).color(color))
            .fill(fill)
            .corner_radius(CornerRadius::same(8))
            .min_size(vec2(68.0, 28.0)),
    )
}

fn sort_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let color = if selected { TEXT } else { MUTED };
    ui.add(
        egui::Button::new(RichText::new(text).size(12.0).color(color))
            .frame(false)
            .min_size(vec2(40.0, 24.0)),
    )
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(MUTED).size(12.0));
}

fn stat_row(ui: &mut egui::Ui, snap: &Snapshot) {
    let gap = 8.0;
    let width = ((ui.available_width() - gap * 2.0) / 3.0).max(40.0);
    let mem = monitor::percent(snap.mem_used, snap.mem_total);
    let (disk_pct, disk_sub) = snap
        .disks
        .first()
        .map(|disk| {
            (
                disk.percent,
                format!(
                    "{} / {}",
                    monitor::fmt_bytes(disk.used),
                    monitor::fmt_bytes(disk.total)
                ),
            )
        })
        .unwrap_or((0.0, "—".into()));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        stat_card(
            ui,
            width,
            "CPU",
            &format!("{:.0}%", snap.cpu),
            snap.cpu,
            usage_color(snap.cpu, CPU),
            &format!("{} 核", snap.logical.max(snap.physical)),
        );
        stat_card(
            ui,
            width,
            "内存",
            &format!("{mem:.0}%"),
            mem,
            usage_color(mem, MEM),
            &format!(
                "{} / {}",
                monitor::fmt_bytes(snap.mem_used),
                monitor::fmt_bytes(snap.mem_total)
            ),
        );
        stat_card(
            ui,
            width,
            "磁盘",
            &format!("{disk_pct:.0}%"),
            disk_pct,
            usage_color(disk_pct, DISK),
            &disk_sub,
        );
    });
}

fn stat_card(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    value: &str,
    pct: f32,
    color: Color32,
    sub: &str,
) {
    let height = 100.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), CARD);
    let inner = rect.shrink(8.0);
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_width(inner.width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).color(MUTED).size(11.5));
            ui.label(RichText::new(value).color(color).size(22.0));
            thin_bar(ui, pct, color);
            ui.add(egui::Label::new(RichText::new(sub).color(MUTED).size(10.5)).truncate());
        },
    );
}

fn thin_bar(ui: &mut egui::Ui, pct: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), Color32::from_white_alpha(20));
    let mut fill = rect;
    fill.max.x = rect.min.x + rect.width() * (pct / 100.0).clamp(0.0, 1.0);
    if fill.width() > 0.5 {
        painter.rect_filled(fill, CornerRadius::same(3), color);
    }
}

fn meter_row(ui: &mut egui::Ui, label: &str, pct: f32, right: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let right_w = 148.0;
    let name_rect =
        egui::Rect::from_min_max(rect.left_top(), pos2(rect.right() - right_w, rect.bottom()));
    ui.painter_at(name_rect).text(
        name_rect.left_center(),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.5),
        TEXT,
    );
    ui.painter().text(
        rect.right_center(),
        Align2::RIGHT_CENTER,
        right,
        FontId::proportional(11.5),
        MUTED,
    );
    thin_bar(ui, pct, color);
    ui.add_space(3.0);
}

fn core_bars(ui: &mut egui::Ui, cores: &[f32]) {
    if cores.is_empty() {
        return;
    }
    let height = 28.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let n = cores.len() as f32;
    let gap = 3.0;
    let width = ((rect.width() - gap * (n - 1.0)) / n).max(1.0);
    let painter = ui.painter();
    for (index, value) in cores.iter().enumerate() {
        let x = rect.min.x + index as f32 * (width + gap);
        let bar = egui::Rect::from_min_size(pos2(x, rect.min.y), vec2(width, height));
        painter.rect_filled(bar, CornerRadius::same(2), Color32::from_white_alpha(16));
        let frac = (value / 100.0).clamp(0.0, 1.0);
        let mut fill = bar;
        fill.min.y = bar.max.y - bar.height() * frac;
        painter.rect_filled(fill, CornerRadius::same(2), usage_color(*value, CPU));
    }
    response.on_hover_text(format!("{} 个逻辑核心", cores.len()));
}

fn cpu_sparkline(ui: &mut egui::Ui, values: &[f32]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    spark_line(ui, rect.shrink2(vec2(0.0, 2.0)), values, 100.0, CPU);
}

fn speed_hero(ui: &mut egui::Ui, snap: &Snapshot) {
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 8.0) / 2.0;
        speed_card(ui, width, "下载", &monitor::fmt_rate(snap.rx_bps), DOWN);
        speed_card(ui, width, "上传", &monitor::fmt_rate(snap.tx_bps), UP);
    });
}

fn speed_card(ui: &mut egui::Ui, width: f32, title: &str, value: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 64.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), CARD);
    let inner = rect.shrink(10.0);
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_width(inner.width());
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(title).color(MUTED).size(11.5));
            ui.label(RichText::new(value).color(color).size(18.0));
        },
    );
}

fn dual_sparkline(ui: &mut egui::Ui, down: &[f32], up: &[f32]) {
    let height = 42.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(8), CARD);
    let plot = rect.shrink(6.0);
    let max = down
        .iter()
        .chain(up.iter())
        .copied()
        .fold(1.0_f32, f32::max)
        .max(1.0);
    spark_line(ui, plot, down, max, DOWN);
    spark_line(ui, plot, up, max, UP);
}

fn spark_line(ui: &mut egui::Ui, rect: egui::Rect, values: &[f32], max: f32, color: Color32) {
    if values.len() < 2 {
        return;
    }
    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let x = rect.min.x + rect.width() * index as f32 / (values.len() - 1) as f32;
            let y = rect.max.y - rect.height() * (value / max).clamp(0.0, 1.0);
            pos2(x, y)
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(1.6, color)));
}

fn proc_header(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let painter = ui.painter();
    painter.text(
        rect.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        "进程",
        FontId::proportional(11.5),
        MUTED,
    );
    painter.text(
        pos2(rect.right() - COL_MEM - 8.0, rect.center().y),
        Align2::RIGHT_CENTER,
        "CPU",
        FontId::proportional(11.5),
        MUTED,
    );
    painter.text(
        pos2(rect.right() - 4.0, rect.center().y),
        Align2::RIGHT_CENTER,
        "内存",
        FontId::proportional(11.5),
        MUTED,
    );
}

fn proc_row(ui: &mut egui::Ui, row: &monitor::ProcRow, height: f32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(12));
    }
    let name = format!("{}  {}", row.name, row.pid);
    let name_rect = egui::Rect::from_min_max(
        rect.left_top(),
        pos2(rect.right() - COL_CPU - COL_MEM, rect.bottom()),
    );
    ui.painter_at(name_rect).text(
        name_rect.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.0),
        TEXT,
    );
    ui.painter().text(
        pos2(rect.right() - COL_MEM - 8.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("{:.1}%", row.cpu),
        FontId::proportional(12.5),
        usage_color(row.cpu, TEXT),
    );
    ui.painter().text(
        pos2(rect.right() - 4.0, rect.center().y),
        Align2::RIGHT_CENTER,
        monitor::fmt_bytes(row.mem),
        FontId::proportional(12.5),
        TEXT,
    );
    response.on_hover_ui(|ui| {
        ui.label(RichText::new(format!("{}  ({})", row.name, row.pid)).strong());
        if !row.exe.is_empty() {
            ui.label(RichText::new(&row.exe).color(MUTED).size(11.0));
        }
        ui.label(format!(
            "CPU {:.1}%   内存 {}",
            row.cpu,
            monitor::fmt_bytes(row.mem)
        ));
        ui.label(format!(
            "磁盘读 {}   写 {}",
            monitor::fmt_rate(row.read_bps),
            monitor::fmt_rate(row.write_bps)
        ));
    });
}

fn iface_row(ui: &mut egui::Ui, iface: &monitor::IfaceRow) {
    let height = if iface.ips.is_empty() { 36.0 } else { 50.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(8), CARD);
    let painter = ui.painter();
    let name_color = if iface.in_total { TEXT } else { MUTED };
    painter.text(
        pos2(rect.left() + 10.0, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        &iface.name,
        FontId::proportional(13.0),
        name_color,
    );
    painter.text(
        pos2(rect.right() - 10.0, rect.top() + 14.0),
        Align2::RIGHT_CENTER,
        format!(
            "↓ {}    ↑ {}",
            monitor::fmt_rate(iface.rx_bps),
            monitor::fmt_rate(iface.tx_bps)
        ),
        FontId::proportional(12.5),
        TEXT,
    );
    let detail = if iface.ips.is_empty() {
        format!(
            "累计 ↓ {}   ↑ {}",
            monitor::fmt_bytes(iface.rx_total),
            monitor::fmt_bytes(iface.tx_total)
        )
    } else {
        format!(
            "{}   ·   累计 ↓ {}  ↑ {}",
            iface.ips.join("  "),
            monitor::fmt_bytes(iface.rx_total),
            monitor::fmt_bytes(iface.tx_total)
        )
    };
    painter.text(
        pos2(rect.left() + 10.0, rect.bottom() - 12.0),
        Align2::LEFT_CENTER,
        detail,
        FontId::proportional(11.0),
        MUTED,
    );
    ui.add_space(4.0);
}

fn top_apps(ui: &mut egui::Ui, snap: &Snapshot, limit: usize) {
    let rows = visible_apps(snap, "", AppSort::Down);
    if rows.is_empty() {
        let text = snap
            .net_error
            .as_deref()
            .unwrap_or("正在读取每个应用的上传下载…");
        ui.label(RichText::new(text).color(MUTED).size(12.0));
        return;
    }
    for row in rows.into_iter().take(limit) {
        app_row(ui, snap, row, 28.0);
    }
}

fn visible_apps<'a>(snap: &'a Snapshot, query: &str, sort: AppSort) -> Vec<&'a crate::net::AppNet> {
    let mut rows: Vec<_> = snap
        .apps
        .iter()
        .filter(|row| app_matches(query, row))
        .filter(|row| !query.trim().is_empty() || row.rx_bps + row.tx_bps > 0)
        .collect();
    rows.sort_by(|a, b| match sort {
        AppSort::Down => b.rx_bps.cmp(&a.rx_bps).then(b.tx_bps.cmp(&a.tx_bps)),
        AppSort::Up => b.tx_bps.cmp(&a.tx_bps).then(b.rx_bps.cmp(&a.rx_bps)),
    });
    rows
}

fn app_matches(query: &str, row: &crate::net::AppNet) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let needle = query.to_ascii_lowercase();
    row.name.to_ascii_lowercase().contains(&needle) || row.pid.to_string().contains(query)
}

fn app_row(ui: &mut egui::Ui, snap: &Snapshot, row: &crate::net::AppNet, height: f32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(12));
    }
    let name = proc_name(snap, row.pid, &row.name);
    let name_rect =
        egui::Rect::from_min_max(rect.left_top(), pos2(rect.right() - 196.0, rect.bottom()));
    ui.painter_at(name_rect).text(
        name_rect.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        format!("{name}  {}", row.pid),
        FontId::proportional(13.0),
        TEXT,
    );
    ui.painter().text(
        pos2(rect.right() - 104.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("↓ {}", monitor::fmt_rate(row.rx_bps)),
        FontId::proportional(12.5),
        DOWN,
    );
    ui.painter().text(
        pos2(rect.right() - 4.0, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("↑ {}", monitor::fmt_rate(row.tx_bps)),
        FontId::proportional(12.5),
        UP,
    );
}

fn visible_ports<'a>(
    snap: &'a Snapshot,
    query: &str,
    filter: PortFilter,
) -> Vec<&'a crate::net::PortRow> {
    let mut rows: Vec<_> = snap
        .ports
        .iter()
        .filter(|row| port_matches(query, row) && port_in_filter(row, filter))
        .collect();
    rows.sort_by(|a, b| {
        (b.rx_bps + b.tx_bps)
            .cmp(&(a.rx_bps + a.tx_bps))
            .then(a.local_port.cmp(&b.local_port))
            .then(a.name.cmp(&b.name))
    });
    rows
}

fn port_in_filter(row: &crate::net::PortRow, filter: PortFilter) -> bool {
    match filter {
        PortFilter::All => true,
        PortFilter::Listen => is_listen(row),
        PortFilter::Active => is_listen(row) || row.rx_bps + row.tx_bps > 0 || is_connected(row),
    }
}

fn is_listen(row: &crate::net::PortRow) -> bool {
    row.state.eq_ignore_ascii_case("listen")
}

fn is_connected(row: &crate::net::PortRow) -> bool {
    matches!(
        row.state.to_ascii_lowercase().as_str(),
        "established"
            | "synsent"
            | "synrecv"
            | "closewait"
            | "finwait1"
            | "finwait2"
            | "timewait"
            | "lastack"
    )
}

fn port_matches(query: &str, row: &crate::net::PortRow) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let needle = query.to_ascii_lowercase();
    row.name.to_ascii_lowercase().contains(&needle)
        || row.local.to_ascii_lowercase().contains(&needle)
        || row.remote.to_ascii_lowercase().contains(&needle)
        || row.local_port.contains(query)
        || row.remote_port.contains(query)
        || row.proto.to_ascii_lowercase().contains(&needle)
        || row.pid.to_string() == query
}

fn port_row(ui: &mut egui::Ui, snap: &Snapshot, row: &crate::net::PortRow, height: f32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), Color32::from_white_alpha(12));
    }
    let name = proc_name(snap, row.pid, &row.name);
    let state = state_text(&row.state);
    let name_rect = egui::Rect::from_min_max(
        rect.left_top(),
        pos2(rect.right() - 72.0, rect.top() + height * 0.55),
    );
    ui.painter_at(name_rect).text(
        pos2(rect.left() + 6.0, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        format!("{name}  {}", row.pid),
        FontId::proportional(13.0),
        TEXT,
    );
    ui.painter().text(
        pos2(rect.right() - 6.0, rect.top() + 14.0),
        Align2::RIGHT_CENTER,
        state,
        FontId::proportional(12.0),
        state_color(&row.state),
    );
    let path_rect = egui::Rect::from_min_max(
        pos2(rect.left(), rect.top() + height * 0.45),
        pos2(rect.right() - 188.0, rect.bottom()),
    );
    ui.painter_at(path_rect).text(
        pos2(rect.left() + 6.0, rect.bottom() - 12.0),
        Align2::LEFT_CENTER,
        format!("{}  {}  →  {}", row.proto, row.local, row.remote),
        FontId::proportional(11.5),
        MUTED,
    );
    ui.painter().text(
        pos2(rect.right() - 6.0, rect.bottom() - 12.0),
        Align2::RIGHT_CENTER,
        format!(
            "↓ {}  ↑ {}",
            monitor::fmt_rate(row.rx_bps),
            monitor::fmt_rate(row.tx_bps)
        ),
        FontId::proportional(11.0),
        TEXT,
    );
}

fn proc_name(snap: &Snapshot, pid: u32, fallback: &str) -> String {
    snap.procs
        .iter()
        .find(|row| row.pid == pid)
        .map(|row| row.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn state_text(state: &str) -> String {
    match state.to_ascii_lowercase().as_str() {
        "established" => "已连接".into(),
        "listen" => "监听".into(),
        "closewait" => "关闭中".into(),
        "timewait" => "等待".into(),
        "synsent" | "synrecv" => "握手".into(),
        "finwait1" | "finwait2" | "lastack" => "结束".into(),
        "" => "—".into(),
        _ => state.to_string(),
    }
}

fn state_color(state: &str) -> Color32 {
    match state.to_ascii_lowercase().as_str() {
        "established" => DOWN,
        "listen" => UP,
        _ => MUTED,
    }
}

fn proc_matches(query: &str, row: &monitor::ProcRow) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let needle = query.to_ascii_lowercase();
    row.name.to_ascii_lowercase().contains(&needle)
        || row.exe.to_ascii_lowercase().contains(&needle)
        || row.pid.to_string().contains(query)
}

fn usage_color(pct: f32, base: Color32) -> Color32 {
    if pct >= 90.0 {
        Color32::from_rgb(248, 113, 113)
    } else if pct >= 75.0 {
        CPU
    } else {
        base
    }
}

fn quit_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘Q"
    } else {
        "Ctrl+Q"
    }
}
