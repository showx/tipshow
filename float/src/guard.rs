//! 实时监视：文档目录的批量改写、注入迹象、启动项改动。
//!
//! 用户态悬浮窗不能像驱动那样拦住写入，只能在发现后立刻提示。

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use eframe::egui;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::monitor::Shared;

const WINDOW: Duration = Duration::from_secs(12);
const VOLUME_THRESHOLD: usize = 45;
const SUSPICIOUS_THRESHOLD: usize = 6;
const LOOP_IDLE: Duration = Duration::from_millis(700);
const STARTUP_POLL: Duration = Duration::from_secs(8);
const INJECT_POLL: Duration = Duration::from_secs(3);

const SUSPICIOUS_EXT: &[&str] = &[
    "encrypted",
    "locked",
    "crypt",
    "crypto",
    "crypted",
    "enc",
    "ryk",
    "lockbit",
    "wncry",
    "wnry",
    "locky",
    "cerber",
    "cryptolocker",
    "micro",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub id: u64,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuardView {
    pub ready: bool,
    pub alerts: Vec<Alert>,
    pub note: String,
}

pub fn spawn(shared: Arc<Shared>, ctx: egui::Context) {
    thread::Builder::new()
        .name("tipshow-guard".into())
        .spawn(move || run(shared, ctx))
        .expect("启动实时监视线程");
}

pub fn dismiss(shared: &Shared, id: u64) {
    lock(&shared.guard_dismiss).push(id);
}

fn run(shared: Arc<Shared>, ctx: egui::Context) {
    let mut engine = Engine::new();
    let doc_roots = document_roots();
    let startup_roots = startup_roots();
    engine.prime_startup(&startup_roots);
    engine.note = watch_note(&doc_roots, &startup_roots);
    let (tx, rx) = mpsc::channel();
    let watcher = start_watcher(tx, &doc_roots);
    if watcher.is_none() && !doc_roots.is_empty() {
        engine.note = format!("{}  文档目录监视没有启动。", engine.note);
    }
    engine.ready = true;
    publish(&shared, &engine, &ctx);

    let mut last_startup = Instant::now();
    let mut last_inject = Instant::now();
    loop {
        drain_files(&rx, watcher.is_some(), &mut engine);
        if last_inject.elapsed() >= INJECT_POLL {
            engine.scan_injection();
            last_inject = Instant::now();
        }
        if last_startup.elapsed() >= STARTUP_POLL {
            engine.rescan_startup(&startup_roots);
            last_startup = Instant::now();
        }
        apply_dismissals(&shared, &mut engine);
        publish(&shared, &engine, &ctx);
    }
}

fn start_watcher(
    tx: mpsc::Sender<Result<Event, notify::Error>>,
    roots: &[PathBuf],
) -> Option<RecommendedWatcher> {
    if roots.is_empty() {
        return None;
    }
    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default()).ok()?;
    let mut any = false;
    for root in roots {
        if watcher.watch(root, RecursiveMode::Recursive).is_ok() {
            any = true;
            eprintln!("tipshow: 监视 {}", root.display());
        }
    }
    any.then_some(watcher)
}

fn drain_files(rx: &Receiver<Result<Event, notify::Error>>, watching: bool, engine: &mut Engine) {
    if !watching {
        thread::sleep(LOOP_IDLE);
        return;
    }
    match rx.recv_timeout(LOOP_IDLE) {
        Ok(event) => engine.on_notify(event),
        Err(mpsc::RecvTimeoutError::Timeout) => return,
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            thread::sleep(LOOP_IDLE);
            return;
        }
    }
    while let Ok(event) = rx.try_recv() {
        engine.on_notify(event);
    }
}

fn publish(shared: &Shared, engine: &Engine, ctx: &egui::Context) {
    *lock(&shared.guard) = engine.view();
    ctx.request_repaint();
}

