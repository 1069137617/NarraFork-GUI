//! 本机 NarraFork 核心的生命周期。
//!
//! 壳只管理自己拉起的后端，不探测、不复用外部实例：端口与就绪状态来自后端自己的
//! `NarraFork server running on …` 启动行。这样桌面壳和 NarraFork 核心是同一套启动
//! 语义，不需要额外的发现协议。
//!
//! 这里重新实现了日志行解析，而不是复制 `server/manager/*.ts`：那些文件属于
//! MPL-2.0 覆盖的 NarraFork 源码，不能原样带进桌面壳产物。

use std::collections::VecDeque;
use std::env;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::{self, ShellConfig};
use crate::window;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const STOP_TIMEOUT: Duration = Duration::from_secs(15);
const LOG_TAIL: usize = 200;
const DIAGNOSTIC_LINES: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
	Checking,
	Starting,
	Ready,
	Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootState {
	pub phase: Phase,
	pub message: String,
	pub detail: Option<String>,
	pub url: Option<String>,
	pub version: Option<String>,
	pub commit: Option<String>,
	pub log_path: Option<String>,
	pub reused: Option<bool>,
}

impl BootState {
	fn checking(message: impl Into<String>) -> Self {
		Self {
			phase: Phase::Checking,
			message: message.into(),
			detail: None,
			url: None,
			version: None,
			commit: None,
			log_path: None,
			reused: None,
		}
	}

	fn starting(message: impl Into<String>) -> Self {
		Self {
			phase: Phase::Starting,
			..Self::checking(message)
		}
	}

	fn ready(url: String, info: BuildInfo) -> Self {
		Self {
			phase: Phase::Ready,
			message: "服务已就绪，正在打开工作台…".to_string(),
			detail: None,
			url: Some(url),
			version: info.version,
			commit: info.commit,
			log_path: None,
			reused: Some(false),
		}
	}

	fn error(message: impl Into<String>, detail: Option<String>) -> Self {
		Self {
			phase: Phase::Error,
			message: message.into(),
			detail,
			url: None,
			version: None,
			commit: None,
			log_path: None,
			reused: None,
		}
	}
}

#[derive(Debug, Clone, Default)]
struct BuildInfo {
	version: Option<String>,
	commit: Option<String>,
}

/// 启动日志行。`seq` 让 splash 在「先取历史、后收事件」之间去重。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
	pub seq: u64,
	pub line: String,
}

#[derive(Debug, Clone)]
pub struct ListenTarget {
	scheme: String,
	host: String,
	port: u16,
}

impl ListenTarget {
	fn base_url(&self) -> String {
		format!(
			"{}://{}:{}",
			self.scheme,
			config::url_host(&self.host),
			self.port
		)
	}
}

struct BackendHandle {
	child: Option<Child>,
	target: Option<ListenTarget>,
}

struct BootError {
	message: String,
	detail: Option<String>,
}

impl BootError {
	fn new(message: impl Into<String>, detail: Option<String>) -> Self {
		Self {
			message: message.into(),
			detail,
		}
	}
}

pub struct AppState {
	phase: Mutex<BootState>,
	backend: Mutex<BackendHandle>,
	build_info: Mutex<BuildInfo>,
	token: String,
	pub log_dir: PathBuf,
	pub log_path: PathBuf,
	config: ShellConfig,
	http: reqwest::blocking::Client,
	booting: AtomicBool,
	log_seq: AtomicU64,
	recent: Mutex<VecDeque<LogLine>>,
	#[cfg(windows)]
	_job: Option<job::JobHandle>,
}

impl AppState {
	pub fn new(app: &AppHandle, config: ShellConfig) -> Self {
		let log_dir = app
			.path()
			.app_log_dir()
			.unwrap_or_else(|_| env::temp_dir().join("narrafork-desktop"));
		let _ = std::fs::create_dir_all(&log_dir);
		let log_path = log_dir.join("backend.log");

		let http = reqwest::blocking::Client::builder()
			.timeout(Duration::from_secs(5))
			// 停止请求只发生在本机回环；自签证书不应该阻断优雅退出。
			.danger_accept_invalid_certs(true)
			.build()
			.unwrap_or_else(|_| reqwest::blocking::Client::new());

		Self {
			phase: Mutex::new(BootState::checking("正在启动核心…")),
			backend: Mutex::new(BackendHandle {
				child: None,
				target: None,
			}),
			build_info: Mutex::new(BuildInfo::default()),
			token: random_token(),
			log_dir,
			log_path,
			config,
			http,
			booting: AtomicBool::new(false),
			log_seq: AtomicU64::new(0),
			recent: Mutex::new(VecDeque::new()),
			#[cfg(windows)]
			_job: job::JobHandle::create(),
		}
	}

