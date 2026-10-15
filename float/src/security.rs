//! 杀毒软件和关键服务检测。macOS 读系统防护开关和 launchd，Windows 读安全中心和服务。

#[cfg(any(target_os = "macos", test))]
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::monitor::Shared;

const POLL: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    On,
    Off,
    /// Windows 防火墙多个配置文件开关不一致时使用。
    #[allow(dead_code)]
    Mixed,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckRow {
    pub name: String,
    pub health: Health,
    pub status: String,
    pub detail: String,
    pub critical: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SecuritySample {
    pub av: Vec<CheckRow>,
    pub guards: Vec<CheckRow>,
    pub services: Vec<CheckRow>,
    pub error: Option<String>,
    pub ready: bool,
}

pub fn spawn(shared: Arc<Shared>, ctx: egui::Context) {
    thread::Builder::new()
        .name("tipshow-security".into())
        .spawn(move || loop {
            let sample = collect();
            *lock(&shared.security) = sample;
            ctx.request_repaint();
            thread::sleep(POLL);
        })
        .expect("启动安全检测线程");
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn collect() -> SecuritySample {
    #[cfg(target_os = "macos")]
    {
        return collect_macos();
    }
    #[cfg(target_os = "windows")]
    {
        return collect_windows();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        SecuritySample {
            error: Some("当前系统暂不检测杀毒软件和服务".into()),
            ready: true,
            ..SecuritySample::default()
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows", test)),
    allow(dead_code)
)]
enum RowKind {
    Guard,
    Service,
}

#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows", test)),
    allow(dead_code)
)]
fn status_label(kind: RowKind, health: Health) -> String {
    match (kind, health) {
        (_, Health::Mixed) => "部分开启".into(),
        (_, Health::Unknown) => "未知".into(),
        (RowKind::Service, Health::On) => "运行中".into(),
        (RowKind::Service, Health::Off) => "未运行".into(),
        (_, Health::On) => "已开启".into(),
        (_, Health::Off) => "未开启".into(),
    }
}

#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows", test)),
    allow(dead_code)
)]
fn row(
    kind: RowKind,
    name: &str,
    health: Health,
    detail: impl Into<String>,
    critical: bool,
) -> CheckRow {
    CheckRow {
        name: name.to_string(),
        health,
        status: status_label(kind, health),
        detail: detail.into(),
        critical,
    }
}

#[cfg(target_os = "macos")]
fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(cmd);
    command.args(args).stdin(Stdio::null());
    hide_window(&mut command);
    let out = command
        .output()
        .map_err(|err| format!("无法执行 {cmd}: {err}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        text = String::from_utf8_lossy(&out.stderr).trim().to_string();
    }
    if text.is_empty() && !out.status.success() {
        return Err(format!("{cmd} 退出码 {}", out.status.code().unwrap_or(-1)));
    }
    Ok(text)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn hide_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}

#[cfg(target_os = "macos")]
fn health_from_phrases(text: &str) -> Health {
    let text = text.to_ascii_lowercase();
    if text.contains("state = 0")
        || text.contains("disabled")
        || text.contains("filevault is off")
        || text.contains(" is off")
    {
        return Health::Off;
    }
    if text.contains("state = 1")
        || text.contains("enabled")
        || text.contains("filevault is on")
        || text.contains(" is on")
    {
        return Health::On;
    }
    Health::Unknown
}

#[cfg(target_os = "macos")]
fn collect_macos() -> SecuritySample {
    let mut errors = Vec::new();
    let procs = process_names();
    let extensions = run("systemextensionsctl", &["list"]).unwrap_or_default();
    let labels = match launchd_labels() {
        Ok(labels) => labels,
        Err(err) => {
            errors.push(err);
            HashMap::new()
        }
    };
    let app_names = dir_names("/Applications");
    let mut av = vec![xprotect(&labels)];
    av.extend(third_party(&procs, &extensions, &app_names));
    let guards = vec![
        guard_cmd(
            "应用程序防火墙",
            "/usr/libexec/ApplicationFirewall/socketfilterfw",
            &["--getglobalstate"],
            |health, raw| match health {
                Health::On => "正在拦截未经允许的入站连接".into(),
                Health::Off => "入站防火墙已关闭".into(),
                _ => raw,
            },
        ),
        guard_cmd(
            "Gatekeeper",
            "spctl",
            &["--status"],
            |health, raw| match health {
                Health::On => "会检查已下载应用的开发者签名".into(),
                Health::Off => "已关闭应用来源检查".into(),
                _ => raw,
            },
        ),
        guard_cmd(
            "系统完整性保护",
            "csrutil",
            &["status"],
            |health, raw| {
                let custom = raw.to_ascii_lowercase().contains("custom");
                match health {
                    Health::On if custom => "已开启，当前是自定义配置".into(),
                    Health::On => "系统关键位置受到保护".into(),
                    Health::Off => "系统完整性保护已关闭".into(),
                    _ => raw,
                }
            },
        ),
        guard_cmd(
            "FileVault",
            "fdesetup",
            &["status"],
            |health, raw| match health {
                Health::On => "启动磁盘已加密".into(),
                Health::Off => "启动磁盘未加密".into(),
                _ => raw,
            },
        ),
    ];
    SecuritySample {
        av,
        guards,
        services: macos_services(&labels),
        error: if errors.is_empty() {
            None
        } else {
            Some(errors.join("；"))
        },
        ready: true,
    }
}

