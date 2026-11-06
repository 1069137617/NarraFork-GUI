# NarraFork Desktop Shell — 计划

Tauri v2 桌面壳：原生窗口直接加载**本机 NarraFork 后端自己提供的 SPA**，壳负责后端生命周期、窗口、托盘、单实例与外链。

## 1. 为什么是这条路

| 方案 | 结论 |
|---|---|
| WebView 加载后端 SPA（本方案） | 同源浏览，token/localStorage/Service Worker 全部天然可用；UI 与后端永远同版本 |
| 打包前端副本 + 自定义 origin 嵌入 | 需要 `server.allowedOrigins`、`nfEmbed` 宿主桥，且内置 SPA 会与后端版本漂移 |
| 原生重写 UI（External API v1） | 只能做伴侣客户端：无审批交互、无终端、无文件浏览，能力边界太大 |

协议面只需要三个进程级约定，不碰应用层 API：

1. `--serve` 启动服务模式（`server/index.ts`）
2. 启动日志 `NarraFork server running on http(s)://host:port`；端口被占用时服务端会顺延，壳以这行为准（`server/main.ts:1331`）
3. 退出走 `POST /api/system/lifecycle/signal`（manager token，`server/lib/manager-control-server.ts`）

## 2. 架构

```
Tauri Shell (Rust)
├─ boot        启动核心 → 解析 running on → 打开窗口
├─ backend     Command(+Job Object)、stdout 解析、就绪判定、优雅停止
├─ window      主窗口导航到后端 origin；外链 → 系统浏览器
└─ tray        显示/隐藏、打开浏览器、重启后端、日志、退出
        │
        ▼
  narrafork.exe --serve  ── 数据目录 ~/.narrafork
```

设计约束：

- **不探测、不复用外部实例**：每次由壳拉起自己管理的核心，端口与就绪状态只来自后端启动日志。
- **不打包前端副本**，主窗口是 `WebviewUrl::External(http://localhost:<port>/)`。
- **不给远程页面 Tauri IPC**：capability 只覆盖本地 splash 窗口。
- **不改 NarraFork 后端源码**（若最终必须加 `--no-open`，单独维护并公开 patch）。
- 数据目录沿用 `~/.narrafork`，不另建一套。

## 3. 后端生命周期

1. **启动**：一律由壳拉起自己的核心。
   - 开发：`bun server/index.ts --serve`（cwd = 仓库根，需先有 `dist/frontend`）
   - 发布：同目录 sidecar `narrafork.exe --serve`
   - 注入随机 `NARRAFORK_MANAGER_TOKEN` 与 `NARRAFORK_MANAGER_MANAGED=1`；子进程加入 Windows Job Object（`KILL_ON_JOB_CLOSE`）。
2. **确定地址**：解析后端 stdout 的 `NarraFork server running on …` 行，绝不写死 7778。端口被占用时服务端会顺延，这行是唯一真相。
3. **就绪**：看到该行即加载（此时迁移已完成、端口已绑定）；120s 未出现则报错并附最后 40 行日志。不再请求 `/api/health`。
4. **停止**：`POST /api/system/lifecycle/signal`（Bearer manager token，回环伴生控制端口），202/409 后等 15s，超时杀进程树。
5. **崩溃恢复**：主窗口保留；splash 显示错误、最后若干行日志、重启按钮。

同机已有实例占用数据目录时，新核心会因实例锁退出；壳展示退出码与日志，由用户先关闭那个实例。

## 4. 窗口与交互

- Splash（本地页面）：蓝色主题启动动画（环形光晕、logo 视差、进度条），实时显示后端 stdout 日志；就绪后停留约 0.9 秒再进入工作台。
- 错误态：状态点/进度条转红，展示退出码与最后 40 行日志，提供重试、打开日志、退出。
- 主窗口：`WebviewUrl::External`，同源导航放行；`http/https` 外链与 `mailto:` 交系统浏览器；`blob:`/`data:`/`about:blank` 放行。
- 托盘：显示/隐藏、在浏览器打开、重启后端、打开日志目录、退出。关闭按钮默认最小化到托盘。
- 单实例：第二次启动聚焦已有窗口。
- 深链 v1 不做；预留 `narrafork-gui://`，避免与后端 `narrafork://` reveal 协议冲突。

