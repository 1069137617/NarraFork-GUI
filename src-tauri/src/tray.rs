//! 托盘菜单。关闭最后一个窗口不会退出应用，退出只走这里的“退出”。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;
use crate::backend;
use crate::window;

pub fn install(app: &AppHandle) -> tauri::Result<()> {
	let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
	let open = MenuItem::with_id(app, "open", "在浏览器中打开", true, None::<&str>)?;
	let restart = MenuItem::with_id(app, "restart", "重启后端", true, None::<&str>)?;
	let logs = MenuItem::with_id(app, "logs", "打开日志", true, None::<&str>)?;
	let about = MenuItem::with_id(app, "about", "源码与许可证", true, None::<&str>)?;
	let separator = PredefinedMenuItem::separator(app)?;
	let separator_two = PredefinedMenuItem::separator(app)?;
	let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

	let menu = Menu::with_items(
		app,
		&[
			&show,
			&open,
			&restart,
			&logs,
			&separator,
			&about,
			&separator_two,
			&quit,
		],
	)?;

	let mut builder = TrayIconBuilder::with_id("main")
		.menu(&menu)
		.tooltip("NarraFork Desktop")
		.show_menu_on_left_click(false)
		.on_menu_event(|app, event| match event.id.as_ref() {
			"show" => window::focus_or_create(app),
			"open" => backend::open_in_browser(app),
			"restart" => backend::restart_from_tray(app),
			"logs" => backend::open_logs(app),
			"about" => backend::open_source_info(app),
			"quit" => app.exit(0),
			_ => {}
		})
		.on_tray_icon_event(|tray, event| {
			if let TrayIconEvent::Click {
				button: MouseButton::Left,
				button_state: MouseButtonState::Up,
				..
			} = event
			{
				window::focus_or_create(tray.app_handle());
			}
		});

	if let Some(icon) = app.default_window_icon().cloned() {
		builder = builder.icon(icon);
	}

	builder.build(app)?;
	Ok(())
}
