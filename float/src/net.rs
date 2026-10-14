//! 用 macOS `nettop` 采集每个进程的上下行，以及连接上的本地/远端端口。

use eframe::egui;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppNet {
    pub pid: u32,
    pub name: String,
    pub rx_bps: u64,
    pub tx_bps: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortRow {
    pub pid: u32,
    pub name: String,
    pub proto: String,
    pub local: String,
    pub local_port: String,
    pub remote: String,
    pub remote_port: String,
    pub state: String,
    pub rx_bps: u64,
    pub tx_bps: u64,
}

#[derive(Clone, Debug, Default)]
pub struct NetSample {
    pub apps: Vec<AppNet>,
    pub ports: Vec<PortRow>,
    pub error: Option<String>,
}

pub fn spawn(slot: Arc<Mutex<NetSample>>, pid_slot: Arc<AtomicU32>, ctx: egui::Context) {
    thread::Builder::new()
        .name("tipshow-nettop".into())
        .spawn(move || loop {
            match run_nettop(&slot, &pid_slot, &ctx) {
                Ok(()) => {
                    set_error(&slot, "nettop 已退出，正在重试");
                }
                Err(err) => set_error(&slot, &err),
            }
            ctx.request_repaint();
            thread::sleep(Duration::from_secs(2));
        })
        .expect("启动 nettop 线程");
}

fn set_error(slot: &Mutex<NetSample>, message: &str) {
    let mut guard = slot.lock().unwrap_or_else(|err| err.into_inner());
    guard.error = Some(message.to_string());
}

fn run_nettop(
    slot: &Mutex<NetSample>,
    pid_slot: &AtomicU32,
    ctx: &egui::Context,
) -> Result<(), String> {
    let mut child = Command::new("nettop")
        .args([
            "-n",
            "-x",
            "-d",
            "-L",
            "0",
            "-s",
            "1",
            "-J",
            "bytes_in,bytes_out,state",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("无法启动 nettop（应用流量和端口需要它）: {err}"))?;
    pid_slot.store(child.id(), Ordering::Relaxed);
    let stdout = child.stdout.take().ok_or("nettop 没有标准输出")?;
    let result = read_nettop(stdout, slot, ctx);
    pid_slot.store(0, Ordering::Relaxed);
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn read_nettop(
    stdout: impl std::io::Read,
    slot: &Mutex<NetSample>,
    ctx: &egui::Context,
) -> Result<(), String> {
    let reader = BufReader::new(stdout);
    let mut header: Option<String> = None;
    let mut block = Vec::new();
    let mut first = true;
    let mut started = Instant::now();

    for line in reader.lines() {
        let line = line.map_err(|err| err.to_string())?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with(',') {
            if let Some(prev) = header.take() {
                if !block.is_empty() {
                    let elapsed = started.elapsed().as_secs_f64().max(0.25);
                    let sample = finish_block(&prev, &block, !first, elapsed);
                    *slot.lock().unwrap_or_else(|err| err.into_inner()) = sample;
                    ctx.request_repaint();
                    first = false;
                    block.clear();
                }
            }
            header = Some(line.to_string());
            started = Instant::now();
            continue;
        }
        block.push(line.to_string());
    }
    Err("nettop 输出结束".into())
}

pub fn finish_block(header: &str, lines: &[String], is_delta: bool, elapsed: f64) -> NetSample {
    let columns: Vec<&str> = header.split(',').collect();
    let state_at = columns.iter().position(|name| *name == "state");
    let rx_at = columns.iter().position(|name| *name == "bytes_in");
    let tx_at = columns.iter().position(|name| *name == "bytes_out");
    let (Some(rx_at), Some(tx_at)) = (rx_at, tx_at) else {
        return NetSample {
            error: Some("nettop 输出里没有 bytes_in/bytes_out".into()),
            ..NetSample::default()
        };
    };

    let mut apps: Vec<AppNet> = Vec::new();
    let mut ports: Vec<PortRow> = Vec::new();
    let mut current: Option<(String, u32)> = None;

    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();
        let label = fields.first().copied().unwrap_or("").trim();
        if label.is_empty() {
            continue;
        }
        let rx = field_u64(&fields, rx_at);
        let tx = field_u64(&fields, tx_at);
        let (rx, tx) = if is_delta {
            (per_sec(rx, elapsed), per_sec(tx, elapsed))
        } else {
            (0, 0)
        };
        if label.contains("<->") {
            let Some((name, pid)) = current.clone() else {
                continue;
            };
            let state = state_at
                .and_then(|index| fields.get(index))
                .copied()
                .unwrap_or("")
                .trim()
                .to_string();
            if let Some(mut row) = parse_connection(label, &name, pid, &state, rx, tx) {
                if row.state.is_empty() && row.remote_port == "*" && row.proto == "TCP" {
                    row.state = "Listen".into();
                }
                if let Some(existing) = ports.iter_mut().find(|item| same_socket(item, &row)) {
                    existing.rx_bps += row.rx_bps;
                    existing.tx_bps += row.tx_bps;
                } else {
                    ports.push(row);
                }
            }
            continue;
        }
        if let Some((name, pid)) = parse_process(label) {
            current = Some((name.clone(), pid));
            if let Some(existing) = apps.iter_mut().find(|app| app.pid == pid) {
                existing.rx_bps += rx;
                existing.tx_bps += tx;
            } else {
                apps.push(AppNet {
                    pid,
                    name,
                    rx_bps: rx,
                    tx_bps: tx,
                });
            }
        }
    }

    apps.sort_by(|a, b| {
        (b.rx_bps + b.tx_bps)
            .cmp(&(a.rx_bps + a.tx_bps))
            .then(a.name.cmp(&b.name))
    });
    ports.sort_by(|a, b| {
        (b.rx_bps + b.tx_bps)
            .cmp(&(a.rx_bps + a.tx_bps))
            .then(a.local_port.cmp(&b.local_port))
            .then(a.name.cmp(&b.name))
    });
    if ports.len() > 500 {
        ports.truncate(500);
    }
    NetSample {
        apps,
        ports,
        error: None,
    }
}

fn same_socket(left: &PortRow, right: &PortRow) -> bool {
    left.pid == right.pid
        && left.proto == right.proto
        && left.local_port == right.local_port
        && left.remote == right.remote
        && left.state == right.state
}

fn field_u64(fields: &[&str], index: usize) -> u64 {
    fields
        .get(index)
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

fn per_sec(bytes: u64, elapsed: f64) -> u64 {
    if elapsed <= 0.0 {
        return 0;
    }
    (bytes as f64 / elapsed).round() as u64
}

fn parse_process(label: &str) -> Option<(String, u32)> {
    let (name, pid) = label.rsplit_once('.')?;
    let pid = pid.parse().ok()?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), pid))
}

