//! 后台采样：CPU、内存、磁盘、进程、网卡速率。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use sysinfo::{Disks, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::net::{AppNet, NetSample, PortRow};

pub const HISTORY: usize = 60;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct DiskRow {
    pub mount: String,
    pub total: u64,
    pub used: u64,
    pub percent: f32,
}

#[derive(Clone)]
pub struct IfaceRow {
    pub name: String,
    pub ips: Vec<String>,
    pub rx_bps: u64,
    pub tx_bps: u64,
    pub rx_total: u64,
    pub tx_total: u64,
    pub in_total: bool,
}

#[derive(Clone)]
pub struct ProcRow {
    pub pid: u32,
    pub name: String,
    pub exe: String,
    pub cpu: f32,
    pub mem: u64,
    pub read_bps: u64,
    pub write_bps: u64,
}

#[derive(Clone)]
pub struct Snapshot {
    pub seq: u64,
    pub hostname: String,
    pub os: String,
    pub cpu_brand: String,
    pub logical: usize,
    pub physical: usize,
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub load: [f64; 3],
    pub uptime_secs: u64,
    pub disks: Vec<DiskRow>,
    pub rx_bps: u64,
    pub tx_bps: u64,
    pub ifaces: Vec<IfaceRow>,
    pub procs: Vec<ProcRow>,
    pub apps: Vec<AppNet>,
    pub ports: Vec<PortRow>,
    pub net_error: Option<String>,
    pub cpu_hist: Vec<f32>,
    pub rx_hist: Vec<f32>,
    pub tx_hist: Vec<f32>,
}

pub struct Shared {
    pub latest: Mutex<Option<Snapshot>>,
    pub net: Arc<Mutex<NetSample>>,
    pub nettop_pid: Arc<AtomicU32>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            latest: Mutex::new(None),
            net: Arc::new(Mutex::new(NetSample::default())),
            nettop_pid: Arc::new(AtomicU32::new(0)),
        }
    }
}

pub fn spawn(shared: Arc<Shared>, ctx: egui::Context) {
    thread::Builder::new()
        .name("tipshow-sampler".into())
        .spawn(move || sample_loop(shared, ctx))
        .expect("启动采样线程");
}

