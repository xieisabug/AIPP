var GUOFENG_THEME_ID = "guofeng-zhusha";
var GUOFENG_THEME_NAME = "国风·朱砂";
var GUOFENG_THEME_VARIABLES: Record<string, string> = {
  "--background": "60 100% 97%",
  "--foreground": "357 45% 20%",
  "--radius": "1rem",
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

var GUOFENG_THEME_EXTRA_CSS = `
:scope body {
  background-image:
    radial-gradient(circle at 12% 16%, rgba(192, 30, 37, 0.05), transparent 35%),
    radial-gradient(circle at 88% 84%, rgba(192, 30, 37, 0.04), transparent 32%);
}

:scope .rounded-sm {
  border-radius: calc(var(--radius) * 0.45);
}

:scope .rounded-md {
  border-radius: calc(var(--radius) * 0.72);
}

:scope .rounded-lg {
  border-radius: calc(var(--radius) + 0.12rem);
}

:scope .rounded-xl {
  border-radius: calc(var(--radius) + 0.34rem);
}

:scope .rounded-2xl {
  border-radius: calc(var(--radius) + 0.64rem);
}

:scope .border {
  border-color: hsl(var(--border) / 0.88);
}

:scope .shadow-sm {
  box-shadow:
    0 6px 16px -12px rgba(192, 30, 37, 0.4),
    0 1px 0 rgba(255, 255, 240, 0.65) inset !important;
}

:scope .shadow,
:scope .shadow-md,
:scope .shadow-lg,
:scope .shadow-xl {
  box-shadow:
    0 14px 28px -18px rgba(192, 30, 37, 0.42),
    0 2px 0 rgba(255, 255, 240, 0.72) inset !important;
}

:scope .bg-card,
:scope .bg-popover {
  backdrop-filter: saturate(108%);
}

:scope .bg-secondary {
  background-color: hsl(48 35% 92% / 0.9);
}

:scope .bg-muted {
  background-color: hsl(48 35% 93% / 0.82);
}

:scope .bg-primary {
  box-shadow: 0 8px 18px -12px rgba(192, 30, 37, 0.75);
}
`;

var GUOFENG_THEME_WINDOW_CSS: Record<string, string> = {
  chat_ui: `
:scope [data-aipp-slot="chat-input-send-button"][data-custom-visual="true"] {
  background: transparent !important;
  border: 0 !important;
  border-radius: 0 !important;
  box-shadow: none !important;
  overflow: visible;
}

:scope .guofeng-send-button-visual {
  width: 66px;
  height: 66px;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
  transform-origin: 50% 62%;
  filter: drop-shadow(0 10px 12px rgba(126, 20, 24, 0.24));
}

:scope .guofeng-send-button-visual img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  user-select: none;
}

:scope .guofeng-send-button-visual.responding {
  animation: guofeng-leaf-drift 2.8s ease-in-out infinite;
}

@keyframes guofeng-leaf-drift {
  0%, 100% { transform: translateX(0) translateY(0) rotate(-6deg); }
  42% { transform: translateX(-5px) translateY(-2px) rotate(7deg); }
  72% { transform: translateX(4px) translateY(1px) rotate(2deg); }
}

:scope .input-area-send-button {
  box-shadow:
    0 12px 24px -14px rgba(192, 30, 37, 0.72),
    0 0 0 1px rgba(255, 255, 240, 0.65) inset;
}

:scope [data-theme-slot="input-area-container"] {
  border-color: hsl(357 54% 52% / 0.62);
}
`,
  butler_experiment: `
:scope [data-aipp-slot="chat-input-send-button"][data-custom-visual="true"] {
  background: transparent !important;
  border: 0 !important;
  border-radius: 0 !important;
  box-shadow: none !important;
  overflow: visible;
}

:scope .guofeng-send-button-visual {
  width: 66px;
  height: 66px;
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
  transform-origin: 50% 62%;
  filter: drop-shadow(0 10px 12px rgba(126, 20, 24, 0.24));
}

:scope .guofeng-send-button-visual img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  user-select: none;
}

:scope .guofeng-send-button-visual.responding {
  animation: guofeng-leaf-drift 2.8s ease-in-out infinite;
}

@keyframes guofeng-leaf-drift {
  0%, 100% { transform: translateX(0) translateY(0) rotate(-6deg); }
  42% { transform: translateX(-5px) translateY(-2px) rotate(7deg); }
  72% { transform: translateX(4px) translateY(1px) rotate(2deg); }
}

:scope .input-area-send-button {
  box-shadow:
    0 12px 24px -14px rgba(192, 30, 37, 0.72),
    0 0 0 1px rgba(255, 255, 240, 0.65) inset;
}

:scope [data-theme-slot="input-area-container"] {
  border-color: hsl(357 54% 52% / 0.62);
}
`,
  config: `
:scope [data-theme-slot="settings-menu-container"] {
  background: linear-gradient(180deg, hsl(48 45% 94% / 0.95), hsl(48 38% 91% / 0.9));
}
`,
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
      extraCss: GUOFENG_THEME_EXTRA_CSS,
      windowCss: GUOFENG_THEME_WINDOW_CSS,
    });
  }

  renderView(viewId: string) {
    if (viewId !== "guofeng-theme-panel") {
      return null;
    }
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

  renderSlot(slotId: string, context?: Record<string, unknown>) {
    if (slotId !== "guofeng-send-button-icon") {
      return null;
    }
    // 叶子发送按钮是本主题的一部分，主题未激活时不渲染，避免“残留”在默认主题上
    var activeTheme = context && typeof context.activeTheme === "string" ? context.activeTheme : "";
    if (activeTheme !== GUOFENG_THEME_ID) {
      return null;
    }
    var React = (window as any).React;
    if (!React || !this.systemApi || typeof this.systemApi.assetUrl !== "function") {
      return null;
    }
    var isResponding = Boolean(context && context.isResponding);
    var assetPath = "assets/send-leaf.svg";
    return React.createElement(
      "span",
      {
        className: "guofeng-send-button-visual " + (isResponding ? "responding" : "idle"),
        "data-aipp-slot": isResponding ? "guofeng-stop-visual" : "guofeng-send-visual",
        style: {
          width: 66,
          height: 66,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          pointerEvents: "none",
        },
      },
      React.createElement("img", {
        src: this.systemApi.assetUrl(assetPath),
        alt: "",
        style: { width: "100%", height: "100%", objectFit: "contain", userSelect: "none" },
        draggable: false,
      })
    );
  }
};