#[cfg(target_os = "macos")]
fn guard_cmd(
    name: &str,
    cmd: &str,
    args: &[&str],
    detail_for: impl Fn(Health, String) -> String,
) -> CheckRow {
    match run(cmd, args) {
        Ok(text) => {
            let health = health_from_phrases(&text);
            let raw = text
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim();
            row(
                RowKind::Guard,
                name,
                health,
                detail_for(health, raw.to_string()),
                true,
            )
        }
        Err(err) => row(RowKind::Guard, name, Health::Unknown, err, true),
    }
}

#[cfg(target_os = "macos")]
fn xprotect(labels: &HashMap<String, bool>) -> CheckRow {
    const BUNDLES: &[&str] = &[
        "/Library/Apple/System/Library/CoreServices/XProtect.bundle",
        "/System/Library/CoreServices/XProtect.bundle",
    ];
    const APPS: &[&str] = &[
        "/Library/Apple/System/Library/CoreServices/XProtect.app",
        "/System/Library/CoreServices/XProtect.app",
    ];
    let bundle = BUNDLES
        .iter()
        .copied()
        .find(|path| Path::new(path).exists());
    let app = APPS.iter().copied().find(|path| Path::new(path).exists());
    let mut parts = Vec::new();
    if let Some(path) = bundle {
        if let Some(version) = bundle_version(path) {
            parts.push(format!("特征库 {version}"));
        }
    }
    if let Some(path) = app {
        if let Some(version) = bundle_version(path) {
            parts.push(format!("修复工具 {version}"));
        }
    }
    if labels
        .get("com.apple.XProtect.daemon.scan")
        .copied()
        .unwrap_or(false)
    {
        parts.push("后台扫描正在运行".into());
    }
    let present = bundle.is_some() || app.is_some();
    let health = if present { Health::On } else { Health::Off };
    let detail = if parts.is_empty() {
        if present {
            "系统自带恶意软件防护".to_string()
        } else {
            "未找到 XProtect".to_string()
        }
    } else {
        parts.join("  ·  ")
    };
    row(RowKind::Guard, "XProtect", health, detail, true)
}

#[cfg(target_os = "macos")]
fn bundle_version(bundle: &str) -> Option<String> {
    let plist = format!("{bundle}/Contents/Info.plist");
    let text = run("defaults", &["read", &plist, "CFBundleShortVersionString"]).ok()?;
    let version = text.trim();
    if version.is_empty() || version.contains("does not exist") {
        None
    } else {
        Some(version.to_string())
    }
}

#[cfg(target_os = "macos")]
struct AvSig {
    name: &'static str,
    paths: &'static [&'static str],
    app_prefixes: &'static [&'static str],
    procs: &'static [&'static str],
    extensions: &'static [&'static str],
}

