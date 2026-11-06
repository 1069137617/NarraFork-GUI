import { defineConfig } from "vite";

// 桌面壳只构建 splash / 错误页；主界面由 NarraFork 后端自己提供。
export default defineConfig({
	clearScreen: false,
	server: {
		port: 5174,
		strictPort: true,
	},
	build: {
		target: "chrome105",
		outDir: "dist",
		emptyOutDir: true,
	},
});