fn sample_loop(shared: Arc<Shared>, ctx: egui::Context) {
    let mut sys = System::new_all();
    let mut networks = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let mut prev_net = net_totals(&networks);
    let mut cpu_hist = VecDeque::with_capacity(HISTORY);
    let mut rx_hist = VecDeque::with_capacity(HISTORY);
    let mut tx_hist = VecDeque::with_capacity(HISTORY);
    let mut seq = 0u64;
    let mut prev = Instant::now();

    loop {
        thread::sleep(SAMPLE_INTERVAL);
        let now = Instant::now();
        let elapsed = now.duration_since(prev).as_secs_f64().max(0.25);
        prev = now;

        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_disk_usage()
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
        networks.refresh(true);
        disks.refresh(true);

        let mut snap = build_snapshot(
            &sys,
            &networks,
            &disks,
            &mut prev_net,
            elapsed,
            &mut cpu_hist,
            &mut rx_hist,
            &mut tx_hist,
            &mut seq,
        );
        {
            let net = lock(&shared.net);
            snap.apps = net.apps.clone();
            snap.ports = net.ports.clone();
            snap.net_error = net.error.clone();
        }
        if seq == 1 {
            eprintln!(
                "tipshow: {}  CPU {:.1}%  内存 {} / {}  ↓ {}  ↑ {}  进程 {}",
                snap.hostname,
                snap.cpu,
                fmt_bytes(snap.mem_used),
                fmt_bytes(snap.mem_total),
                fmt_rate(snap.rx_bps),
                fmt_rate(snap.tx_bps),
                snap.procs.len()
            );
        }
        *lock(&shared.latest) = Some(snap);
        ctx.request_repaint();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn net_totals(networks: &Networks) -> HashMap<String, (u64, u64)> {
    networks
        .list()
        .iter()
        .map(|(name, data)| {
            (
                name.clone(),
                (data.total_received(), data.total_transmitted()),
            )
        })
        .collect()
}

fn build_snapshot(
    sys: &System,
    networks: &Networks,
    disks: &Disks,
    prev_net: &mut HashMap<String, (u64, u64)>,
    elapsed: f64,
    cpu_hist: &mut VecDeque<f32>,
    rx_hist: &mut VecDeque<f32>,
    tx_hist: &mut VecDeque<f32>,
    seq: &mut u64,
) -> Snapshot {
    let cores: Vec<f32> = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).collect();
    let cpu = sys.global_cpu_usage();
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .unwrap_or_default();
    let load = System::load_average();

    let (ifaces, rx_bps, tx_bps) = collect_net(networks, prev_net, elapsed);
    push_hist(cpu_hist, cpu);
    push_hist(rx_hist, rx_bps as f32);
    push_hist(tx_hist, tx_bps as f32);

    *seq += 1;
    Snapshot {
        seq: *seq,
        hostname: System::host_name().unwrap_or_else(|| "本机".into()),
        os: System::long_os_version()
            .or_else(System::os_version)
            .or_else(System::name)
            .unwrap_or_default(),
        cpu_brand,
        logical: cores.len(),
        physical: System::physical_core_count().unwrap_or(0),
        cpu,
        cores,
        mem_used: sys.used_memory(),
        mem_total: sys.total_memory(),
        swap_used: sys.used_swap(),
        swap_total: sys.total_swap(),
        load: [load.one, load.five, load.fifteen],
        uptime_secs: System::uptime(),
        disks: collect_disks(disks),
        rx_bps,
        tx_bps,
        ifaces,
        procs: collect_procs(sys, elapsed),
        apps: Vec::new(),
        ports: Vec::new(),
        net_error: None,
        cpu_hist: cpu_hist.iter().copied().collect(),
        rx_hist: rx_hist.iter().copied().collect(),
        tx_hist: tx_hist.iter().copied().collect(),
    }
}

fn collect_net(
    networks: &Networks,
    prev_net: &mut HashMap<String, (u64, u64)>,
    elapsed: f64,
) -> (Vec<IfaceRow>, u64, u64) {
    let mut ifaces = Vec::new();
    let mut rx_bps = 0u64;
    let mut tx_bps = 0u64;
    let mut next = HashMap::new();

    for (name, data) in networks.list() {
        let rx_total = data.total_received();
        let tx_total = data.total_transmitted();
        next.insert(name.clone(), (rx_total, tx_total));
        let (prx, ptx) = prev_net.get(name).copied().unwrap_or((rx_total, tx_total));
        let row_rx = per_sec(rx_total.saturating_sub(prx), elapsed);
        let row_tx = per_sec(tx_total.saturating_sub(ptx), elapsed);
        let in_total = include_in_total(name);
        if in_total {
            rx_bps += row_rx;
            tx_bps += row_tx;
        }
        if is_loopback(name) {
            continue;
        }
        if !in_total && row_rx == 0 && row_tx == 0 && rx_total == 0 && tx_total == 0 {
            continue;
        }
        let ips = data
            .ip_networks()
            .iter()
            .map(|net| net.to_string())
            .filter(|ip| !ip.starts_with("fe80:") && !ip.starts_with("169.254."))
            .take(2)
            .collect();
        ifaces.push(IfaceRow {
            name: name.clone(),
            ips,
            rx_bps: row_rx,
            tx_bps: row_tx,
            rx_total,
            tx_total,
            in_total,
        });
    }

    *prev_net = next;
    ifaces.sort_by(|a, b| {
        b.in_total
            .cmp(&a.in_total)
            .then((b.rx_bps + b.tx_bps).cmp(&(a.rx_bps + a.tx_bps)))
            .then(a.name.cmp(&b.name))
    });
    (ifaces, rx_bps, tx_bps)
}

fn collect_disks(disks: &Disks) -> Vec<DiskRow> {
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy().to_string();
        if !interesting_mount(&mount) || !seen.insert(mount.clone()) {
            continue;
        }
        let total = disk.total_space();
        if total == 0 {
            continue;
        }
        let free = disk.available_space();
        let used = total.saturating_sub(free);
        rows.push(DiskRow {
            mount,
            total,
            used,
            percent: percent(used, total),
        });
    }
    rows.sort_by(|a, b| {
        disk_rank(&a.mount)
            .cmp(&disk_rank(&b.mount))
            .then(b.total.cmp(&a.total))
    });
    let mut unique = Vec::new();
    let mut seen_size = std::collections::HashSet::new();
    for row in rows {
        if seen_size.insert((row.total, row.used)) {
            unique.push(row);
        }
    }
    unique.truncate(4);
    unique
}

fn collect_procs(sys: &System, elapsed: f64) -> Vec<ProcRow> {
    let mut rows = Vec::with_capacity(sys.processes().len());
    for process in sys.processes().values() {
        let name = process.name().to_string_lossy().trim().to_string();
        let name = if name.is_empty() {
            "(无名)".to_string()
        } else {
            name
        };
        let exe = process
            .exe()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        let disk = process.disk_usage();
        rows.push(ProcRow {
            pid: pid_to_u32(process.pid()),
            name,
            exe,
            cpu: process.cpu_usage(),
            mem: process.memory(),
            read_bps: per_sec(disk.read_bytes, elapsed),
            write_bps: per_sec(disk.written_bytes, elapsed),
        });
    }
    rows
}