#[cfg(target_os = "macos")]
fn third_party(procs: &[String], extensions: &str, app_names: &[String]) -> Vec<CheckRow> {
    const SIGS: &[AvSig] = &[
        AvSig {
            name: "Microsoft Defender",
            paths: &[
                "/Applications/Microsoft Defender.app",
                "/Library/Application Support/Microsoft/Defender",
            ],
            app_prefixes: &["Microsoft Defender"],
            procs: &["wdavdaemon", "wdaved"],
            extensions: &["com.microsoft.wdav", "com.microsoft.defender"],
        },
        AvSig {
            name: "CrowdStrike Falcon",
            paths: &[
                "/Applications/Falcon.app",
                "/Library/CS/falcond",
                "/Library/CS/falconctl",
            ],
            app_prefixes: &["Falcon"],
            procs: &["falcond", "falcon-sensor", "falconctl"],
            extensions: &["crowdstrike"],
        },
        AvSig {
            name: "SentinelOne",
            paths: &["/Library/Sentinel", "/Applications/SentinelOne"],
            app_prefixes: &["Sentinel"],
            procs: &["sentineld", "sentinelagent", "sentinelagenthelper"],
            extensions: &["sentinelone"],
        },
        AvSig {
            name: "Sophos",
            paths: &["/Library/Sophos Anti-Virus", "/Applications/Sophos"],
            app_prefixes: &["Sophos"],
            procs: &["sophos", "sophosscand", "sophosantivirus"],
            extensions: &["sophos"],
        },
        AvSig {
            name: "Malwarebytes",
            paths: &["/Applications/Malwarebytes.app"],
            app_prefixes: &["Malwarebytes"],
            procs: &["rtprotectiondaemon", "malwarebytes"],
            extensions: &["malwarebytes"],
        },
        AvSig {
            name: "Bitdefender",
            paths: &["/Library/Bitdefender"],
            app_prefixes: &["Bitdefender"],
            procs: &["bdagent", "bdservice", "bdcoreissues"],
            extensions: &["bitdefender"],
        },
        AvSig {
            name: "Norton",
            paths: &["/Library/Norton"],
            app_prefixes: &["Norton"],
            procs: &["norton", "symdaemon"],
            extensions: &["norton", "symantec"],
        },
        AvSig {
            name: "Kaspersky",
            paths: &[],
            app_prefixes: &["Kaspersky"],
            procs: &["kavd", "kaspersky"],
            extensions: &["kaspersky"],
        },
        AvSig {
            name: "Avast",
            paths: &["/Library/Application Support/Avast"],
            app_prefixes: &["Avast"],
            procs: &["avast"],
            extensions: &["avast"],
        },
        AvSig {
            name: "AVG",
            paths: &["/Library/Application Support/AVG"],
            app_prefixes: &["AVG"],
            procs: &["avg"],
            extensions: &["avg"],
        },
        AvSig {
            name: "ESET",
            paths: &["/Library/Application Support/ESET"],
            app_prefixes: &["ESET"],
            procs: &["esets", "eset"],
            extensions: &["eset"],
        },
        AvSig {
            name: "ClamAV",
            paths: &[
                "/Applications/ClamXAV.app",
                "/opt/homebrew/opt/clamav",
                "/usr/local/clamav",
            ],
            app_prefixes: &["ClamXAV", "ClamAV"],
            procs: &["clamd", "freshclam", "clamxav"],
            extensions: &[],
        },
        AvSig {
            name: "Trend Micro",
            paths: &["/Library/Application Support/TrendMicro"],
            app_prefixes: &["Trend Micro"],
            procs: &["icoreservice", "trendmicro"],
            extensions: &["trendmicro"],
        },
        AvSig {
            name: "Carbon Black",
            paths: &["/Applications/VMware Carbon Black Cloud"],
            app_prefixes: &["VMware Carbon Black", "Carbon Black"],
            procs: &["cbagentd", "repmgr"],
            extensions: &["carbonblack", "confer"],
        },
        AvSig {
            name: "Cortex XDR",
            paths: &[],
            app_prefixes: &["Cortex XDR", "Traps"],
            procs: &["trapsd", "cortex-xdr", "cyserver"],
            extensions: &["paloaltonetworks", "cortex"],
        },
        AvSig {
            name: "Cisco Secure Endpoint",
            paths: &["/opt/cisco/amp"],
            app_prefixes: &["Cisco Secure Endpoint"],
            procs: &["ampdaemon"],
            extensions: &["cisco.amp"],
        },
        AvSig {
            name: "McAfee",
            paths: &["/Library/McAfee"],
            app_prefixes: &["McAfee", "Trellix"],
            procs: &["mcafee", "vshieldscanner", "trellix"],
            extensions: &["mcafee", "trellix"],
        },
        AvSig {
            name: "Webroot",
            paths: &[],
            app_prefixes: &["Webroot"],
            procs: &["wrcore", "webroot"],
            extensions: &["webroot"],
        },
        AvSig {
            name: "Intego",
            paths: &[],
            app_prefixes: &["Intego", "VirusBarrier"],
            procs: &["virusbarrier", "intego"],
            extensions: &["intego"],
        },
        AvSig {
            name: "Avira",
            paths: &[],
            app_prefixes: &["Avira"],
            procs: &["avguard", "avira"],
            extensions: &["avira"],
        },
        AvSig {
            name: "FortiClient",
            paths: &["/Library/Application Support/Fortinet"],
            app_prefixes: &["FortiClient", "FortiEDR"],
            procs: &["fctserv", "fortiedr"],
            extensions: &["fortinet", "forticlient"],
        },
        AvSig {
            name: "Cybereason",
            paths: &[],
            app_prefixes: &["Cybereason"],
            procs: &["cybereason", "activeprobe"],
            extensions: &["cybereason"],
        },
        AvSig {
            name: "Cylance",
            paths: &[],
            app_prefixes: &["Cylance"],
            procs: &["cylancesvc"],
            extensions: &["cylance"],
        },
        AvSig {
            name: "Jamf Protect",
            paths: &["/Library/Application Support/JamfProtect"],
            app_prefixes: &["Jamf Protect"],
            procs: &["jamfprotect"],
            extensions: &["jamfprotect"],
        },
    ];

    let mut found = Vec::new();
    for sig in SIGS {
        let path = installed_path(sig, app_names);
        let running = proc_running(procs, sig.procs);
        let extension = extension_state(extensions, sig.extensions);
        if path.is_none() && !running && extension.is_none() {
            continue;
        }
        let health = if running || extension == Some(true) {
            Health::On
        } else {
            Health::Off
        };
        let detail = match (running, extension, path.as_deref()) {
            (true, _, _) => "防护进程正在运行",
            (_, Some(true), _) => "系统扩展已启用",
            (_, _, Some(_)) => "已安装，防护进程未运行",
            _ => "已发现安装痕迹，当前未运行",
        };
        found.push(row(RowKind::Guard, sig.name, health, detail, true));
    }
    found
}