	pub fn snapshot(&self) -> BootState {
		let mut state = lock(&self.phase).clone();
		if state.log_path.is_none() {
			state.log_path = Some(self.log_path.display().to_string());
		}
		state
	}

	pub fn current_url(&self) -> Option<String> {
		lock(&self.phase).url.clone()
	}

	/// 最近的启动日志，供 splash 刚加载时补齐（事件可能早于页面就绪）。
	pub fn log_tail(&self, limit: usize) -> Vec<LogLine> {
		let recent = lock(&self.recent);
		recent
			.iter()
			.skip(recent.len().saturating_sub(limit))
			.cloned()
			.collect()
	}
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn random_token() -> String {
	use rand::RngCore;

	let mut bytes = [0u8; 32];
	rand::thread_rng().fill_bytes(&mut bytes);
	let mut token = String::with_capacity(64);
	for byte in bytes {
		use std::fmt::Write as _;
		let _ = write!(token, "{byte:02x}");
	}
	token
}

fn set_state(app: &AppHandle, state: &AppState, mut next: BootState) {
	if next.log_path.is_none() {
		next.log_path = Some(state.log_path.display().to_string());
	}
	*lock(&state.phase) = next.clone();
	let _ = app.emit("boot-state", &next);
}

/// 启动（或重试）启动流程。重复调用会被 `booting` 标志合并。
pub fn start(app: AppHandle, state: Arc<AppState>) {
	if state.booting.swap(true, Ordering::SeqCst) {
		return;
	}
	thread::spawn(move || {
		if let Err(error) = boot(&app, &state) {
			append_log(
				&state,
				&format!(
					"[shell] boot failed: {} ({})",
					error.message,
					error.detail.as_deref().unwrap_or("no detail")
				),
			);
			set_state(
				&app,
				&state,
				BootState::error(error.message, error.detail),
			);
		}
		state.booting.store(false, Ordering::SeqCst);
	});
}

fn boot(app: &AppHandle, state: &Arc<AppState>) -> Result<(), BootError> {
	set_state(
		app,
		state,
		BootState::starting("正在启动本机 NarraFork 核心…"),
	);
	spawn_backend(app, state)?;

	let started = Instant::now();
	loop {
		let exited = {
			let mut handle = lock(&state.backend);
			handle
				.child
				.as_mut()
				.and_then(|child| child.try_wait().ok().flatten())
		};
		if let Some(status) = exited {
			return Err(BootError::new(
				"本机核心启动失败",
				Some(format!(
					"后端进程已退出：{status}\n\n最近日志：\n{}",
					tail(state)
				)),
			));
		}

		if let Some(target) = lock(&state.backend).target.clone() {
			let info = lock(&state.build_info).clone();
			return finish_ready(app, state, &target.base_url(), info);
		}

		if started.elapsed() > STARTUP_TIMEOUT {
			stop(app, state);
			return Err(BootError::new(
				"本机核心启动超时",
				Some(format!(
					"超过 {} 秒仍未看到启动完成日志（可能在执行数据库迁移）。\n\n最近日志：\n{}",
					STARTUP_TIMEOUT.as_secs(),
					tail(state)
				)),
			));
		}
		thread::sleep(Duration::from_millis(100));
	}
}

fn finish_ready(
	app: &AppHandle,
	state: &Arc<AppState>,
	base: &str,
	info: BuildInfo,
) -> Result<(), BootError> {
	let url = format!("{}/", base.trim_end_matches('/'));
	append_log(state, &format!("[shell] ready at {url}"));
	set_state(app, state, BootState::ready(url.clone(), info));

	// 给 splash 一点时间展示“已就绪”和最后几行日志，再切主窗口。
	thread::sleep(Duration::from_millis(900));

	let app_for_window = app.clone();
	let state_for_window = state.clone();
	app.run_on_main_thread(move || {
		if let Err(error) = window::show_main(&app_for_window, &url) {
			append_log(
				&state_for_window,
				&format!("[shell] could not create main window: {error}"),
			);
			set_state(
				&app_for_window,
				&state_for_window,
				BootState::error("无法创建主窗口", Some(error)),
			);
		}
	})
	.map_err(|error| BootError::new("无法创建主窗口", Some(error.to_string())))?;

	Ok(())
}

fn spawn_backend(app: &AppHandle, state: &Arc<AppState>) -> Result<(), BootError> {
	let (program, args, cwd) = resolve_backend_command()?;
	append_log(
		state,
		&format!(
			"[shell] starting core: {} {} (cwd={})",
			program.display(),
			args.join(" "),
			cwd.as_ref()
				.map(|path| path.display().to_string())
				.unwrap_or_else(|| "<inherited>".to_string())
		),
	);
	let mut command = Command::new(&program);
	command
		.args(&args)
		.env("NARRAFORK_MANAGER_TOKEN", &state.token)
		// 与现有 manager 一致：由外壳负责打开前端窗口，后端自己不要再弹浏览器。
		.env("NARRAFORK_MANAGER_MANAGED", "1")
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());