fn pid_to_u32(pid: Pid) -> u32 {
    pid.as_u32()
}

fn push_hist(buf: &mut VecDeque<f32>, value: f32) {
    if buf.len() >= HISTORY {
        buf.pop_front();
    }
    buf.push_back(value);
}

fn per_sec(bytes: u64, elapsed: f64) -> u64 {
    if elapsed <= 0.0 {
        return 0;
    }
    (bytes as f64 / elapsed).round() as u64
}

pub fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        used as f32 / total as f32 * 100.0
    }
}

pub fn is_loopback(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "lo" || name == "lo0" || name.starts_with("loopback")
}

pub fn include_in_total(name: &str) -> bool {
    if is_loopback(name) {
        return false;
    }
    let name = name.to_ascii_lowercase();
    const SKIP: &[&str] = &[
        "awdl", "llw", "bridge", "gif", "stf", "anpi", "pktap", "vmnet", "veth", "docker", "br-",
        "virbr", "cni", "flannel", "tunl",
    ];
    if SKIP.iter().any(|prefix| name.starts_with(prefix)) {
        return false;
    }
    if name.starts_with("ap") && name.chars().nth(2).is_some_and(|ch| ch.is_ascii_digit()) {
        return false;
    }
    true
}

pub fn interesting_mount(mount: &str) -> bool {
    if mount.is_empty() || mount.starts_with("/dev") {
        return false;
    }
    const SKIP: &[&str] = &[
        "/System/Volumes/Preboot",
        "/System/Volumes/VM",
        "/System/Volumes/Update",
        "/System/Volumes/xarts",
        "/System/Volumes/iSCPreboot",
        "/System/Volumes/Hardware",
        "/System/Volumes/Recovery",
        "/private/var/vm",
    ];
    !SKIP.iter().any(|prefix| mount.starts_with(prefix))
}

fn disk_rank(mount: &str) -> u8 {
    if mount == "/" {
        0
    } else if mount == "/System/Volumes/Data" {
        1
    } else if mount.starts_with("/Volumes/") {
        2
    } else {
        3
    }
}

pub fn fmt_bytes(n: u64) -> String {
    const K: f64 = 1024.0;
    let value = n as f64;
    if value < K {
        format!("{value:.0} B")
    } else if value < K * K {
        format!("{:.0} KB", value / K)
    } else if value < K * K * K {
        format!("{:.0} MB", value / K / K)
    } else {
        format!("{:.1} GB", value / K / K / K)
    }
}

pub fn fmt_rate(bps: u64) -> String {
    const K: f64 = 1024.0;
    let value = bps as f64;
    if value < K {
        format!("{value:.0} B/s")
    } else if value < K * K {
        format!("{:.0} KB/s", value / K)
    } else if value < 10.0 * K * K {
        format!("{:.2} MB/s", value / K / K)
    } else if value < K * K * K {
        format!("{:.1} MB/s", value / K / K)
    } else {
        format!("{:.2} GB/s", value / K / K / K)
    }
}

pub fn fmt_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}天{hours}小时")
    } else if hours > 0 {
        format!("{hours}小时{mins}分")
    } else {
        format!("{mins}分钟")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_and_rates() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "2 KB");
        assert_eq!(fmt_bytes(1024 * 1024), "1 MB");
        assert_eq!(fmt_rate(0), "0 B/s");
        assert_eq!(fmt_rate(2048), "2 KB/s");
        assert_eq!(fmt_rate(1024 * 1024), "1.00 MB/s");
    }

    #[test]
    fn formats_uptime() {
        assert_eq!(fmt_uptime(60), "1分钟");
        assert_eq!(fmt_uptime(3_600 + 120), "1小时2分");
        assert_eq!(fmt_uptime(86_400 + 3_600), "1天1小时");
    }

    #[test]
    fn classifies_interfaces_and_mounts() {
        assert!(is_loopback("lo0"));
        assert!(!include_in_total("awdl0"));
        assert!(!include_in_total("bridge100"));
        assert!(include_in_total("en0"));
        assert!(include_in_total("utun3"));
        assert!(include_in_total("Wi-Fi"));
        assert!(!interesting_mount("/System/Volumes/Preboot"));
        assert!(interesting_mount("/"));
        assert!(interesting_mount("/Volumes/Data"));
    }
}
