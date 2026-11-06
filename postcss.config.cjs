// 桌面壳的 splash 只有手写 CSS；这里阻断 Vite 向上找到仓库根目录里
// NarraFork 的 PostCSS 配置（它依赖 Mantine 插件，桌面壳不需要）。
module.exports = {
	plugins: [],
};