fn apply_dismissals(shared: &Shared, engine: &mut Engine) {
    let ids: Vec<u64> = std::mem::take(&mut *lock(&shared.guard_dismiss));
    for id in ids {
        engine.dismiss(id);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

struct Engine {
    ready: bool,
    note: String,
    alerts: Vec<Alert>,
    next_id: u64,
    files: VecDeque<(Instant, PathBuf)>,
    encrypt_muted: bool,
    startup: BTreeMap<String, Stamp>,
    #[cfg(target_os = "windows")]
    reg_base: BTreeMap<String, String>,
    #[cfg(target_os = "windows")]
    reg_ready: bool,
    #[cfg(target_os = "windows")]
    debugger_base: BTreeMap<String, String>,
    #[cfg(target_os = "windows")]
    debugger_ready: bool,
    inject_seen: HashSet<String>,
    #[cfg(target_os = "windows")]
    appinit_sig: String,
}

impl Engine {
    fn new() -> Self {
        Self {
            ready: false,
            note: "正在建立监视基线…".into(),
            alerts: Vec::new(),
            next_id: 1,
            files: VecDeque::new(),
            encrypt_muted: false,
            startup: BTreeMap::new(),
            #[cfg(target_os = "windows")]
            reg_base: BTreeMap::new(),
            #[cfg(target_os = "windows")]
            reg_ready: false,
            #[cfg(target_os = "windows")]
            debugger_base: BTreeMap::new(),
            #[cfg(target_os = "windows")]
            debugger_ready: false,
            inject_seen: HashSet::new(),
            #[cfg(target_os = "windows")]
            appinit_sig: String::new(),
        }
    }

    fn view(&self) -> GuardView {
        GuardView {
            ready: self.ready,
            alerts: self.alerts.clone(),
            note: self.note.clone(),
        }
    }

    fn dismiss(&mut self, id: u64) {
        if self
            .alerts
            .iter()
            .any(|alert| alert.id == id && alert_key(alert) == "encrypt")
        {
            self.encrypt_muted = true;
        }
        self.alerts.retain(|alert| alert.id != id);
    }

    fn on_notify(&mut self, event: Result<Event, notify::Error>) {
        let Ok(event) = event else {
            return;
        };
        if !interesting_event(&event.kind) {
            return;
        }
        let now = Instant::now();
        for path in event.paths {
            if watched_file(&path) {
                self.files.push_back((now, path));
            }
        }
        self.trim_files(now);
        self.judge_files(now);
    }

    fn trim_files(&mut self, now: Instant) {
        while self
            .files
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > WINDOW)
        {
            self.files.pop_front();
        }
        while self.files.len() > 4000 {
            self.files.pop_front();
        }
    }

    fn judge_files(&mut self, now: Instant) {
        let paths: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|(at, _)| now.duration_since(*at) <= WINDOW)
            .map(|(_, path)| path.clone())
            .collect();
        let unique = unique_paths(&paths);
        match burst_verdict(&unique) {
            None => self.encrypt_muted = false,
            Some(_) if self.encrypt_muted => {}
            Some(verdict) => self.upsert("encrypt", &verdict.title, verdict.detail),
        }
    }

    fn prime_startup(&mut self, roots: &[PathBuf]) {
        self.startup = scan_startup(roots);
        self.prime_registry();
    }

    fn rescan_startup(&mut self, roots: &[PathBuf]) {
        let now = scan_startup(roots);
        let changes = diff_stamps(&self.startup, &now);
        self.startup = now;
        if !changes.is_empty() {
            self.upsert("startup", "启动项被改动", format_changes(&changes));
        }
        self.rescan_registry();
    }

    fn prime_registry(&mut self) {
        #[cfg(target_os = "windows")]
        {
            self.reg_base = read_run_keys();
            self.reg_ready = true;
            self.note_new_debuggers();
            self.note_appinit(&read_appinit());
        }
    }

    fn rescan_registry(&mut self) {
        #[cfg(target_os = "windows")]
        {
            let now = read_run_keys();
            if self.reg_ready {
                let changes = diff_text_map(&self.reg_base, &now);
                if !changes.is_empty() {
                    self.upsert("startup-reg", "启动项被改动", format_changes(&changes));
                }
            }
            self.reg_base = now;
            self.reg_ready = true;
            self.note_appinit(&read_appinit());
            self.note_new_debuggers();
        }
    }

    #[cfg(target_os = "windows")]
    fn note_appinit(&mut self, entries: &[(String, String)]) {
        let active: Vec<_> = entries
            .iter()
            .filter(|(name, value)| name.ends_with("AppInit_DLLs") && registry_value_set(value))
            .cloned()
            .collect();
        let signature = active
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("|");
        if signature.is_empty() || signature == self.appinit_sig {
            if signature.is_empty() {
                self.appinit_sig.clear();
            }
            return;
        }
        self.appinit_sig = signature;
        let detail = active
            .iter()
            .map(|(name, value)| format!("{name} = {value}"))
            .collect::<Vec<_>>()
            .join("；");
        self.upsert(
            "appinit",
            "进程注入配置",
            format!("AppInit_DLLs 会在进程启动时加载额外模块。{detail}"),
        );
    }

    #[cfg(target_os = "windows")]
    fn note_new_debuggers(&mut self) {
        let now: BTreeMap<String, String> = read_debuggers().into_iter().collect();
        if self.debugger_ready {
            let changes = diff_text_map(&self.debugger_base, &now);
            if !changes.is_empty() {
                self.upsert(
                    "ifeo",
                    "进程启动被劫持",
                    format!("映像劫持里的调试器设置有变化。{}", format_changes(&changes)),
                );
            }
        }
        self.debugger_base = now;
        self.debugger_ready = true;
    }

    fn scan_injection(&mut self) {
        #[cfg(target_os = "macos")]
        {
            let text = command_text("ps", &["-axeww", "-o", "pid=,command="]).unwrap_or_default();
            for (pid, name, libs) in parse_dyld(&text) {
                let key = format!("inject:{pid}:{libs}");
                if !self.inject_seen.insert(key.clone()) {
                    continue;
                }
                self.upsert(
                    &key,
                    "进程注入迹象",
                    format!("进程 {name} ({pid}) 设置了 DYLD_INSERT_LIBRARIES={libs}"),
                );
            }
        }
    }

    fn upsert(&mut self, key: &str, title: &str, detail: String) {
        if let Some(alert) = self.alerts.iter_mut().find(|alert| alert_key(alert) == key) {
            alert.title = title.to_string();
            alert.detail = detail;
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.alerts.insert(
            0,
            Alert {
                id,
                title: format!("{key}\u{1f}{title}"),
                detail,
            },
        );
        self.alerts.truncate(20);
    }
}

fn alert_key(alert: &Alert) -> &str {
    alert.title.split('\u{1f}').next().unwrap_or("")
}

fn alert_title(alert: &Alert) -> &str {
    alert
        .title
        .split_once('\u{1f}')
        .map(|(_, title)| title)
        .unwrap_or(alert.title.as_str())
}

pub fn display_title(alert: &Alert) -> &str {
    alert_title(alert)
}

struct Verdict {
    title: String,
    detail: String,
}

fn burst_verdict(paths: &[PathBuf]) -> Option<Verdict> {
    if paths.is_empty() {
        return None;
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for path in paths {
        if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            let ext = ext.to_ascii_lowercase();
            if SUSPICIOUS_EXT.contains(&ext.as_str()) {
                *counts.entry(ext).or_default() += 1;
            }
        }
    }
    let suspicious = counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count >= SUSPICIOUS_THRESHOLD);
    let volume = paths.len() >= VOLUME_THRESHOLD;
    if suspicious.is_none() && !volume {
        return None;
    }
    let sample = paths
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .take(4)
        .collect::<Vec<_>>()
        .join("、");
    let (title, detail) = if let Some((ext, count)) = suspicious {
        (
            "疑似批量加密".to_string(),
            format!(
                "{count} 个文件在十余秒内变成 .{ext}，共涉及 {} 个文件。如果不是你在批量改名或加密，请马上暂停同步并检查正在写文件的程序。例如 {sample}",
                paths.len()
            ),
        )
    } else {
        (
            "大量文件被改写".to_string(),
            format!(
                "{} 个文件在十余秒内被改写。解压、导入和同步也会这样；如果不是你在操作，请立刻确认来源。例如 {sample}",
                paths.len()
            ),
        )
    };
    Some(Verdict { title, detail })
}