	if let Some(cwd) = &cwd {
		command.current_dir(cwd);
	}
	if let Some(port) = state.config.port_override {
		command.arg(format!("--port={port}"));
	}

	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		// CREATE_NO_WINDOW：后端是控制台程序，但桌面壳不应该闪出一个黑框。
		command.creation_flags(0x0800_0000);
	}

	let mut child = command.spawn().map_err(|error| {
		BootError::new(
			"无法启动本机核心",
			Some(format!("{}: {error}", program.display())),
		)
	})?;

	let stdout = child.stdout.take();
	let stderr = child.stderr.take();

	#[cfg(windows)]
	if let Some(job) = &state._job {
		job.assign(&child);
	}

	{
		let mut handle = lock(&state.backend);
		handle.child = Some(child);
		handle.target = None;
	}
	*lock(&state.build_info) = BuildInfo::default();

	if let Some(stdout) = stdout {
		spawn_reader(stdout, app.clone(), state.clone());
	}
	if let Some(stderr) = stderr {
		spawn_reader(stderr, app.clone(), state.clone());
	}

	Ok(())
}

fn is_dev_backend() -> bool {
	cfg!(debug_assertions) || env::var_os("NF_GUI_DEV").is_some()
}

/// 找到真正的 Bun 可执行文件。
///
/// Windows 上 `bun` 经常是 npm 生成的 `bun.cmd` 垫片，而 GUI 进程不能直接 CreateProcess
/// 一个 `.cmd`；`bun.exe` 实际在 `<prefix>/node_modules/bun/bin/bun.exe`。因此同时检查
/// 同名目录与 PATH 里的 bun 包目录，并支持 `NF_GUI_BUN` 显式覆盖。
fn resolve_bun() -> Result<PathBuf, BootError> {
	if let Some(explicit) = env::var_os("NF_GUI_BUN") {
		let path = PathBuf::from(explicit);
		if path.is_file() {
			return Ok(path);
		}
		return Err(BootError::new(
			"NF_GUI_BUN 指向的文件不存在",
			Some(path.display().to_string()),
		));
	}

	let mut candidates: Vec<PathBuf> = Vec::new();
	if let Some(path) = env::var_os("PATH") {
		for directory in env::split_paths(&path) {
			candidates.push(directory.join("bun.exe"));
			candidates.push(
				directory
					.join("node_modules")
					.join("bun")
					.join("bin")
					.join("bun.exe"),
			);
		}
	}
	if let Some(install) = env::var_os("BUN_INSTALL") {
		candidates.push(PathBuf::from(install).join("bin").join("bun.exe"));
	}
	if let Some(home) = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")) {
		candidates.push(
			PathBuf::from(home)
				.join(".bun")
				.join("bin")
				.join("bun.exe"),
		);
	}

	for candidate in candidates {
		if candidate.is_file() {
			return Ok(candidate);
		}
	}

	Err(BootError::new(
		"找不到 Bun",
		Some(
			"开发模式需要 Bun。请把 bun.exe 加入 PATH，或用 NF_GUI_BUN 指向它（npm 全局安装通常在 <prefix>/node_modules/bun/bin/bun.exe）。"
				.to_string(),
		),
	))
}

