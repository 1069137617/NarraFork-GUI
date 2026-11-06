// Windows 上始终使用 GUI 子系统：debug 构建也不额外弹一个控制台窗口。
// 启动日志由壳捕获并写入 %LOCALAPPDATA%\app.narrafork.desktop\logs\backend.log。
#![cfg_attr(windows, windows_subsystem = "windows")]

mod backend;
mod config;
mod tray;
mod window;

use std::sync::Arc;

use tauri::{Manager, RunEvent};

use crate::backend::AppState;

mod commands {
	use std::sync::Arc;

	use tauri::{AppHandle, State};

	use crate::backend::{self, AppState, BootState};

	#[tauri::command]
	pub fn boot_state(state: State<'_, Arc<AppState>>) -> BootState {
		state.snapshot()
	}

	#[tauri::command]
	pub fn backend_log_tail(state: State<'_, Arc<AppState>>) -> Vec<backend::LogLine> {
		state.log_tail(120)
	}

	#[tauri::command]
	pub fn retry_backend(app: AppHandle, state: State<'_, Arc<AppState>>) {
		backend::restart(&app, state.inner());
	}

	#[tauri::command]
	pub fn open_logs(app: AppHandle) {
		backend::open_logs(&app);
	}

	#[tauri::command]
	pub fn quit_app(app: AppHandle) {
		app.exit(0);
	}
}

fn main() {
	tauri::Builder::default()
		// single-instance must be the first plugin registered.
		.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
			window::focus_or_create(app);
		}))
		.plugin(tauri_plugin_opener::init())
		.invoke_handler(tauri::generate_handler![
			commands::boot_state,
			commands::backend_log_tail,
			commands::retry_backend,
			commands::open_logs,
			commands::quit_app
		])
		.setup(|app| {
			let shell_config = config::load_shell_config(app.handle());
			let state = Arc::new(AppState::new(app.handle(), shell_config));
			app.manage(state.clone());

			window::create_splash(app.handle())?;
			tray::install(app.handle())?;
			backend::start(app.handle().clone(), state);
			Ok(())
		})
		.build(tauri::generate_context!())
		.expect("failed to build NarraFork Desktop")
		.run(|app, event| match event {
			// Closing the last window keeps the app alive in the tray. Only an explicit
			// `app.exit(...)` (tray "退出") carries an exit code, and that path is allowed.
			RunEvent::ExitRequested { code, api, .. } => {
				if code.is_none() {
					api.prevent_exit();
				}
			}
			RunEvent::Exit => {
				if let Some(state) = app.try_state::<Arc<AppState>>() {
					backend::stop(app, state.inner());
				}
			}
			_ => {}
		});
}
