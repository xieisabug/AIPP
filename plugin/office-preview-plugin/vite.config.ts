import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  root: path.resolve(import.meta.dirname, "src/viewer"),
  base: "./",
  plugins: [react()],
  worker: {
    format: "es",
  },
  build: {
    outDir: path.resolve(import.meta.dirname, "dist/viewer"),
    emptyOutDir: true,
    target: "es2022",
    sourcemap: false,
  },
});
