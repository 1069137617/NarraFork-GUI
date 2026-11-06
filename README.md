# NarraFork Desktop

NarraFork 的**非官方**桌面壳（Tauri v2）。窗口直接加载本机 NarraFork 后端提供的 SPA，壳负责后端生命周期、托盘、单实例和外链。

> Unofficial desktop shell for NarraFork. Not affiliated with the NarraFork project.

## 它做什么

- 启动时拉起**自己管理的** NarraFork 核心（开发模式是 `bun server/index.ts --serve`，发布版是 sidecar），不探测、不复用外部实例。
- 端口与就绪状态从后端自己的 `NarraFork server running on …` 启动行读取，绝不写死 7778。
- 启动界面是蓝色动画 + 实时启动日志；看到 `running on` 后停留约 0.9 秒再进入工作台。
- 主窗口导航到 `http://localhost:<port>/`，与后端同源，登录态、Storage、Service Worker 全部按浏览器语义工作。
- 托盘：显示窗口、在浏览器打开、重启后端、打开日志、退出。
- 后端子进程放进 Windows Job Object，壳崩溃时不会留下孤儿进程。

## 它刻意不做

- 不打包前端副本（避免 UI 与后端版本漂移）。
- 不给远程页面 Tauri IPC：capability 只覆盖本地 splash。
- 不修改 NarraFork 后端源码。
- 不写 NarraFork 的 `settings.json`；监听地址仍由 NarraFork 自己的设置页或现有 manager 管理。

## 开发

前置：

- Windows 10/11 + WebView2 Runtime
- Bun 1.4+
- Rust 工具链（本机为 `x86_64-pc-windows-gnu`）
- MinGW（`gcc`、`dlltool`、`windres` 在 PATH）

```sh
cd desktop
bun install
bun run tauri dev
```

开发模式会从仓库根执行 `bun <repo>/server/index.ts --serve`。后端在 dev 模式下不再自带 SPA，所以第一次运行前需要先构建前端：

```sh
cd ..        # NarraFork 仓库根
bun run build
```

后端首次启动会在 `server/db/index.ts` 顶层完成数据库迁移，不需要额外命令。同机已有实例占用数据目录时，核心会因实例锁退出；壳会把退出码和最后几十行日志显示在启动页。

常用环境变量：

| 变量 | 作用 |
|---|---|
| `NF_GUI_URL=...` | （已移除）壳不再连接外部实例 |
| `NF_GUI_BACKEND=<exe 或 .ts>` | 指定后端可执行文件/脚本 |
| `NF_GUI_BUN=<bun.exe>` | 指定开发模式使用的 Bun；GUI 进程通常拿不到 npm 的 `bun.cmd` 垫片 |
| `NF_GUI_DEV=1` | 在 release 构建里也走开发模式的后端启动路径 |

Windows 上 debug 构建也使用 GUI 子系统，不会再额外弹控制台窗口；启动日志见托盘「打开日志」。

## 打包

```sh
cd desktop
bun run prepare:sidecar   # 找 dist/，没有就调用仓库根 bun run build:windows
bun run tauri build
```

产物在 `src-tauri/target/release/bundle/nsis/`。`prepare-sidecar` 会生成 `sidecar.json`，记录后端版本、commit、源码地址和 SHA-256——这是 MPL 源码披露与问题定位的依据。

## 配置与日志

- 壳配置：`%APPDATA%\app.narrafork.desktop\config.json`
  - `portOverride`：覆盖 NarraFork 的端口（默认 `null`，完全跟随 `settings.json`）
  - `closeToTray`：关闭窗口时最小化到托盘（默认 `true`）
- 后端日志：托盘「打开日志」，或 `%LOCALAPPDATA%\app.narrafork.desktop\logs\backend.log`

## 协议面

只依赖三个进程级约定，不碰应用层 API：

1. `--serve` 启动服务模式。
2. 启动日志 `NarraFork server running on http(s)://host:port`：端口被占用时服务端会顺延，壳以这行为准。
3. 退出走 `POST /api/system/lifecycle/signal`（带启动时注入的 manager token），超时再杀进程树。

## 许可证与合规

本仓库代码采用 MPL-2.0。NarraFork 同样是 MPL-2.0：**壳本身可以任意许可证，只有在分发 `narrafork.exe` 时才产生义务**。

发布内置 sidecar 的安装包时：

1. 安装目录附 MPL-2.0 全文。
2. 固定 sidecar 的 `version + commit`，在“关于”页给出对应源码链接（`sidecar.json` 已记录）。
3. 保留后端自带的 `/licenses` 页面（第三方组件披露）。
4. 不要复制 NarraFork 源码文件进壳；所需逻辑重新实现。

商标不在 MPL 授权范围内。启动页与应用图标引用 NarraFork 项目自带的品牌资源（`frontend/public/pwa-512x512.png`）；本项目仍标注为非官方外壳。
