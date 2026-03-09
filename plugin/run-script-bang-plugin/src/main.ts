var RunScriptBangPlugin = function () {};

RunScriptBangPlugin.prototype.config = function () {
  return {
    name: "Run Script Bang Plugin",
    type: ["toolType", "applicationType"],
  };
};

RunScriptBangPlugin.prototype.onPluginLoad = function () {
  return undefined;
};

(window as any)["run-script-bang-plugin"] = RunScriptBangPlugin;
(window as any).RunScriptBangPlugin = RunScriptBangPlugin;