fn unique_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for path in paths {
        if seen.insert(path.to_string_lossy().to_string()) {
            unique.push(path.clone());
        }
    }
    unique
}

fn interesting_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(notify::event::ModifyKind::Data(_))
            | EventKind::Modify(notify::event::ModifyKind::Name(_))
            | EventKind::Modify(notify::event::ModifyKind::Any)
            | EventKind::Any
    )
}

fn watched_file(path: &Path) -> bool {
    if path.components().count() == 0 {
        return false;
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.is_empty()
        || name.starts_with('.')
        || name == "desktop.ini"
        || name.ends_with(".crdownload")
        || name.ends_with(".download")
        || name.ends_with(".part")
        || name.ends_with(".tmp")
    {
        return false;
    }
    let text = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    const SKIP: &[&str] = &[
        "/node_modules/",
        "/.git/",
        "/target/",
        "/library/",
        "/caches/",
        "/.trash/",
        "/deriveddata/",
    ];
    !SKIP.iter().any(|part| text.contains(part))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: u64,
    len: u64,
}

fn scan_startup(roots: &[PathBuf]) -> BTreeMap<String, Stamp> {
    let mut map = BTreeMap::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            if name.starts_with('.') || name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            map.insert(path.display().to_string(), stamp_of(&meta));
        }
    }
    map
}