#[cfg(target_os = "macos")]
fn installed_path(sig: &AvSig, app_names: &[String]) -> Option<String> {
    for path in sig.paths {
        if Path::new(path).exists() {
            return Some((*path).to_string());
        }
    }
    for name in app_names {
        let lower = name.to_ascii_lowercase();
        if sig
            .app_prefixes
            .iter()
            .any(|prefix| lower.starts_with(&prefix.to_ascii_lowercase()))
        {
            return Some(format!("/Applications/{name}"));
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn dir_names(dir: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

#[cfg(target_os = "macos")]
fn process_names() -> Vec<String> {
    run("ps", &["-ax", "-o", "comm="])
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn proc_running(procs: &[String], needles: &[&str]) -> bool {
    procs.iter().any(|comm| {
        let base = comm
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(comm)
            .to_ascii_lowercase();
        needles.iter().any(|needle| {
            let needle = needle.to_ascii_lowercase();
            if needle.len() <= 4 {
                base == needle
            } else {
                base == needle || base.starts_with(&needle)
            }
        })
    })
}

#[cfg(any(target_os = "macos", test))]
fn extension_state(text: &str, needles: &[&str]) -> Option<bool> {
    if needles.is_empty() {
        return None;
    }
    let mut found = None;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !needles
            .iter()
            .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
        {
            continue;
        }
        let active = lower.contains("activated enabled") || lower.trim_start().starts_with('*');
        if active {
            return Some(true);
        }
        found = Some(false);
    }
    found
}

#[cfg(target_os = "macos")]
fn macos_services(labels: &HashMap<String, bool>) -> Vec<CheckRow> {
    const SERVICES: &[(&str, &[&str], bool)] = &[
        ("远程登录", &["com.openssh.sshd"], false),
        ("屏幕共享", &["com.apple.screensharing"], false),
        ("隔空投送", &["com.apple.sharingd"], false),
        ("XProtect 扫描", &["com.apple.XProtect.daemon.scan"], true),
        ("应用防火墙服务", &["com.apple.alf"], true),
        ("软件更新", &["com.apple.softwareupdated"], true),
        ("Time Machine", &["com.apple.backupd"], false),
        ("Spotlight", &["com.apple.metadata.mds"], true),
        ("蓝牙", &["com.apple.bluetoothd"], false),
        ("Wi-Fi", &["com.apple.airportd"], false),
        ("定位服务", &["com.apple.locationd"], false),
        (
            "Bonjour",
            &[
                "com.apple.mDNSResponder",
                "com.apple.mDNSResponder.reloaded",
            ],
            false,
        ),
        ("iCloud", &["com.apple.cloudd"], false),
        ("推送通知", &["com.apple.apsd"], false),
        ("安全服务", &["com.apple.securityd"], true),
        ("窗口服务", &["com.apple.WindowServer"], true),
        ("文件事件", &["com.apple.fseventsd"], false),
        ("电源管理", &["com.apple.powerd"], false),
        ("系统日志", &["com.apple.syslogd"], false),
        ("配置服务", &["com.apple.configd"], false),
        ("时间同步", &["com.apple.timed"], false),
        ("信任评估", &["com.apple.trustd"], true),
        ("隐私权限", &["com.apple.tccd"], false),
        ("控制中心", &["com.apple.controlcenter"], false),
        ("访达", &["com.apple.Finder"], false),
    ];
    SERVICES
        .iter()
        .map(|(name, keys, critical)| {
            let (health, detail) = service_from_labels(labels, keys);
            row(RowKind::Service, name, health, detail, *critical)
        })
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn service_from_labels(labels: &HashMap<String, bool>, keys: &[&str]) -> (Health, String) {
    let mut seen = None;
    for key in keys {
        if let Some(running) = labels.get(*key) {
            if *running {
                return (Health::On, (*key).to_string());
            }
            seen = Some(*key);
        }
    }
    match seen {
        Some(key) => (Health::Off, key.to_string()),
        None => (Health::Off, "未加载".into()),
    }
}

#[cfg(target_os = "macos")]
fn launchd_labels() -> Result<HashMap<String, bool>, String> {
    let mut labels = HashMap::new();
    let system = run("launchctl", &["print", "system"])?;
    note_labels(&mut labels, &system);
    if let Some(uid) = current_uid() {
        if let Ok(user) = run("launchctl", &["print", &format!("gui/{uid}")]) {
            note_labels(&mut labels, &user);
        }
    }
    Ok(labels)
}

#[cfg(target_os = "macos")]
fn current_uid() -> Option<u32> {
    run("id", &["-u"]).ok()?.trim().parse().ok()
}

#[cfg(any(target_os = "macos", test))]
fn note_labels(labels: &mut HashMap<String, bool>, text: &str) {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(pid) = parts.next() else { continue };
        let Some(status) = parts.next() else { continue };
        let Some(label) = parts.next() else { continue };
        if parts.next().is_some() || !label.contains('.') {
            continue;
        }
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let status_ok = status == "-"
            || status.starts_with('(')
            || status.chars().all(|ch| ch.is_ascii_digit());
        if !status_ok {
            continue;
        }
        let running = pid > 0;
        labels
            .entry(label.to_string())
            .and_modify(|value| *value = *value || running)
            .or_insert(running);
    }
}

#[cfg(target_os = "windows")]
fn collect_windows() -> SecuritySample {
    let script = WINDOWS_SCRIPT;
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_window(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return SecuritySample {
                error: Some(format!("无法启动 PowerShell: {err}")),
                ready: true,
                ..SecuritySample::default()
            };
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(script.as_bytes());
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(err) => {
            return SecuritySample {
                error: Some(format!("PowerShell 没有返回结果: {err}")),
                ready: true,
                ..SecuritySample::default()
            };
        }
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut sample = parse_windows_report(&text);
    if sample.av.is_empty() && sample.guards.is_empty() && sample.services.is_empty() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if sample.error.is_none() {
            sample.error = Some(if err.is_empty() {
                "没有读到杀毒软件和服务状态".into()
            } else {
                err
            });
        }
    }
    sample.ready = true;
    sample
}

#[cfg(target_os = "windows")]
const WINDOWS_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$utf8 = New-Object System.Text.UTF8Encoding $false
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8

function Line([string]$k, [string]$n, [string]$s, [string]$d) {
  if ($null -eq $n) { $n = '' }
  if ($null -eq $d) { $d = '' }
  $n = ($n -replace "[`r`n`t]", ' ').Trim()
  $d = ($d -replace "[`r`n`t]", ' ').Trim()
  Write-Output "$k`t$n`t$s`t$d"
}

try {
  $products = @(Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct)
} catch {
  $products = @()
}
$saw = $false
foreach ($p in $products) {
  if ($null -eq $p -or [string]::IsNullOrWhiteSpace([string]$p.displayName)) { continue }
  $saw = $true
  $state = [uint32]0
  try { $state = [uint32]$p.productState } catch {}
  $mid = ($state -shr 8) -band 255
  $sig = $state -band 255
  $on = ($mid -eq 16 -or $mid -eq 17)
  $fresh = ($sig -eq 0)
  $flag = if ($on) { 'on' } else { 'off' }
  $code = if ($on -and $fresh) { 'realtime fresh' } elseif ($on) { 'realtime stale' } elseif ($fresh) { 'off fresh' } else { 'off stale' }
  $exe = ''
  try { $exe = [string]$p.pathToSignedProductExe } catch {}
  Line 'AV' ([string]$p.displayName) $flag "$code|$exe"
}
if (-not $saw) {
  $wd = Get-Service -Name WinDefend -ErrorAction SilentlyContinue
  if ($null -ne $wd) {
    $flag = if ([string]$wd.Status -eq 'Running') { 'on' } else { 'off' }
    Line 'AV' 'Microsoft Defender' $flag 'service only|'
  }
}

try {
  $fw = New-Object -ComObject HNetCfg.FwPolicy2
  $pairs = @(
    @{ id = 1; key = 'domain' },
    @{ id = 2; key = 'private' },
    @{ id = 4; key = 'public' }
  )
  $bits = @()
  $onCount = 0
  foreach ($pair in $pairs) {
    $enabled = $false
    try { $enabled = [bool]$fw.FirewallEnabled([int]$pair.id) } catch {}
    if ($enabled) { $onCount++; $bits += ($pair.key + '=on') } else { $bits += ($pair.key + '=off') }
  }
  $st = if ($onCount -eq 3) { 'on' } elseif ($onCount -eq 0) { 'off' } else { 'mixed' }
  Line 'GUARD' 'firewall' $st ($bits -join ',')
} catch {
  Line 'ERR' 'firewall' 'unknown' ([string]$_.Exception.Message)
}

$names = @(
  'WinDefend','WdNisSvc','MDCoreSvc','SecurityHealthService','wscsvc','Sense','MpsSvc','BFE',
  'wuauserv','BITS','EventLog','Winmgmt','Dhcp','Dnscache','WlanSvc','BthServ','Spooler',
  'LanmanServer','TermService','WinRM','RemoteRegistry','W32Time','WSearch','SysMain','Schedule',
  'Audiosrv','CryptSvc','nsi','NlaSvc','iphlpsvc','PolicyAgent','RemoteAccess','SharedAccess',
  'SSDPSRV','WerSvc'
)
$always = @('WinDefend','MpsSvc','BFE','wscsvc','SecurityHealthService','EventLog','wuauserv')
$found = @{}
foreach ($svc in @(Get-Service -ErrorAction SilentlyContinue)) {
  if ($null -eq $svc) { continue }
  $found[$svc.Name.ToLowerInvariant()] = $svc
}
foreach ($name in $names) {
  $key = $name.ToLowerInvariant()
  if ($found.ContainsKey($key)) {
    $s = $found[$key]
    $run = if ([string]$s.Status -eq 'Running') { 'run' } else { 'stop' }
    $start = ''
    try { $start = [string]$s.StartType } catch {}
    $disp = ''
    try { $disp = [string]$s.DisplayName } catch {}
    if ([string]$s.Status -eq 'Paused') { $disp = "paused $disp" }
    Line 'SVC' $name $run "$start|$disp"
  } elseif ($always -contains $name) {
    Line 'SVC' $name 'stop' 'Missing|'
  }
}
"#;

#[cfg(any(target_os = "windows", test))]
fn parse_windows_report(text: &str) -> SecuritySample {
    let mut sample = SecuritySample::default();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let kind = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("").trim();
        let state = parts.next().unwrap_or("").trim();
        let detail = parts.next().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        match kind {
            "AV" => sample.av.push(windows_av(name, state, detail)),
            "GUARD" => {
                if let Some(row) = windows_guard(name, state, detail) {
                    sample.guards.push(row);
                }
            }
            "SVC" => sample.services.push(windows_service(name, state, detail)),
            "ERR" => {
                if sample.error.is_none() {
                    let message = if detail.is_empty() { name } else { detail };
                    sample.error = Some(message.to_string());
                }
            }
            _ => {}
        }
    }
    sample
}

#[cfg(any(target_os = "windows", test))]
fn windows_av(name: &str, state: &str, detail: &str) -> CheckRow {
    let (code, path) = detail.split_once('|').unwrap_or((detail, ""));
    let health = match state {
        "on" => Health::On,
        "off" => Health::Off,
        _ => Health::Unknown,
    };
    let base = match code {
        "realtime fresh" => "实时防护已开启，病毒库最新",
        "realtime stale" => "实时防护已开启，病毒库可能过期",
        "off fresh" => "实时防护未开启",
        "off stale" => "实时防护未开启，病毒库可能过期",
        "service only" => "安全中心没有返回产品，以下为 Defender 服务状态",
        other => other,
    };
    let text = if path.trim().is_empty() {
        base.to_string()
    } else {
        format!("{base}  ·  {}", path.trim())
    };
    row(RowKind::Guard, name, health, text, true)
}

#[cfg(any(target_os = "windows", test))]
fn windows_guard(name: &str, state: &str, detail: &str) -> Option<CheckRow> {
    if name != "firewall" {
        return Some(row(RowKind::Guard, name, health_token(state), detail, true));
    }
    let health = health_token(state);
    Some(row(
        RowKind::Guard,
        "Windows 防火墙",
        health,
        firewall_detail(detail),
        true,
    ))
}

#[cfg(any(target_os = "windows", test))]
fn firewall_detail(spec: &str) -> String {
    if spec.is_empty() {
        return "没有读到配置文件状态".into();
    }
    spec.split(',')
        .filter(|item| !item.is_empty())
        .map(|item| {
            let (key, value) = item.split_once('=').unwrap_or((item, ""));
            let name = match key.trim() {
                "domain" => "域网络",
                "private" => "专用网络",
                "public" => "公用网络",
                other => other,
            };
            let state = if value.trim() == "on" { "开" } else { "关" };
            format!("{name}{state}")
        })
        .collect::<Vec<_>>()
        .join("  ·  ")
}

#[cfg(any(target_os = "windows", test))]
fn windows_service(name: &str, state: &str, detail: &str) -> CheckRow {
    let (start, display) = detail.split_once('|').unwrap_or((detail, ""));
    let health = match state {
        "run" | "on" => Health::On,
        "stop" | "off" => Health::Off,
        _ => Health::Unknown,
    };
    let start = match start.trim() {
        "Automatic" => "自动启动",
        "Manual" => "手动启动",
        "Disabled" => "已禁用",
        "Boot" => "引导启动",
        "System" => "系统启动",
        "Missing" => "未安装或无法读取",
        "" => "",
        other => other,
    };
    let mut parts = Vec::new();
    if !start.is_empty() {
        parts.push(start.to_string());
    }
    parts.push(name.to_string());
    let display = display.trim();
    if !display.is_empty() {
        let display = display.strip_prefix("paused ").unwrap_or(display);
        if display.to_ascii_lowercase() != name.to_ascii_lowercase() {
            parts.push(display.to_string());
        }
        if detail.contains("paused ") {
            parts.push("已暂停".into());
        }
    }
    row(
        RowKind::Service,
        &service_title(name),
        health,
        parts.join("  ·  "),
        service_critical(name),
    )
}

#[cfg(any(target_os = "windows", test))]
fn service_title(name: &str) -> String {
    match name {
        "WinDefend" => "Microsoft Defender 防病毒",
        "WdNisSvc" => "Defender 网络检查",
        "MDCoreSvc" => "Defender 核心服务",
        "SecurityHealthService" => "Windows 安全健康",
        "wscsvc" => "安全中心",
        "Sense" => "Defender 终结点",
        "MpsSvc" => "Windows 防火墙服务",
        "BFE" => "基础筛选引擎",
        "wuauserv" => "Windows 更新",
        "BITS" => "后台智能传输",
        "EventLog" => "事件日志",
        "Winmgmt" => "WMI",
        "Dhcp" => "DHCP 客户端",
        "Dnscache" => "DNS 客户端",
        "WlanSvc" => "WLAN 自动配置",
        "BthServ" => "蓝牙支持",
        "Spooler" => "打印后台",
        "LanmanServer" => "文件和打印机共享",
        "TermService" => "远程桌面服务",
        "WinRM" => "远程管理",
        "RemoteRegistry" => "远程注册表",
        "W32Time" => "时间同步",
        "WSearch" => "Windows 搜索",
        "SysMain" => "SysMain",
        "Schedule" => "任务计划",
        "Audiosrv" => "Windows 音频",
        "CryptSvc" => "加密服务",
        "nsi" => "网络存储接口",
        "NlaSvc" => "网络位置感知",
        "iphlpsvc" => "IP 帮助程序",
        "PolicyAgent" => "IPsec 策略",
        "RemoteAccess" => "路由和远程访问",
        "SharedAccess" => "Internet 连接共享",
        "SSDPSRV" => "SSDP 发现",
        "WerSvc" => "错误报告",
        other => other,
    }
    .to_string()
}

#[cfg(any(target_os = "windows", test))]
fn service_critical(name: &str) -> bool {
    matches!(
        name,
        "WinDefend"
            | "WdNisSvc"
            | "MDCoreSvc"
            | "SecurityHealthService"
            | "wscsvc"
            | "Sense"
            | "MpsSvc"
            | "BFE"
            | "EventLog"
    )
}

#[cfg(any(target_os = "windows", test))]
fn health_token(state: &str) -> Health {
    match state {
        "on" | "run" => Health::On,
        "off" | "stop" => Health::Off,
        "mixed" => Health::Mixed,
        _ => Health::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchd_line_marks_running_pid() {
        let mut labels = HashMap::new();
        note_labels(
            &mut labels,
            "\t\t    3954      - \tcom.apple.XProtect.daemon.scan\n\
             \t\t       0      - \tcom.openssh.sshd\n\
             \t\t 0x22693    M   D   com.apple.alf\n\
             \t\t   57293   (pe) \tcom.apple.remotemanagementd\n",
        );
        assert_eq!(labels.get("com.apple.XProtect.daemon.scan"), Some(&true));
        assert_eq!(labels.get("com.openssh.sshd"), Some(&false));
        assert_eq!(labels.get("com.apple.remotemanagementd"), Some(&true));
        assert!(!labels.contains_key("com.apple.alf"));
        let (health, detail) = service_from_labels(
            &labels,
            &[
                "com.apple.mDNSResponder",
                "com.apple.mDNSResponder.reloaded",
            ],
        );
        assert_eq!(health, Health::Off);
        assert_eq!(detail, "未加载");
    }

    #[test]
    fn windows_report_decodes_defender_and_firewall() {
        let text = "\
AV\tMicrosoft Defender\ton\trealtime fresh|C:\\Program Files\\Windows Defender\\MsMpEng.exe
GUARD\tfirewall\tmixed\tdomain=on,private=on,public=off
SVC\tWinDefend\trun\tAutomatic|Microsoft Defender Antivirus Service
SVC\tTermService\tstop\tDisabled|Remote Desktop Services
SVC\tMpsSvc\tstop\tMissing|
ERR\tfirewall\tunknown\tdenied
";
        let sample = parse_windows_report(text);
        assert_eq!(sample.av.len(), 1);
        assert_eq!(sample.av[0].health, Health::On);
        assert!(sample.av[0].detail.contains("病毒库最新"));
        assert_eq!(sample.guards[0].name, "Windows 防火墙");
        assert_eq!(sample.guards[0].health, Health::Mixed);
        assert!(sample.guards[0].detail.contains("公用网络关"));
        assert_eq!(sample.services[0].name, "Microsoft Defender 防病毒");
        assert_eq!(sample.services[0].health, Health::On);
        assert!(sample.services[1].detail.contains("已禁用"));
        assert_eq!(sample.services[2].health, Health::Off);
        assert!(sample.services[2].detail.contains("未安装"));
        assert!(sample.error.unwrap().contains("denied"));
    }

    #[test]
    fn process_and_extension_match_vendor_names() {
        let procs = vec!["/Library/CS/falcond".into(), "WindowServer".into()];
        assert!(proc_running(&procs, &["falcond"]));
        assert!(!proc_running(&procs, &["falcon-sensor"]));
        let text = "*\t*\tTEAM\tcom.crowdstrike.falcon.Agent (7.1)\tFalcon\t[activated enabled]\n";
        assert_eq!(extension_state(text, &["crowdstrike"]), Some(true));
        assert_eq!(
            extension_state("disabled com.sentinelone.agent\n", &["sentinelone"]),
            Some(false)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn live_macos_reports_xprotect_and_guards() {
        let sample = collect();
        assert!(sample.ready);
        assert!(
            sample
                .av
                .iter()
                .any(|row| row.name == "XProtect" && row.health == Health::On),
            "av={:?}",
            sample.av
        );
        assert!(sample.guards.len() >= 4, "guards={:?}", sample.guards);
        assert!(
            sample.services.iter().any(|row| row.health == Health::On),
            "services={:?}",
            sample.services
        );
        assert!(sample.error.is_none(), "error={:?}", sample.error);
    }
}
