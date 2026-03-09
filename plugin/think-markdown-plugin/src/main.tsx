var ThinkMarkdownPlugin = function () {
  this.systemApi = null;
};

ThinkMarkdownPlugin.prototype.config = function () {
  return {
    name: "Think Markdown Plugin",
    type: ["markdownType", "interfaceType"],
  };
};

ThinkMarkdownPlugin.prototype.onPluginLoad = function (systemApi: SystemApi) {
  this.systemApi = systemApi || null;
  if (!this.systemApi || typeof this.systemApi.registerMarkdownTag !== "function") {
    return;
  }
  this.systemApi.registerMarkdownTag({
    tagName: "think",
    render: function ({ children, attributes }) {
      var summary = attributes.summary || "思考过程";
      return React.createElement(
        "details",
        {
          className: "my-3 rounded-lg border border-primary/20 bg-primary/5 px-3 py-2 text-sm text-foreground",
          open: false,
        },
        React.createElement(
          "summary",
          {
            className: "cursor-pointer select-none font-medium text-primary",
          },
          summary,
        ),
        React.createElement(
          "div",
          {
            className: "mt-2 whitespace-pre-wrap text-muted-foreground",
          },
          children,
        ),
      );
    },
  });
};

(window as any)["think-markdown-plugin"] = ThinkMarkdownPlugin;
(window as any).ThinkMarkdownPlugin = ThinkMarkdownPlugin;