fn resolve_backend_command() -> Result<(PathBuf, Vec<String>, Option<PathBuf>), BootError> {
	if let Ok(explicit) = env::var("NF_GUI_BACKEND") {
		let value = explicit.trim();
		if !value.is_empty() {
			let path = PathBuf::from(value);
			let is_script = path
				.extension()
				.and_then(|extension| extension.to_str())
				.map(|extension| {
					extension.eq_ignore_ascii_case("ts") || extension.eq_ignore_ascii_case("js")
				})
				.unwrap_or(false);
			if is_script {
				let cwd = path.parent().map(PathBuf::from);
				return Ok((
					resolve_bun()?,
					vec![path.to_string_lossy().into_owned(), "--serve".to_string()],
					cwd,
				));
			}
			return Ok((path, vec!["--serve".to_string()], None));
		}
	}

	if is_dev_backend() {
		let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.parent()
			.and_then(|desktop| desktop.parent())
			.map(PathBuf::from)
			.ok_or_else(|| BootError::new("无法定位仓库根目录", None))?;
		let entry = repo_root.join("server").join("index.ts");
		let frontend = repo_root.join("dist").join("frontend").join("index.html");
		if !entry.exists() {
			return Err(BootError::new(
				"找不到 NarraFork 源码",
				Some(format!(
					"期望存在 {}。请把桌面壳放在 NarraFork 仓库的 desktop/ 下，或用 NF_GUI_BACKEND 指定后端。",
					entry.display()
				)),
			));
		}
		if !frontend.exists() {
			return Err(BootError::new(
				"开发模式缺少前端构建",
				Some(format!(
					"请先在仓库根运行 `bun run build`。\n期望产物：{}",
					frontend.display()
				)),
			));
		}
		return Ok((
			resolve_bun()?,
			vec![entry.to_string_lossy().into_owned(), "--serve".to_string()],
			Some(repo_root),
		));
	}

	let executable = env::current_exe().map_err(|error| {
		BootError::new("无法定位桌面壳可执行文件", Some(error.to_string()))
	})?;
	let sidecar = executable
		.parent()
		.map(|directory| directory.join("narrafork.exe"))
		.ok_or_else(|| BootError::new("无法定位后端 sidecar 目录", None))?;

	if sidecar.exists() {
		return Ok((sidecar, vec!["--serve".to_string()], None));
	}

	Err(BootError::new(
		"未找到 NarraFork 核心",
		Some(format!(
			"期望在 {} 找到 narrafork.exe。发布构建请先运行 `bun run prepare:sidecar`，或用 NF_GUI_BACKEND 指定可执行文件。",
			sidecar.display()
		)),
	))
}

fn spawn_reader<R: Read + Send + 'static>(
	reader: R,
	app: AppHandle,
	state: Arc<AppState>,
) {
	thread::spawn(move || {
		for line in BufReader::new(reader).lines().map_while(Result::ok) {
			handle_line(&app, &state, &line);
		}
	});
}

fn handle_line(app: &AppHandle, state: &AppState, line: &str) {
	append_log(state, line);
	let clean = strip_ansi(line);
	let seq = state.log_seq.fetch_add(1, Ordering::SeqCst) + 1;
	let entry = LogLine {
		seq,
		line: clean.clone(),
	};
	{
		let mut recent = lock(&state.recent);
		if recent.len() >= LOG_TAIL {
			recent.pop_front();
		}
		recent.push_back(entry.clone());
	}
	if let Some((version, commit)) = parse_banner_line(&clean) {
		let mut info = lock(&state.build_info);
		info.version = version;
		info.commit = commit;
	}
	if let Some(target) = parse_listen_line(&clean) {
		lock(&state.backend).target = Some(target);
	}
	let _ = app.emit("backend-log", &entry);
}

