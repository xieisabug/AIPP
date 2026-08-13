const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const data = new Map();
let insertCount = 0;
const api = {
  storage: {
    execute: async ({ sql }) => ({ rowsAffected: sql.startsWith("INSERT OR IGNORE") ? (insertCount++ === 0 ? 1 : 0) : 0, lastInsertRowid: 0 }),
    query: async () => ({ columns: [], rows: [], rowCount: 0, truncated: false }),
  },
  hooks: { register() {}, unregister() {} },
  getData: async (key, session = "global") => data.get(`${session}:${key}`) ?? null,
  setData: async (key, value, session = "global") => { data.set(`${session}:${key}`, value); },
  toast: { success() {}, error() {}, info() {}, warning() {} },
  ui: {},
};
const context = {
  console,
  window: { React: { createElement: () => null, Fragment: Symbol("Fragment") } },
  setTimeout,
  clearTimeout,
};
vm.createContext(context);
const bundle = fs.readFileSync(path.join(__dirname, "../dist/main.js"), "utf8");
assert.match(bundle, /query:\s*\{\s*taskId:\s*"\$task_id"\s*\}/, "Kie polling must send the created task ID");
vm.runInContext(bundle, context);
assert.doesNotThrow(
  () => vm.runInContext(bundle, context),
  "plugin bundle must support repeated injection in one WebView"
);

assert.equal(
  context.comfyRenderInstruction("画面：{{assistant_reply}}", "山海"),
  "画面：山海",
  "assistant reply placeholder should be replaced"
);
assert.throws(() => context.comfyRenderInstruction("没有占位符", "山海"));

const workflow = context.comfyUiBuildWorkflow("new prompt");
assert.equal(workflow["57:27"].inputs.text, "new prompt");
assert.equal(workflow["57:3"].inputs.seed, 1047870638845959, "seed must remain fixed");
const customWorkflow = context.comfyUiBuildWorkflow("custom prompt", "9", "filename_prefix");
assert.equal(customWorkflow["9"].inputs.filename_prefix, "custom prompt");
assert.throws(() => context.comfyUiBuildWorkflow("custom prompt", "57:27", "missing"));

(async () => {
  const Plugin = context.window.ComfyUiImagePlugin;
  const plugin = new Plugin();
  await plugin.onPluginLoad(api);
  await plugin.setAutoEnabled(42, true);
  assert.equal(await plugin.getAutoEnabled(42), true);
  assert.equal(await plugin.getAutoEnabled(43), false, "auto switch must be conversation-scoped");
  assert.equal(await plugin.claimJob("auto:7", 42, 7, "auto"), true);
  assert.equal(await plugin.claimJob("auto:7", 42, 7, "auto"), false, "automatic job key must be idempotent");
  console.log("COMFYUI_IMAGE_PLUGIN_TEST_OK");
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
