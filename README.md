# NarraFork Desktop

NarraFork 的非官方桌面壳（Tauri v2 + WebView2）。启动时拉起本机 NarraFork 核心，主窗口直接加载核心自己提供的 SPA；壳负责启动动画、日志、托盘、单实例与进程回收。

> Unofficial desktop shell for NarraFork. Not affiliated with the NarraFork project.

## 特性

- **蓝色启动动画**：项目 logo、环形光晕、进度条与实时启动日志；核心就绪后进入工作台。
- **不探测、不复用外部实例**：每次由壳拉起自己管理的核心，端口与就绪状态只来自后端的 `NarraFork server running on …` 启动行。
- **与后端同源**：不打包前端副本。登录态、localStorage、Service Worker 全部按浏览器语义工作，UI 与后端版本天然一致。
- **进程不残留**：后端子进程加入 Windows Job Object（`KILL_ON_JOB_CLOSE`），壳崩溃时一起结束。
- **托盘与单实例**：显示窗口、在浏览器打开、重启后端、打开日志、源码与许可证、退出。关闭主窗口留在托盘，二次启动或托盘点击会直接恢复主窗口。
- **最小权限**：远程页面不授予 Tauri IPC；外部链接交给系统浏览器。
- **无控制台窗口**：Windows 上 debug 构建也使用 GUI 子系统，日志写入文件。

## 依赖

| 场景 | 需要 |
|---|---|
| 运行发布版 | Windows 10/11 + WebView2 Runtime |
| 开发 | Bun 1.4+、Rust（`x86_64-pc-windows-gnu`）、MinGW（`gcc` / `dlltool` / `windres`） |
| 构建发布版 | 以上开发依赖 + NarraFork 源码（用于生成 sidecar） |

## 快速开始（开发）

本仓库默认作为 NarraFork 仓库的 `desktop/` 子目录使用：

```sh
# 1. 构建 NarraFork 前端（后端在 dev 模式下不自带 SPA）
cd NarraFork
bun install
bun run build

# 2. 启动桌面壳（会自动起 Vite、壳和后端）
cd desktop
bun install
bun run tauri dev
```

后端首次启动会在 `server/db/index.ts` 顶层完成数据库迁移，不需要额外命令。

如果本仓库不在 NarraFork 的 `desktop/` 下，用 `NF_GUI_BACKEND` 指向一个已有的 `narrafork.exe`，壳会跳过源码启动路径。

## 构建正式版

```sh
# 1. 准备 sidecar：优先复用 ../dist 里的 Windows 构建，没有就调用 bun run build:windows
bun run prepare:sidecar

# 或明确指定已有后端
bun run prepare:sidecar -- --binary D:\path\to\narrafork.exe --skip-build

# 2. 构建 release + NSIS 安装包
bun run bundle
```

产物：

```
src-tauri/target/release/narrafork-desktop.exe
src-tauri/target/release/bundle/nsis/NarraFork Desktop_0.1.0_x64-setup.exe
```

安装包会自动带上 sidecar。若直接分发 `target/release` 下的裸 exe，需要把 `narrafork.exe` 放在同一目录；否则壳会报告“未找到 NarraFork 核心”。

`prepare-sidecar` 会生成 `sidecar.json`，记录后端版本、commit、源码地址与 SHA-256——用于 MPL 源码披露和问题定位。

## 工作原理

```
Tauri Shell (Rust)
├─ boot      启动核心 → 解析 running on → 打开窗口
├─ backend   Command + Job Object、stdout 解析、优雅停止
├─ window    主窗口导航到后端 origin；外链 → 系统浏览器
└─ tray      显示/隐藏、打开浏览器、重启后端、日志、退出
        │
        ▼
  narrafork.exe --serve  ── 数据目录 ~/.narrafork
```

只依赖三个进程级约定：

1. `--serve` 启动服务模式。
2. 启动日志 `NarraFork server running on http(s)://host:port`：端口被占用时服务端会顺延，壳以这行为准。
3. 退出走 `POST /api/system/lifecycle/signal`（启动时注入的 manager token），超时再杀进程树。

不读取、不写入 NarraFork 的 `settings.json` 监听字段；唯一的端口覆盖入口是壳配置里的 `portOverride`（透传为 `--port`）。

## 配置与日志