/// 后端横幅带 ANSI 颜色（`NarraFork\x1b[0m v0.9.11`），解析前先去掉。
fn strip_ansi(input: &str) -> String {
	let mut output = String::with_capacity(input.len());
	let mut chars = input.chars().peekable();
	while let Some(character) = chars.next() {
		if character == '\u{1b}' {
			if chars.peek() == Some(&'[') {
				chars.next();
				while let Some(next) = chars.next() {
					if next.is_ascii_alphabetic() {
						break;
					}
				}
			}
			continue;
		}
		output.push(character);
	}
	output
}

fn append_log(state: &AppState, line: &str) {
	if let Ok(mut file) = OpenOptions::new()
		.create(true)
		.append(true)
		.open(&state.log_path)
	{
		let _ = writeln!(file, "{line}");
	}
}

fn tail(state: &AppState) -> String {
	let recent = lock(&state.recent);
	if recent.is_empty() {
		return "（后端暂无输出）".to_string();
	}
	recent
		.iter()
		.rev()
		.take(DIAGNOSTIC_LINES)
		.rev()
		.map(|entry| entry.line.clone())
		.collect::<Vec<_>>()
		.join("\n")
}

fn parse_listen_line(line: &str) -> Option<ListenTarget> {
	const MARKER: &str = "running on ";
	let start = line.find(MARKER)? + MARKER.len();
	let rest = &line[start..];
	let end = rest
		.find(|character: char| {
			character.is_whitespace() || character == '"' || character == ',' || character == '}'
		})
		.unwrap_or(rest.len());
	let raw = &rest[..end];
	let parsed = tauri::Url::parse(raw).ok()?;
	let host = parsed.host_str()?.to_string();
	let port = parsed.port_or_known_default()?;
	Some(ListenTarget {
		scheme: parsed.scheme().to_string(),
		host,
		port,
	})
}

/// 后端启动横幅：`NarraFork v0.9.11 (abc1234)`。版本行先于 `running on` 输出。
fn parse_banner_line(line: &str) -> Option<(Option<String>, Option<String>)> {
	const MARKER: &str = "NarraFork v";
	let start = line.find(MARKER)? + MARKER.len();
	let rest = &line[start..];
	let version_end = rest
		.find(|character: char| character.is_whitespace() || character == ')')
		.unwrap_or(rest.len());
	let version = rest[..version_end].trim();
	if version.is_empty() {
		return None;
	}
	let commit = rest.find('(').and_then(|open| {
		let tail = &rest[open + 1..];
		let end = tail.find(')')?;
		let commit = tail[..end].trim();
		(!commit.is_empty()).then(|| commit.to_string())
	});
	Some((Some(version.to_string()), commit))
}

/// 停止后端。优先走 manager 伴生控制端口的优雅退出，超时再杀进程树。
pub fn stop(_app: &AppHandle, state: &AppState) {
	let (child, target) = {
		let mut handle = lock(&state.backend);
		(handle.child.take(), handle.target.take())
	};
	let Some(mut child) = child else {
		return;
	};

	if let Some(target) = &target {
		request_graceful_stop(state, target);
	}

	let deadline = Instant::now() + STOP_TIMEOUT;
	loop {
		match child.try_wait() {
			Ok(Some(_)) => return,
			Ok(None) => {}
			Err(_) => break,
		}
		if Instant::now() >= deadline {
			break;
		}
		thread::sleep(Duration::from_millis(100));
	}

	let _ = child.kill();
	let _ = child.wait();
}

fn request_graceful_stop(state: &AppState, target: &ListenTarget) {
	// 伴生控制端口只监听回环：监听 0.0.0.0 / 局域网地址时也必须往 127.0.0.1 发。
	let host = if config::is_loopback_host(&target.host) {
		target.host.clone()
	} else {
		"127.0.0.1".to_string()
	};
	let url = format!(
		"{}://{}:{}/api/system/lifecycle/signal",
		target.scheme,
		config::url_host(&host),
		target.port
	);
	let _ = state
		.http
		.post(url)
		.bearer_auth(&state.token)
		.timeout(Duration::from_secs(5))
		.send();
}

