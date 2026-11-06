import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type Phase = "checking" | "starting" | "ready" | "error";

interface BootState {
	phase: Phase;
	message: string;
	detail?: string | null;
	url?: string | null;
	version?: string | null;
	commit?: string | null;
	logPath?: string | null;
	reused?: boolean | null;
}

interface LogLine {
	seq: number;
	line: string;
}

function el<T extends HTMLElement>(id: string): T {
	const node = document.getElementById(id);
	if (!node) throw new Error(`missing element #${id}`);
	return node as T;
}

const dot = el("dot");
const message = el("message");
const progress = el("progress");
const meta = el("meta");
const version = el("version");
const url = el("url");
const detail = el("detail");
const logEl = el<HTMLPreElement>("log");
const logHint = el("logHint");
const retry = el<HTMLButtonElement>("retry");
const logs = el<HTMLButtonElement>("logs");
const quit = el<HTMLButtonElement>("quit");

const MAX_LOG_LINES = 200;
const logLines: LogLine[] = [];
let lastSeq = 0;
let tailLoaded = false;
const pending: LogLine[] = [];

function appendLog(entry: LogLine | null | undefined): void {
	if (!entry || typeof entry.seq !== "number") return;
	if (entry.seq <= lastSeq) return;
	lastSeq = entry.seq;
	logLines.push(entry);
	if (logLines.length > MAX_LOG_LINES) logLines.shift();
	logEl.textContent = logLines.map((item) => item.line).join("\n");
	logEl.scrollTop = logEl.scrollHeight;
	logHint.textContent = `${entry.seq} 行`;
}

function render(state: BootState): void {
	message.textContent = state.message;
	dot.className = `dot ${state.phase}`;

	progress.className = "progress";
	if (state.phase === "ready") {
		progress.classList.add("done");
		logHint.textContent = "已就绪";
	} else if (state.phase === "error") {
		progress.classList.add("err");
		logHint.textContent = "启动失败";
	}

	const hasMeta = Boolean(state.version || state.url);
	meta.hidden = !hasMeta;
	version.textContent = state.version
		? state.commit
			? `${state.version} (${state.commit.slice(0, 8)})`
			: state.version
		: "—";
	url.textContent = state.url ?? "—";

	const showDetail = state.phase === "error" && Boolean(state.detail);
	detail.hidden = !showDetail;
	detail.textContent = showDetail ? (state.detail ?? "") : "";

	retry.hidden = state.phase !== "error";
	logs.hidden = state.phase !== "error";
	retry.disabled = false;
}

async function call(command: string): Promise<void> {
	try {
		await invoke(command);
	} catch (error) {
		console.error(`invoke ${command} failed`, error);
	}
}

async function main(): Promise<void> {
	const unlisteners: UnlistenFn[] = [];

	try {
		unlisteners.push(
			await listen<LogLine>("backend-log", (event) => {
				if (!tailLoaded) {
					pending.push(event.payload);
				} else {
					appendLog(event.payload);
				}
			}),
		);
		unlisteners.push(
			await listen<BootState>("boot-state", (event) => render(event.payload)),
		);

		// 页面加载可能晚于最早几行日志：先取历史，再冲刷监听期间收到的事件。
		const tail = await invoke<LogLine[]>("backend_log_tail");
		for (const entry of tail) appendLog(entry);
		tailLoaded = true;
		for (const entry of pending) appendLog(entry);
		pending.length = 0;

		render(await invoke<BootState>("boot_state"));
	} catch (error) {
		render({
			phase: "error",
			message: "桌面壳与本地核心通信失败",
			detail: String(error),
		});
	}

	window.addEventListener("beforeunload", () => {
		for (const unlisten of unlisteners) unlisten();
	});

	retry.addEventListener("click", () => {
		retry.disabled = true;
		logHint.textContent = "重新启动中";
		void call("retry_backend");
	});
	logs.addEventListener("click", () => void call("open_logs"));
	quit.addEventListener("click", () => void call("quit_app"));
}

void main();