## 5. 里程碑

| 阶段 | 内容 | 验收 |
|---|---|---|
| M0 | Tauri 工程、splash、加载 SPA、外链拦截 | 窗口能打开本机 `http://localhost:7778` |
| M1 | 启动核心、日志解析、就绪判定、优雅停止、Job Object | 冷启动进主界面；看不到 `running on` 时报错并附日志；杀壳无残留 |
| M2 | 托盘、单实例、关闭到托盘、设置页、日志 | 托盘操作可用；二次启动聚焦 |
| M3 | sidecar 准备脚本、NSIS、图标、合规包 | 干净机器安装后可注册并进入主界面 |
| M4 | 崩溃恢复、开机自启、慢启动提示 | 杀后端后壳能提示并一键重启 |

## 6. MPL-2.0 合规（M3 必做）

NarraFork 是 MPL-2.0 文件级弱 copyleft。壳本身可以闭源；义务只在**分发 NarraFork 二进制**时触发。

### 6.1 内置 sidecar 时的四件事

- [x] 安装包展示 MPL-2.0 全文并随装 `LICENSE`（`bundle.licenseFile` + resources）。
- [x] 固定 sidecar 的 `version + commit`，安装目录带 `sidecar.json`；托盘「源码与许可证」打开对应源码链接（MPL §3.2(a)）。
- [x] 保留后端 `/licenses` 页面（约 1280 个第三方组件，含 Bun/JSC 的 LGPL-2 relink 说明），不替换或删除。
- [x] 不复制仓库源码进壳（`listen-log.ts`、`backend-process.ts`、`shared/*`）；所需逻辑重新实现。

### 6.2 若修改后端

- 仅自用、不分发：无义务。
- 随壳分发：被改的 MPL 文件必须以源码形式公开（公开 patch 或完整文件）。
- 优先用现有配置/参数规避改动，确需改动时单独维护 `patches/` 并公开。

### 6.3 命名与品牌

- MPL §2.3 不授予商标权；`server/lib/branding`、PWA 图标属于项目品牌资源。本壳按项目所有者要求引用 `frontend/public/pwa-512x512.png` 作为启动页 logo 与应用图标，并保留 “Unofficial, not affiliated” 说明。
- 若要把改动合回上游，才涉及 `CLA.md`。

### 6.4 分发模式备选

| 模式 | MPL 义务 | 体验 |
|---|---|---|
| 内置 sidecar（本计划） | 需要上述 4 项 | 开箱即用、可离线安装 |
| 首次运行下载官方 Release | 不构成你分发 | 需联网 |
| 依赖用户自行安装 | 不构成你分发 | 安装步骤多 |

## 7. 已知风险（需实测）

1. WebView2 的 WebAuthn / Passkey 与浏览器行为不完全一致；密码 + TOTP 一定可用。
2. `window.open` / OAuth 弹窗在 Tauri + WebView2 下的行为需要验证，必要时转系统浏览器。
3. 下载与文件选择的默认体验需要验收。
4. 端口漂移：绝不写死 7778，以 `running on` 日志为准。
5. 同机已有实例占用数据目录时核心会因实例锁退出；壳展示错误与日志，不做静默复用。
6. `server.openBrowser` 默认 `browser`，壳拉起后端时用 `NARRAFORK_MANAGER_MANAGED=1` 抑制，不再多弹浏览器窗口。

## 8. 目录

```
desktop/
├── PLAN.md
├── package.json / vite.config.ts / tsconfig.json
├── index.html / src/                 # splash
├── scripts/prepare-sidecar.ts        # sidecar 准备与 sidecar.json
├── scripts/make-icon.ts              # 生成自有图标
└── src-tauri/
    ├── Cargo.toml / build.rs / tauri.conf.json
    ├── capabilities/default.json
    ├── binaries/                     # sidecar（忽略）
    └── src/{main,backend,config,window,tray}.rs
```