pub fn restart(app: &AppHandle, state: &Arc<AppState>) {
	if state.booting.swap(true, Ordering::SeqCst) {
		return;
	}

	if let Some(window) = app.get_webview_window("main") {
		let _ = window.close();
	}
	window::ensure_splash(app);
	set_state(app, state, BootState::checking("正在重启本机核心…"));

	let app = app.clone();
	let state = state.clone();
	thread::spawn(move || {
		stop(&app, &state);
		state.booting.store(false, Ordering::SeqCst);
		start(app, state);
	});
}

pub fn restart_from_tray(app: &AppHandle) {
	if let Some(state) = app.try_state::<Arc<AppState>>() {
		restart(app, state.inner());
	}
}

pub fn open_in_browser(app: &AppHandle) {
	let url = app
		.try_state::<Arc<AppState>>()
		.and_then(|state| state.current_url());
	match url {
		Some(url) => {
			let _ = tauri_plugin_opener::open_url(&url, None::<&str>);
		}
		None => window::focus_or_create(app),
	}
}

pub fn open_logs(app: &AppHandle) {
	if let Some(state) = app.try_state::<Arc<AppState>>() {
		let _ = tauri_plugin_opener::open_path(state.log_dir.clone(), None::<&str>);
	}
}

#[cfg(windows)]
mod job {
	use std::os::windows::io::AsRawHandle;
	use std::process::Child;

	use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
	use windows_sys::Win32::System::JobObjects::{
		AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
		SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
		JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
	};

	pub struct JobHandle(HANDLE);

	// 句柄本身可以跨线程使用；关闭只发生在 AppState 析构时。
	unsafe impl Send for JobHandle {}
	unsafe impl Sync for JobHandle {}

	impl JobHandle {
		pub fn create() -> Option<Self> {
			unsafe {
				let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
				if handle.is_null() {
					return None;
				}

				let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
				info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
				let configured = SetInformationJobObject(
					handle,
					JobObjectExtendedLimitInformation,
					&info as *const _ as *const core::ffi::c_void,
					std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
				);
				if configured == 0 {
					CloseHandle(handle);
					return None;
				}
				Some(Self(handle))
			}
		}

		pub fn assign(&self, child: &Child) {
			unsafe {
				let _ = AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE);
			}
		}
	}

	impl Drop for JobHandle {
		fn drop(&mut self) {
			unsafe {
				CloseHandle(self.0);
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_plain_listen_line() {
		let target = parse_listen_line("NarraFork server running on http://127.0.0.1:7780").unwrap();
		assert_eq!(target.base_url(), "http://127.0.0.1:7780");
	}

	#[test]
	fn parses_json_wrapped_listen_line() {
		let line = r#"{"level":"info","message":"NarraFork server running on https://[::1]:8890"}"#;
		let target = parse_listen_line(line).unwrap();
		assert_eq!(target.base_url(), "https://[::1]:8890");
	}

	#[test]
	fn ignores_unrelated_lines() {
		assert!(parse_listen_line("script started").is_none());
	}

	#[test]
	fn parses_banner_version_and_commit() {
		let line = "\u{1b}[1m\u{1b}[38;5;105m⛏  NarraFork\u{1b}[0m v0.9.11 (abc1234)";
		let (version, commit) = parse_banner_line(&strip_ansi(line)).unwrap();
		assert_eq!(version.as_deref(), Some("0.9.11"));
		assert_eq!(commit.as_deref(), Some("abc1234"));
	}

	#[test]
	fn parses_banner_with_ansi_reset() {
		let line = "\u{1b}[1m\u{1b}[38;5;105m⛏  NarraFork\u{1b}[0m v0.9.11 (4d64e9ef8)";
		let (version, commit) = parse_banner_line(&strip_ansi(line)).unwrap();
		assert_eq!(version.as_deref(), Some("0.9.11"));
		assert_eq!(commit.as_deref(), Some("4d64e9ef8"));
	}

	#[test]
	fn parses_banner_without_commit() {
		let (version, commit) = parse_banner_line("  ⛏  NarraFork v0.9.11").unwrap();
		assert_eq!(version.as_deref(), Some("0.9.11"));
		assert!(commit.is_none());
	}
}
