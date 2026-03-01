var ReactRuntime = (window as any).React as typeof import("react");

type GuofengUiComponent = import("react").ComponentType<any>;

function guofengFallbackComponent(tag: string): GuofengUiComponent {
  return function FallbackComponent(props: any) {
    var react = (window as any).React;
    if (!react || typeof react.createElement !== "function") {
      return null;
    }
    return react.createElement(tag, props, props?.children);
  };
}

function guofengResolveUi(systemApi: SystemApi | null) {
  var ui = (systemApi && systemApi.ui) || {};
  return {
    UIButton: ui.Button || guofengFallbackComponent("button"),
    UICard: ui.Card || guofengFallbackComponent("div"),
    UICardContent: ui.CardContent || guofengFallbackComponent("div"),
    UICardDescription: ui.CardDescription || guofengFallbackComponent("div"),
    UICardHeader: ui.CardHeader || guofengFallbackComponent("div"),
    UICardTitle: ui.CardTitle || guofengFallbackComponent("div"),
  };
}

function guofengExtractError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error || "未知错误");
}

function GuofengThemePanel(props: {
  systemApi: SystemApi | null;
  themeId: string;
  themeName: string;
}) {
  var { systemApi, themeId, themeName } = props;
  var { useEffect, useState } = ReactRuntime;
  var ui = guofengResolveUi(systemApi);
  var { UIButton, UICard, UICardContent, UICardDescription, UICardHeader, UICardTitle } = ui;
  var [activeTheme, setActiveTheme] = useState("default");
  var [statusText, setStatusText] = useState("");
  var [applying, setApplying] = useState(false);

  useEffect(() => {
    var cancelled = false;
    if (!systemApi || typeof systemApi.getDisplayConfig !== "function") {
      setStatusText("当前宿主未注入主题配置 API");
      return () => {};
    }
    (async () => {
      try {
        var displayConfig = await systemApi.getDisplayConfig();
        if (cancelled) {
          return;
        }
        setActiveTheme(String(displayConfig.theme || "default"));
      } catch (error) {
        if (cancelled) {
          return;
        }
        setStatusText("读取当前主题失败: " + guofengExtractError(error));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [systemApi]);

  var applyTheme = async () => {
    if (!systemApi || typeof systemApi.applyTheme !== "function") {
      setStatusText("当前宿主未注入主题应用 API");
      return;
    }
    setApplying(true);
    setStatusText("正在应用主题...");
    try {
      await systemApi.applyTheme(themeId);
      var displayConfig = await systemApi.getDisplayConfig();
      var nextTheme = String((displayConfig && displayConfig.theme) || themeId);
      setActiveTheme(nextTheme);
      setStatusText("主题已切换为「" + themeName + "」");
    } catch (error) {
      setStatusText("主题切换失败: " + guofengExtractError(error));
    } finally {
      setApplying(false);
    }
  };

  var resetTheme = async () => {
    if (!systemApi || typeof systemApi.applyTheme !== "function") {
      setStatusText("当前宿主未注入主题应用 API");
      return;
    }
    setApplying(true);
    setStatusText("正在恢复默认主题...");
    try {
      await systemApi.applyTheme("default");
      setActiveTheme("default");
      setStatusText("已恢复默认主题");
    } catch (error) {
      setStatusText("恢复默认主题失败: " + guofengExtractError(error));
    } finally {
      setApplying(false);
    }
  };

  return (
    <UICard>
      <UICardHeader>
        <UICardTitle>国风主题</UICardTitle>
        <UICardDescription>
          主色：朱砂红 #C01E25，底色：象牙白 #FFFFF0。适配浅色模式，强调国风视觉质感。
        </UICardDescription>
      </UICardHeader>
      <UICardContent className="space-y-4">
        <div className="grid grid-cols-2 gap-3">
          <div className="rounded-md border border-border/70 p-3 space-y-1">
            <div className="h-10 rounded-sm" style={{ backgroundColor: "#C01E25" }} />
            <div className="text-xs text-muted-foreground">朱砂红 / #C01E25</div>
          </div>
          <div className="rounded-md border border-border/70 p-3 space-y-1">
            <div className="h-10 rounded-sm border border-border/50" style={{ backgroundColor: "#FFFFF0" }} />
            <div className="text-xs text-muted-foreground">象牙白 / #FFFFF0</div>
          </div>
        </div>
        <div className="text-sm text-muted-foreground">
          当前主题：<span className="font-medium text-foreground">{activeTheme === themeId ? themeName : activeTheme}</span>
        </div>
        {statusText ? <div className="text-sm text-muted-foreground">{statusText}</div> : null}
        <div className="flex flex-wrap gap-2">
          <UIButton onClick={applyTheme} disabled={applying}>
            {applying ? "应用中..." : "应用国风主题"}
          </UIButton>
          <UIButton variant="outline" onClick={resetTheme} disabled={applying}>
            恢复默认主题
          </UIButton>
        </div>
      </UICardContent>
    </UICard>
  );
}
