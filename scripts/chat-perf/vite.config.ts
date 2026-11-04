import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '../..');
export default defineConfig({
    root: path.join(root, 'scripts/chat-perf'),
    publicDir: path.join(root, 'tmp/chat-perf/fixtures'),
    plugins: [react(), tailwindcss()],
    resolve: { alias: [
        { find: /^@tauri-apps\/.*$/, replacement: path.join(root, 'scripts/chat-perf/tauri.ts') },
        { find: '@', replacement: path.join(root, 'src') },
    ] },
    build: { outDir: path.join(root, 'tmp/chat-perf/site'), emptyOutDir: true },
    server: { host: '127.0.0.1', port: 4179, strictPort: true, fs: { allow: [root] } },
    preview: { host: '127.0.0.1', port: 4179, strictPort: true },
});
