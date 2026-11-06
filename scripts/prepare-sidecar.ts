/**
 * 把 NarraFork 后端可执行文件放进 Tauri 的 sidecar 位置。
 *
 * Tauri 的 externalBin 命名规则：`binaries/narrafork-<target-triple>.exe`。
 * 目标三元组取当前 rustc host（本机是 x86_64-pc-windows-gnu），避免和 CLI 的默认
 * msvc 假设打架。
 *
 * 用法：
 *   bun scripts/prepare-sidecar.ts                  # 找 dist/ 里的构建，找不到就构建
 *   bun scripts/prepare-sidecar.ts --skip-build     # 只用已有 dist/
 *   bun scripts/prepare-sidecar.ts --binary <path>  # 明确指定 exe
 */
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
	copyFileSync,
	existsSync,
	mkdirSync,
	readFileSync,
	readdirSync,
	statSync,
	writeFileSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const desktopDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(desktopDir, "..");
const args = process.argv.slice(2);

function argument(name: string): string | undefined {
	const index = args.indexOf(name);
	if (index < 0) return undefined;
	const value = args[index + 1];
	return value && !value.startsWith("--") ? value : undefined;
}

function run(command: string, commandArgs: string[], cwd: string): string {
	return execFileSync(command, commandArgs, { cwd, encoding: "utf8" }).trim();
}

function resolveTriple(): string {
	const explicit = argument("--triple") ?? process.env.TAURI_ENV_TARGET_TRIPLE;
	if (explicit) return explicit;
	const output = run("rustc", ["-vV"], desktopDir);
	const match = /^host:\s*(\S+)$/m.exec(output);
	if (!match?.[1]) throw new Error("无法从 rustc -vV 解析 host triple，请传 --triple");
	return match[1];
}

function newestWindowsBuild(): string | undefined {
	const dist = join(repoRoot, "dist");
	if (!existsSync(dist)) return undefined;
	const candidates = readdirSync(dist)
		.filter((name) => /^narrafork-.*windows-(x64|x64-baseline)\.exe$/i.test(name))
		.map((name) => join(dist, name))
		.sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
	return candidates[0];
}

const triple = resolveTriple();
const target = join(desktopDir, "src-tauri", "binaries", `narrafork-${triple}.exe`);
mkdirSync(dirname(target), { recursive: true });

let source = argument("--binary");
if (source) {
	source = resolve(process.cwd(), source);
} else {
	source = newestWindowsBuild();
}

if (!source || !existsSync(source)) {
	if (args.includes("--skip-build")) {
		throw new Error(
			`找不到 Windows 构建产物。请先运行 \`bun run build:windows\`，或用 --binary 指定路径。`,
		);
	}
	console.log("未找到 dist/ 里的 Windows 构建，开始执行 bun run build:windows …");
	// 用当前 Bun 的真实 exe，避免 Windows 上 `bun` 只有 .cmd 垫片。
	execFileSync(process.execPath, ["run", "build:windows"], {
		cwd: repoRoot,
		stdio: "inherit",
	});
	source = newestWindowsBuild();
	if (!source) {
		throw new Error("构建完成但仍未在 dist/ 找到 narrafork-*-windows-x64.exe");
	}
}

copyFileSync(source, target);

const bytes = readFileSync(source);
const sha256 = createHash("sha256").update(bytes).digest("hex");
const rootPackage = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8")) as {
	version?: string;
};

let commit = "unknown";
try {
	commit = run("git", ["rev-parse", "HEAD"], repoRoot);
} catch {
	// 源码目录不是 git 仓库时不阻断打包。
}

let origin = "";
try {
	origin = run("git", ["remote", "get-url", "origin"], repoRoot);
} catch {
	// 没有 remote 时只记录空值。
}

const sourceUrl = origin
	? `${origin.replace(/^git@github\.com:/, "https://github.com/").replace(/\.git$/, "")}/tree/${commit}`
	: null;

const metadata = {
	version: rootPackage.version ?? "unknown",
	commit,
	sourceUrl,
	binary: basename(source),
	sha256,
	preparedAt: new Date().toISOString(),
};

const serialized = `${JSON.stringify(metadata, null, 2)}\n`;
writeFileSync(join(desktopDir, "sidecar.json"), serialized);
// 打包资源：安装目录内也放一份，便于接收者核对版本与源码地址。
writeFileSync(join(desktopDir, "src-tauri", "sidecar.json"), serialized);
console.log(`sidecar ready: ${target}`);
console.log(`  narrafork ${metadata.version} @ ${commit.slice(0, 8)}`);
console.log(`  sha256 ${sha256}`);
if (sourceUrl) console.log(`  source ${sourceUrl}`);
