import path from "path"
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import svgr from "vite-plugin-svgr"
import tailwindcss from '@tailwindcss/vite'

// https://vitejs.dev/config/
export default defineConfig(async () => ({
	plugins: [react(), svgr(), tailwindcss()],

	// Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
	//
	// 1. prevent vite from obscuring rust errors
	clearScreen: false,
	// 2. tauri expects a fixed port, fail if that port is not available
	server: {
		host: '0.0.0.0',
		port: 3000,
		strictPort: true,
		// HMR 配置 - 用于 Android 开发
		hmr: {
			// 使用环境变量或默认使用 localhost
			// Android 开发时需要设置 TAURI_DEV_HOST 为开发机器的局域网 IP
			host: process.env.TAURI_DEV_HOST || 'localhost',
			protocol: 'ws',
			port: 3000,
		},
		watch: {
			// 3. tell vite to ignore watching `src-tauri`
			ignored: ["**/src-tauri/**", '**/target/**'],
		},
	},
	resolve: {
		alias: {
			"@": path.resolve(__dirname, "./src"),
		},
	},
}));
