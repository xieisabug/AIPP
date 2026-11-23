import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const repoRoot = process.cwd();
const pluginRoot = path.join(repoRoot, "plugin");
const outputRoot = path.join(repoRoot, "dist-official-plugins");
const stagingRoot = path.join(outputRoot, ".staging");
const repository = process.env.GITHUB_REPOSITORY || "xieisabug/AIPP";
const releaseTag = process.env.GITHUB_REF_NAME || "plugins-dev";
const releaseBaseUrl = `https://github.com/${repository}/releases/download/${releaseTag}`;

const optionalEntries = ["README.md", "README.MD", "readme.md", "LICENSE", "LICENSE.md", "assets", "avatars", "accessories"];

function toPosixPath(value) {
    return value.split(path.sep).join("/");
}

async function pathExists(value) {
    try {
        await fs.access(value);
        return true;
    } catch {
        return false;
    }
}

async function sha256File(filePath) {
    const bytes = await fs.readFile(filePath);
    return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function inferTags(manifest) {
    const tags = new Set(["official"]);
    for (const type of manifest.pluginTypes || manifest.pluginType || []) {
        const normalized = String(type).replace(/Type$/, "").toLowerCase();
        if (normalized) {
            tags.add(normalized);
        }
    }
    if (manifest.code === "run-script-bang-plugin") {
        tags.add("high-risk");
    }
    if (manifest.code === "benchmark-plugin") {
        tags.add("experimental");
    }
    if (manifest.code === "office-preview-plugin") {
        tags.add("office");
        tags.add("preview");
    }
    return Array.from(tags);
}

/**
 * Plugins that declare their own dependencies (e.g. office-preview-plugin bundles BetterOffice
 * with its own Vite build) need them installed before `build`; the rest build with root tooling.
 */
async function installPluginDependencies(pluginDir) {
    const packageJsonPath = path.join(pluginDir, "package.json");
    if (!(await pathExists(packageJsonPath))) {
        return;
    }
    const pkg = JSON.parse(await fs.readFile(packageJsonPath, "utf8"));
    const hasDeps = Object.keys(pkg.dependencies || {}).length + Object.keys(pkg.devDependencies || {}).length > 0;
    if (!hasDeps) {
        return;
    }
    const frozen = existsSync(path.join(pluginDir, "pnpm-lock.yaml")) ? ["--frozen-lockfile"] : [];
    execFileSync("pnpm", ["install", "--ignore-workspace", ...frozen], {
        cwd: pluginDir,
        stdio: "inherit",
        shell: process.platform === "win32",
    });
}

async function copyRuntimePackage(pluginDir, packageDir, manifest, entryChecksum) {
    const runtimeType = String(manifest.runtime?.type || "js").trim().toLowerCase();
    const isDeclarative = runtimeType === "declarative";
    const runtime = {
        ...(manifest.runtime || {}),
        type: runtimeType,
        entry: manifest.runtime?.entry || manifest.entry || (isDeclarative ? "plugin.json" : "dist/main.js"),
        ...(entryChecksum ? { checksum: entryChecksum } : {}),
    };
    const packagedManifest = {
        ...manifest,
        ...(isDeclarative ? {} : { entry: manifest.entry || runtime.entry }),
        runtime,
    };

    await fs.mkdir(packageDir, { recursive: true });
    await fs.writeFile(
        path.join(packageDir, "plugin.json"),
        `${JSON.stringify(packagedManifest, null, 2)}\n`,
        "utf8",
    );
    if (!isDeclarative) {
        await fs.cp(path.join(pluginDir, "dist"), path.join(packageDir, "dist"), { recursive: true });
    }

    for (const entry of optionalEntries) {
        const source = path.join(pluginDir, entry);
        if (existsSync(source)) {
            await fs.cp(source, path.join(packageDir, entry), { recursive: true });
        }
    }
}

async function packagePlugin(pluginDirName) {
    const pluginDir = path.join(pluginRoot, pluginDirName);
    const manifestPath = path.join(pluginDir, "plugin.json");
    const manifest = JSON.parse(await fs.readFile(manifestPath, "utf8"));
    const code = manifest.code || manifest.id || pluginDirName;

    if (code !== pluginDirName) {
        throw new Error(`Plugin directory and manifest code mismatch: ${pluginDirName} != ${code}`);
    }

    const runtimeType = String(manifest.runtime?.type || "js").trim().toLowerCase();
    const isDeclarative = runtimeType === "declarative";

    let entryChecksum = null;
    if (!isDeclarative) {
        await installPluginDependencies(pluginDir);
        execFileSync("npm", ["--prefix", pluginDir, "run", "build"], { stdio: "inherit", shell: process.platform === "win32" });

        const entry = manifest.runtime?.entry || manifest.entry || "dist/main.js";
        const entryPath = path.join(pluginDir, ...entry.split("/"));
        if (!(await pathExists(entryPath))) {
            throw new Error(`Plugin entry does not exist: ${toPosixPath(path.relative(repoRoot, entryPath))}`);
        }
        entryChecksum = await sha256File(entryPath);
    }

    const packageDir = path.join(stagingRoot, code);
    await fs.rm(packageDir, { recursive: true, force: true });
    await copyRuntimePackage(pluginDir, packageDir, manifest, entryChecksum);

    const version = manifest.version || "0.0.0";
    const zipName = `${code}-${version}.aipp-plugin.zip`;
    const zipPath = path.join(outputRoot, zipName);
    await fs.rm(zipPath, { force: true });
    execFileSync("zip", ["-qr", zipPath, code], { cwd: stagingRoot, stdio: "inherit" });

    const archiveChecksum = await sha256File(zipPath);
    const pluginTypes = manifest.pluginTypes || manifest.pluginType || [];
    const permissions = manifest.permissions || [];

    return {
        id: code,
        code,
        name: manifest.name || code,
        description: manifest.description || "",
        version,
        author: manifest.author || "AIPP",
        tags: inferTags({ ...manifest, code }),
        pluginTypes,
        permissions,
        minAippVersion: manifest.minAippVersion || "0.4.0",
        isExperimental: code === "benchmark-plugin",
        source: {
            type: "zip",
            url: `${releaseBaseUrl}/${zipName}`,
        },
        // The zip holds a single top-level `<code>/` folder, which the installer strips,
        // so the plugin lives at the archive root.
        dirs: [
            {
                from: ".",
                to: code,
            },
        ],
        sourceUrl: `https://github.com/${repository}/tree/main/plugin/${code}`,
        sha256: archiveChecksum,
    };
}

async function main() {
    await fs.rm(outputRoot, { recursive: true, force: true });
    await fs.mkdir(stagingRoot, { recursive: true });

    const entries = await fs.readdir(pluginRoot, { withFileTypes: true });
    // AIPP_PLUGINS=office-preview-plugin,foo packages only the listed plugins.
    const only = new Set(
        (process.env.AIPP_PLUGINS || "")
            .split(",")
            .map((value) => value.trim())
            .filter(Boolean),
    );
    const pluginDirs = entries
        .filter((entry) => entry.isDirectory())
        .map((entry) => entry.name)
        .filter((name) => only.size === 0 || only.has(name))
        .sort();
    const officialPlugins = [];

    for (const pluginDir of pluginDirs) {
        if (!(await pathExists(path.join(pluginRoot, pluginDir, "plugin.json")))) {
            continue;
        }
        officialPlugins.push(await packagePlugin(pluginDir));
    }

    await fs.writeFile(
        path.join(outputRoot, "official-plugins.json"),
        `${JSON.stringify(officialPlugins, null, 2)}\n`,
        "utf8",
    );
    await fs.rm(stagingRoot, { recursive: true, force: true });
    console.log(`Packaged ${officialPlugins.length} official plugins into ${toPosixPath(outputRoot)}`);
}

main().catch((error) => {
    console.error(error);
    process.exit(1);
});
