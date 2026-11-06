//! 桌面壳自己的配置。
//!
//! 不读也不写 NarraFork 的 `settings.json`：监听地址由后端自己解析（`--port` 是唯一的
//! 覆盖入口），壳只关心“要不要覆盖端口”和“关闭窗口后是否留在托盘”。

use std::fs;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// 裸 IPv6 字面量必须加方括号，否则 URL 解析不出来。
pub fn url_host(host: &str) -> String {
	let bare = host.trim_matches(['[', ']']);
	if bare.contains(':') {
		format!("[{bare}]")
	} else {
		bare.to_string()
	}
}

pub fn is_loopback_host(host: &str) -> bool {
	let bare = host.trim_matches(['[', ']']).to_ascii_lowercase();
	if bare == "localhost" || bare == "::1" || bare == "0:0:0:0:0:0:0:1" {
		return true;
	}
	if let Some(rest) = bare.strip_prefix("127.") {
		let candidate = format!("127.{rest}");
		let parts: Vec<&str> = candidate.split('.').collect();
		return parts.len() == 4
			&& parts
				.iter()
				.all(|part| part.parse::<u16>().map(|n| n <= 255).unwrap_or(false));
	}
	if bare.starts_with("::ffff:127.") {
		return true;
	}
	false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShellConfig {
	/// 传给后端的 `--port` 覆盖；`None` 表示完全跟随 NarraFork 自己的 settings.json。
	pub port_override: Option<u16>,
	pub close_to_tray: bool,
}

impl Default for ShellConfig {
	fn default() -> Self {
		Self {
			port_override: None,
			close_to_tray: true,
		}
	}
}

pub fn load_shell_config(app: &AppHandle) -> ShellConfig {
	let Ok(dir) = app.path().app_config_dir() else {
		return ShellConfig::default();
	};
	let path = dir.join("config.json");
	let Ok(text) = fs::read_to_string(path) else {
		return ShellConfig::default();
	};
	serde_json::from_str(&text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ipv6_literals_are_bracketed() {
		assert_eq!(url_host("::1"), "[::1]");
		assert_eq!(url_host("[::1]"), "[::1]");
		assert_eq!(url_host("localhost"), "localhost");
	}

	#[test]
	fn loopback_detection() {
		assert!(is_loopback_host("localhost"));
		assert!(is_loopback_host("127.0.0.1"));
		assert!(is_loopback_host("127.12.3.4"));
		assert!(is_loopback_host("::1"));
		assert!(!is_loopback_host("192.168.1.2"));
	}
}
