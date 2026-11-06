//! 窗口：本地 splash + 后端 origin 的主窗口。
//!
//! 主窗口是 `WebviewUrl::External`，不打包前端副本——SPA 与 API 同源，token、Storage、
//! Service Worker 全部按浏览器语义工作。capability 只覆盖 splash，远程页面拿不到 IPC。

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub fn create_splash(app: &AppHandle) -> tauri::Result<()> {
	if app.get_webview_window("splash").is_some() {
		return Ok(());
	}

	WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("index.html".into()))
		.title("NarraFork Desktop")
		.inner_size(520.0, 600.0)
		.resizable(false)
		.maximizable(false)
		.theme(Some(tauri::Theme::Dark))
		.center()
		.build()?;

	Ok(())
}

/// 重启后端时使用：splash 可能已被主窗口替换，需要重新创建。
pub fn ensure_splash(app: &AppHandle) {
	if let Some(window) = app.get_webview_window("splash") {
		let _ = window.show();
		let _ = window.set_focus();
		return;
	}
	let _ = create_splash(app);
}

/// 托盘/二次启动：优先主窗口，其次已就绪的 URL，最后 splash。
pub fn focus_or_create(app: &AppHandle) {
	if let Some(window) = app.get_webview_window("main") {
		let _ = window.show();
		let _ = window.unminimize();
		let _ = window.set_focus();
		return;
	}

	// 用户关掉主窗口后应用仍留在托盘；此时后端已就绪，直接重建主窗口，
	// 不要退回到 splash 再等一次启动流程。
	if let Some(state) = app.try_state::<std::sync::Arc<crate::backend::AppState>>() {
		if let Some(url) = state.current_url() {
			if show_main(app, &url).is_ok() {
				return;
			}
		}
	}

	if let Some(window) = app.get_webview_window("splash") {
		let _ = window.show();
		let _ = window.unminimize();
		let _ = window.set_focus();
		return;
	}
	let _ = create_splash(app);
}

pub fn show_main(app: &AppHandle, url: &str) -> Result<(), String> {
	if let Some(window) = app.get_webview_window("main") {
		let _ = window.show();
		let _ = window.set_focus();
		return Ok(());
	}

	let parsed = tauri::Url::parse(url).map_err(|error| error.to_string())?;
	let origin = parsed.origin().ascii_serialization();

	let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(parsed))
		.title("NarraFork")
		.inner_size(1440.0, 900.0)
		.min_inner_size(960.0, 640.0)
		.theme(Some(tauri::Theme::Dark))
		.center()
		.on_navigation(move |url| handle_navigation(url, &origin))
		.build()
		.map_err(|error| error.to_string())?;

	let _ = window.set_focus();

	if let Some(splash) = app.get_webview_window("splash") {
		let _ = splash.close();
	}

	Ok(())
}

/// 同源导航留在窗口内；`blob:`/`data:` 用于预览与下载；其余交给系统浏览器。
fn handle_navigation(url: &tauri::Url, origin: &str) -> bool {
	let scheme = url.scheme();
	if scheme == "about" || scheme == "blob" || scheme == "data" {
		return true;
	}
	if url.origin().ascii_serialization() == origin {
		return true;
	}

	let target = url.to_string();
	// 导航回调在 UI 线程上，外部打开可能阻塞，交给独立线程。
	std::thread::spawn(move || {
		let _ = tauri_plugin_opener::open_url(&target, None::<&str>);
	});
	false
}