壳配置：`%APPDATA%\app.narrafork.desktop\config.json`

```json
{
  "portOverride": null,
  "closeToTray": true
}
```

- `portOverride`：传给后端的 `--port`；`null` 表示完全跟随 NarraFork 自己的配置。
- `closeToTray`：关闭窗口时是否留在托盘（默认 `true`）。

后端日志：托盘「打开日志」，或：

```
%LOCALAPPDATA%\app.narrafork.desktop\logs\backend.log
```

环境变量：

| 变量 | 作用 |
|---|---|
| `NF_GUI_BACKEND=<exe 或 .ts>` | 指定后端可执行文件/脚本，跳过默认解析 |
| `NF_GUI_BUN=<bun.exe>` | 指定开发模式使用的 Bun；GUI 进程通常拿不到 npm 的 `bun.cmd` 垫片 |
| `NF_GUI_DEV=1` | 在 release 构建里也走开发模式的后端启动路径 |

## 排错

| 症状 | 原因与处理 |
|---|---|
| `找不到 Bun` | GUI 进程的 PATH 里只有 `bun.cmd` 垫片。用 `NF_GUI_BUN` 指向 `<prefix>/node_modules/bun/bin/bun.exe`，或把真正的 `bun.exe` 加入 PATH |
| `开发模式缺少前端构建` | 在 NarraFork 仓库根执行 `bun run build`，生成 `dist/frontend/index.html` |
| `本机核心启动失败` + 实例锁 | 同机已有实例占用 `~/.narrafork`。关闭那个实例；需要并行时用 `NARRAFORK_HOME` 指向不同目录 |
| 页面空白 / 证书错误 | NarraFork 启用了自签 TLS 时，WebView2 会校验证书。把内置 CA 加入系统信任，或改用 HTTP |
| Release 构建找不到 sidecar | 先运行 `bun run prepare:sidecar`；确认 `src-tauri/binaries/narrafork-x86_64-pc-windows-gnu.exe` 存在 |
| crates 下载慢 | `.cargo/config.toml` 已配置 rsproxy 镜像；不在国内网络可删除该文件 |

## 目录

```
├── PLAN.md                    设计与里程碑（含 MPL 合规清单）
├── index.html / src/          启动页（Vite）
├── scripts/prepare-sidecar.ts sidecar 准备与 sidecar.json
└── src-tauri/
    ├── src/main.rs            组装、单实例、退出
    ├── src/backend.rs         核心生命周期、日志解析、Job Object
    ├── src/window.rs          splash / 主窗口 / 外链策略
    ├── src/config.rs          壳配置与主机名工具
    ├── src/tray.rs            托盘菜单
    └── capabilities/          仅 splash 拥有 IPC
```

## 测试

```sh
cd src-tauri && cargo test     # 8 个单元测试：日志/横幅解析、回环判断
bun run build                  # tsc 类型检查 + Vite 构建
```

## 许可证与合规

本仓库代码采用 MPL-2.0。NarraFork 同样是 MPL-2.0：**壳本身可以任意许可证，只有在分发 `narrafork.exe` 时才产生义务**。

发布内置 sidecar 的安装包时：

1. 安装包会展示 MPL-2.0 全文（`bundle.licenseFile`），安装目录同时附带 `LICENSE`。
2. `sidecar.json` 随安装目录分发，记录 sidecar 的 `version + commit + sourceUrl`；托盘「源码与许可证」会打开对应源码地址。
3. 保留后端自带的 `/licenses` 页面（约 1280 个第三方组件，含 Bun/JSC 的 LGPL-2 relink 说明）。
4. 不要复制 NarraFork 源码文件进壳；所需逻辑重新实现。

商标不在 MPL 授权范围内。启动页与应用图标引用 NarraFork 自带的品牌资源（`frontend/public/pwa-512x512.png`）；本项目仍标注为非官方外壳。

## 已知限制

- Windows 优先：Job Object、sidecar 命名与 WebView2 都是 Windows 路径；macOS/Linux 尚未适配。
- WebAuthn/Passkey、OAuth 弹窗、下载行为仍需真实使用验收。
- 暂无自动更新；壳与 NarraFork 后端的更新是两条独立通道。
- 开发模式依赖 Vite 与 Bun；发布版只依赖 sidecar。