fn stamp_of(meta: &std::fs::Metadata) -> Stamp {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| time.as_secs())
        .unwrap_or(0);
    Stamp {
        modified,
        len: meta.len(),
    }
}

fn diff_stamps(before: &BTreeMap<String, Stamp>, after: &BTreeMap<String, Stamp>) -> Vec<String> {
    let mut changes = Vec::new();
    for (path, stamp) in after {
        match before.get(path) {
            None => changes.push(format!("新增 {path}")),
            Some(prev) if prev != stamp => changes.push(format!("修改 {path}")),
            _ => {}
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) {
            changes.push(format!("删除 {path}"));
        }
    }
    changes
}

#[cfg(target_os = "windows")]
fn diff_text_map(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut changes = Vec::new();
    for (key, value) in after {
        match before.get(key) {
            None => changes.push(format!("新增 {key} = {value}")),
            Some(prev) if prev != value => changes.push(format!("修改 {key} = {value}")),
            _ => {}
        }
    }
    for key in before.keys() {
        if !after.contains_key(key) {
            changes.push(format!("删除 {key}"));
        }
    }
    changes
}

fn format_changes(changes: &[String]) -> String {
    let shown = changes
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join("；");
    if changes.len() > 5 {
        format!("{shown}；等 {} 处", changes.len())
    } else {
        shown
    }
}

fn document_roots() -> Vec<PathBuf> {
    let Some(home) = user_home() else {
        return Vec::new();
    };
    ["Documents", "Desktop", "Downloads", "Pictures"]
        .into_iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_dir())
        .collect()
}

fn startup_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = user_home() {
        roots.push(home.join("Library/LaunchAgents"));
        roots.push(home.join("Library/LaunchDaemons"));
        roots.push(home.join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup"));
    }
    roots.push(PathBuf::from("/Library/LaunchAgents"));
    roots.push(PathBuf::from("/Library/LaunchDaemons"));
    roots.push(PathBuf::from(
        r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Startup",
    ));
    roots.into_iter().filter(|path| path.is_dir()).collect()
}

