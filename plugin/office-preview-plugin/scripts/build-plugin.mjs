import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const pluginDir = path.resolve(import.meta.dirname, "..");
const distDir = path.join(pluginDir, "dist");
const viewerAssetsDir = path.join(distDir, "viewer", "assets");
const licensesOutDir = path.join(pluginDir, "assets", "licenses");

// 1. Build standalone BetterOffice viewer bundle into dist/viewer/
const viteBin = path.join(pluginDir, "node_modules", ".bin", process.platform === "win32" ? "vite.cmd" : "vite");
execFileSync(viteBin, ["build", "--config", path.join(pluginDir, "vite.config.ts")], {
  cwd: pluginDir,
  stdio: "inherit",
  shell: process.platform === "win32",
});

// 2. Prune JP/KR/TC OTF faces to reduce plugin archive by ~14 MB (keep Simplified Chinese SC faces)
if (fs.existsSync(viewerAssetsDir)) {
  const pruned = [];
  for (const name of fs.readdirSync(viewerAssetsDir)) {
    if (/^NotoSans(JP|KR|TC)-.*\.otf$/i.test(name)) {
      fs.rmSync(path.join(viewerAssetsDir, name), { force: true });
      pruned.push(name);
    }
  }
  console.log("[office-preview-plugin] Pruned unused CJK fonts:", pruned.join(", ") || "(none)");
}

// 3. Emit minimal AIPP plugin entry dist/main.js
const mainJs = `// Official AIPP Office preview plugin (BetterOffice 0.5.1).
// runtime.type is "viewer": the host never injects this file. All code runs inside the
// sandboxed iframe served from dist/viewer/ via the aipp-office:// protocol.
// The file exists only because the plugin manifest requires an entry.
`;
fs.mkdirSync(distDir, { recursive: true });
fs.writeFileSync(path.join(distDir, "main.js"), mainJs, "utf8");

// 4. Copy required Apache-2.0 and SIL OFL-1.1 license notices into assets/licenses/
fs.rmSync(licensesOutDir, { recursive: true, force: true });
fs.mkdirSync(licensesOutDir, { recursive: true });

const boRoot = path.join(pluginDir, "node_modules", "@betteroffice");
const copyIfExists = (src, dest) => {
  if (fs.existsSync(src)) {
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.cpSync(src, dest, { recursive: true });
  }
};
copyIfExists(path.join(boRoot, "docx", "LICENSE"), path.join(licensesOutDir, "BetterOffice-Apache-2.0-LICENSE.txt"));
copyIfExists(path.join(boRoot, "docx-react", "THIRD-PARTY-NOTICES.md"), path.join(licensesOutDir, "BetterOffice-THIRD-PARTY-NOTICES.md"));
copyIfExists(path.join(boRoot, "fonts", "LICENSES"), path.join(licensesOutDir, "fonts-latin"));
copyIfExists(path.join(boRoot, "fonts-cjk", "LICENSES"), path.join(licensesOutDir, "fonts-cjk"));

console.log("[office-preview-plugin] Build completed cleanly.");
