var GUOFENG_THEME_ID = "guofeng-zhusha";
var GUOFENG_THEME_NAME = "国风·朱砂";
var GUOFENG_THEME_VARIABLES: Record<string, string> = {
  "--background": "60 100% 97%",
  "--foreground": "357 45% 20%",
  "--card": "60 100% 97%",
  "--card-foreground": "357 45% 20%",
  "--popover": "60 100% 98%",
  "--popover-foreground": "357 45% 20%",
  "--primary": "357 73% 44%",
  "--primary-foreground": "60 100% 97%",
  "--secondary": "48 43% 92%",
  "--secondary-foreground": "357 50% 28%",
  "--muted": "50 38% 93%",
  "--muted-foreground": "357 24% 38%",
  "--accent": "50 38% 92%",
  "--accent-foreground": "357 60% 30%",
  "--border": "357 36% 80%",
  "--input": "357 36% 80%",
  "--ring": "357 73% 44%",
  "--action": "357 73% 44%",
  "--action-foreground": "60 100% 97%",
  "--icon": "357 73% 44%",
  "--icon-selected": "357 60% 30%",
  "--success": "357 73% 44%",
  "--success-foreground": "60 100% 97%",
  "--success-border": "357 60% 30%",
  "--shine-primary": "357 73% 50%",
  "--shine-secondary": "36 80% 68%",
  "--shine-tertiary": "357 46% 72%",
  "--sidebar": "60 52% 96%",
  "--sidebar-foreground": "357 35% 22%",
  "--sidebar-primary": "357 73% 44%",
  "--sidebar-primary-foreground": "60 100% 97%",
  "--sidebar-accent": "48 43% 92%",
  "--sidebar-accent-foreground": "357 60% 30%",
  "--sidebar-border": "357 26% 86%",
  "--sidebar-ring": "357 73% 44%",
};

var GuofengThemePlugin = class GuofengThemePlugin {
  systemApi: SystemApi | null;

  constructor() {
    this.systemApi = null;
  }

  config() {
    return {
      name: "国风主题插件",
      type: ["themeType", "interfaceType", "applicationType"],
    };
  }

  onPluginLoad(systemApi: SystemApi) {
    this.systemApi = systemApi || null;
    if (!this.systemApi || typeof this.systemApi.registerTheme !== "function") {
      return;
    }
    this.systemApi.registerTheme({
      id: GUOFENG_THEME_ID,
      label: GUOFENG_THEME_NAME,
      mode: "light",
      variables: GUOFENG_THEME_VARIABLES,
      description: "以朱砂红与象牙白构建的国风浅色主题",
    });
  }

  renderComponent() {
    var React = (window as any).React;
    if (!React) {
      return null;
    }
    return React.createElement(GuofengThemePanel, {
      systemApi: this.systemApi,
      themeId: GUOFENG_THEME_ID,
      themeName: GUOFENG_THEME_NAME,
    });
  }
};