fn parse_connection(
    label: &str,
    name: &str,
    pid: u32,
    state: &str,
    rx: u64,
    tx: u64,
) -> Option<PortRow> {
    let (left, remote_raw) = label.split_once("<->")?;
    let (proto_raw, local_raw) = left.split_once(' ')?;
    let proto = proto_name(proto_raw).to_string();
    let (local_host, local_port) = split_endpoint(local_raw);
    let (remote_host, remote_port) = split_endpoint(remote_raw);
    Some(PortRow {
        pid,
        name: name.to_string(),
        proto,
        local: format_addr(&local_host, &local_port),
        local_port,
        remote: format_addr(&remote_host, &remote_port),
        remote_port,
        state: state.to_string(),
        rx_bps: rx,
        tx_bps: tx,
    })
}

fn proto_name(raw: &str) -> &str {
    let raw = raw.to_ascii_lowercase();
    if raw.starts_with("tcp") {
        "TCP"
    } else if raw.starts_with("udp") {
        "UDP"
    } else if raw.starts_with("quic") {
        "QUIC"
    } else {
        // leak: return owned via leak is bad. Caller only uses known prefixes.
        "其他"
    }
}

fn split_endpoint(addr: &str) -> (String, String) {
    let addr = addr.trim();
    if addr.is_empty() || addr == "*" || addr == "*.*" {
        return ("*".into(), "*".into());
    }
    if let Some((host, rest)) = addr.split_once('%') {
        let port = rest.rsplit_once('.').map(|(_, port)| port).unwrap_or("*");
        return (host.to_string(), port.to_string());
    }
    if addr.matches(':').count() == 1 {
        let (host, port) = addr.rsplit_once(':').unwrap();
        return (host.to_string(), port.to_string());
    }
    if let Some((host, port)) = addr.rsplit_once('.') {
        if !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) {
            return (host.to_string(), port.to_string());
        }
    }
    (addr.to_string(), "*".into())
}

fn format_addr(host: &str, port: &str) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else if host == "*" {
        format!("*:{port}")
    } else {
        format!("{host}:{port}")
    }
}

pub fn kill_nettop(pid: u32) {
    if pid == 0 {
        return;
    }
    let _ = Command::new("kill").arg(pid.to_string()).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_process_rates_and_ports() {
        let header = ",state,bytes_in,bytes_out,";
        let lines = vec![
            "mDNSResponder.469,,10,1,".into(),
            "udp4 *:5353<->*:*,,10,1,".into(),
            "udp4 *:5353<->*:*,,4,0,".into(),
            "Google Chrome H.16956,,3000,20,".into(),
            "tcp4 10.0.0.8:50000<->1.2.3.4:443,Established,3000,20,".into(),
            "tcp6 fe80::abcd%en0.61615<->fe80::1234%en0.443,Established,8,2,".into(),
            "node.1606,,0,0,".into(),
            "tcp4 *:3000<->*:*,Listen,0,0,".into(),
        ];
        let sample = finish_block(header, &lines, true, 1.0);
        assert!(sample.error.is_none());
        assert_eq!(sample.apps.len(), 3);
        let chrome = sample.apps.iter().find(|app| app.pid == 16956).unwrap();
        assert_eq!(chrome.name, "Google Chrome H");
        assert_eq!(chrome.rx_bps, 3000);
        assert_eq!(chrome.tx_bps, 20);
        let mdns = sample.ports.iter().find(|row| row.pid == 469).unwrap();
        assert_eq!(mdns.proto, "UDP");
        assert_eq!(mdns.local_port, "5353");
        assert_eq!(mdns.rx_bps, 14);
        let web = sample
            .ports
            .iter()
            .find(|row| row.pid == 16956 && row.proto == "TCP")
            .unwrap();
        assert_eq!(web.local, "10.0.0.8:50000");
        assert_eq!(web.remote, "1.2.3.4:443");
        assert_eq!(web.state, "Established");
        let v6 = sample
            .ports
            .iter()
            .find(|row| row.proto == "TCP" && row.local_port == "61615");
        let v6 = v6.unwrap();
        assert_eq!(v6.remote_port, "443");
        assert!(v6.local.starts_with("[fe80::abcd]"));
        let listen = sample.ports.iter().find(|row| row.pid == 1606).unwrap();
        assert_eq!(listen.local_port, "3000");
        assert_eq!(listen.state, "Listen");
    }

    #[test]
    fn first_sample_is_not_treated_as_rate() {
        let lines = vec!["Chrome.1,,999999999,10,".into()];
        let sample = finish_block(",bytes_in,bytes_out,", &lines, false, 1.0);
        assert_eq!(sample.apps[0].rx_bps, 0);
    }
}
