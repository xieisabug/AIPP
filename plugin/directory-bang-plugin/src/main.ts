var DirectoryBangPlugin = function () {};

DirectoryBangPlugin.prototype.config = function () {
  return {
    name: "Directory Bang Plugin",
    type: ["toolType", "applicationType"],
  };
};

DirectoryBangPlugin.prototype.onPluginLoad = function () {
  return undefined;
};

(window as any)["directory-bang-plugin"] = DirectoryBangPlugin;
(window as any).DirectoryBangPlugin = DirectoryBangPlugin;