fn watch_note(docs: &[PathBuf], startup: &[PathBuf]) -> String {
    format!(
        "正在监视 {} 个文档目录和 {} 个启动项目录。发现批量改写、注入迹象或启动项改动会立刻提示。",
        docs.len(),
        startup.len()
    )
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn parse_dyld(text: &str) -> Vec<(u32, String, String)> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((pid_text, command)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid_text.trim().parse::<u32>() else {
            continue;
        };
        let command = command.trim();
        let lower = command.to_ascii_lowercase();
        let Some(index) = lower.find("dyld_insert_libraries=") else {
            continue;
        };
        let value = command[index + "dyld_insert_libraries=".len()..]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches('"');
        if value.is_empty() || value.eq_ignore_ascii_case("(null)") {
            continue;
        }
        let name = command
            .split_whitespace()
            .next()
            .and_then(|token| Path::new(token).file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "进程".into());
        found.push((pid, name, value.to_string()));
    }
    found
}

#[cfg(target_os = "windows")]
fn registry_value_set(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.eq_ignore_ascii_case("(value not set)")
}

#[cfg(target_os = "windows")]
fn read_run_keys() -> BTreeMap<String, String> {
    const KEYS: &[&str] = &[
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\RunOnce",
    ];
    let mut map = BTreeMap::new();
    for key in KEYS {
        if let Ok(text) = command_text("reg", &["query", key]) {
            for (name, value) in parse_reg(&text) {
                if registry_value_set(&value) {
                    map.insert(format!("{key}\\{name}"), value);
                }
            }
        }
    }
    map
}

#[cfg(target_os = "windows")]
fn read_appinit() -> Vec<(String, String)> {
    const KEYS: &[&str] = &[
        r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Windows",
        r"HKLM\SOFTWARE\Wow6432Node\Microsoft\Windows NT\CurrentVersion\Windows",
    ];
    let mut entries = Vec::new();
    for key in KEYS {
        if let Ok(text) = command_text("reg", &["query", key, "/v", "AppInit_DLLs"]) {
            entries.extend(parse_reg(&text));
        }
    }
    entries
}

#[cfg(target_os = "windows")]
fn read_debuggers() -> Vec<(String, String)> {
    const KEY: &str =
        r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";
    let Ok(text) = command_text("reg", &["query", KEY, "/s", "/v", "Debugger"]) else {
        return Vec::new();
    };
    parse_reg(&text)
        .into_iter()
        .filter(|(name, value)| name == "Debugger" && registry_value_set(value))
        .collect()
}

#[cfg(any(target_os = "windows", test))]
fn parse_reg(text: &str) -> Vec<(String, String)> {
    const TYPES: &[&str] = &[
        "REG_EXPAND_SZ",
        "REG_MULTI_SZ",
        "REG_DWORD",
        "REG_QWORD",
        "REG_BINARY",
        "REG_SZ",
    ];
    let mut hive = String::new();
    let mut entries = Vec::new();
    for line in text.lines() {
        let trim = line.trim();
        if trim.is_empty() {
            continue;
        }
        if trim.starts_with("HKEY_") {
            hive = trim.to_string();
            continue;
        }
        let Some((start, kind)) = TYPES
            .iter()
            .find_map(|kind| trim.find(kind).map(|start| (start, *kind)))
        else {
            continue;
        };
        let name = trim[..start].trim();
        if name.is_empty() {
            continue;
        }
        let value = trim[start + kind.len()..].trim();
        let label = if hive.is_empty() {
            name.to_string()
        } else {
            format!("{hive}\\{name}")
        };
        entries.push((label, value.to_string()));
    }
    entries
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn command_text(cmd: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(cmd);
    command.args(args).stdin(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().map_err(|err| err.to_string())?;
    let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    }
    if text.is_empty() && !output.status.success() {
        return Err(format!("{cmd} 退出 {}", output.status));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: impl Into<PathBuf>) -> PathBuf {
        text.into()
    }

    #[test]
    fn volume_and_encrypted_extension_raise_different_alerts() {
        let quiet: Vec<_> = (0..10)
            .map(|index| path(format!("/Users/a/Documents/a{index}.txt")))
            .collect();
        assert!(burst_verdict(&quiet).is_none());

        let mass: Vec<_> = (0..VOLUME_THRESHOLD)
            .map(|index| path(format!("/Users/a/Desktop/photo{index}.jpg")))
            .collect();
        let verdict = burst_verdict(&mass).unwrap();
        assert_eq!(verdict.title, "大量文件被改写");

        let encrypted: Vec<_> = (0..SUSPICIOUS_THRESHOLD)
            .map(|index| path(format!("/Users/a/Documents/note{index}.locked")))
            .collect();
        let verdict = burst_verdict(&encrypted).unwrap();
        assert_eq!(verdict.title, "疑似批量加密");
        assert!(verdict.detail.contains(".locked"));
    }

    #[test]
    fn ignores_downloads_in_progress_and_build_output() {
        assert!(!watched_file(Path::new("/Users/a/Downloads/a.crdownload")));
        assert!(!watched_file(Path::new(
            "/Users/a/Documents/proj/target/debug/app"
        )));
        assert!(!watched_file(Path::new(
            "/Users/a/Documents/proj/node_modules/leftpad/index.js"
        )));
        assert!(watched_file(Path::new("/Users/a/Documents/tax.pdf")));
    }

    #[test]
    fn startup_diff_reports_add_and_remove() {
        let mut before = BTreeMap::new();
        before.insert(
            "/Library/LaunchAgents/a.plist".into(),
            Stamp {
                modified: 1,
                len: 10,
            },
        );
        before.insert(
            "/Library/LaunchAgents/b.plist".into(),
            Stamp {
                modified: 1,
                len: 10,
            },
        );
        let mut after = before.clone();
        after.remove("/Library/LaunchAgents/b.plist");
        after.insert(
            "/Library/LaunchAgents/c.plist".into(),
            Stamp {
                modified: 2,
                len: 12,
            },
        );
        after.get_mut("/Library/LaunchAgents/a.plist").unwrap().len = 99;
        let changes = diff_stamps(&before, &after);
        assert!(changes
            .iter()
            .any(|line| line.contains("新增") && line.contains("c.plist")));
        assert!(changes
            .iter()
            .any(|line| line.contains("删除") && line.contains("b.plist")));
        assert!(changes
            .iter()
            .any(|line| line.contains("修改") && line.contains("a.plist")));
    }

    #[test]
    fn dyld_and_registry_parsers_find_injection_config() {
        let text = "\
  42 /usr/bin/python3 DYLD_INSERT_LIBRARIES=/tmp/extra.dylib
  7 /Applications/Safari.app/Contents/MacOS/Safari
  9 /bin/zsh DYLD_INSERT_LIBRARIES=(null)
";
        let found = parse_dyld(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, 42);
        assert_eq!(found[0].2, "/tmp/extra.dylib");

        let reg = "\
HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run
    OneDrive    REG_SZ    C:\\Users\\a\\OneDrive.exe

HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Windows
    AppInit_DLLs    REG_SZ    C:\\bad.dll
";
        let entries = parse_reg(reg);
        assert!(entries
            .iter()
            .any(|(name, value)| name.ends_with("OneDrive") && value.contains("OneDrive.exe")));
        assert!(entries
            .iter()
            .any(|(name, value)| name.ends_with("AppInit_DLLs") && value.contains("bad.dll")));
    }

    #[test]
    fn dismiss_mutes_the_current_encrypt_burst() {
        let mut engine = Engine::new();
        let now = Instant::now();
        for index in 0..SUSPICIOUS_THRESHOLD {
            engine
                .files
                .push_back((now, path(format!("/Users/a/Documents/f{index}.encrypted"))));
        }
        engine.judge_files(now);
        assert_eq!(engine.alerts.len(), 1);
        let id = engine.alerts[0].id;
        engine.dismiss(id);
        engine.judge_files(now);
        assert!(engine.alerts.is_empty());
        engine.files.clear();
        engine.judge_files(now + WINDOW + Duration::from_secs(1));
        for index in 0..SUSPICIOUS_THRESHOLD {
            engine.files.push_back((
                now + WINDOW + Duration::from_secs(1),
                path(format!("/Users/a/Desktop/n{index}.locked")),
            ));
        }
        engine.judge_files(now + WINDOW + Duration::from_secs(1));
        assert_eq!(engine.alerts.len(), 1);
    }
}
