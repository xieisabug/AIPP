type BenchmarkComponentType = import("react").ComponentType<any>;

function benchmarkFallbackComponent(tag: string): BenchmarkComponentType {
  return function FallbackComponent(props: any) {
    const react = (window as any).React;
    if (!react || typeof react.createElement !== "function") {
      return null;
    }
    return react.createElement(tag, props, props?.children);
  };
}

function benchmarkResolveUi(systemApi: SystemApi | null) {
  const ui = (systemApi && systemApi.ui) || {};
  return {
    UIAlert: ui.Alert || benchmarkFallbackComponent("div"),
    UIAlertDescription: ui.AlertDescription || benchmarkFallbackComponent("div"),
    UIBadge: ui.Badge || benchmarkFallbackComponent("span"),
    UIButton: ui.Button || benchmarkFallbackComponent("button"),
    UICard: ui.Card || benchmarkFallbackComponent("div"),
    UICardContent: ui.CardContent || benchmarkFallbackComponent("div"),
    UICardDescription: ui.CardDescription || benchmarkFallbackComponent("div"),
    UICardHeader: ui.CardHeader || benchmarkFallbackComponent("div"),
    UICardTitle: ui.CardTitle || benchmarkFallbackComponent("div"),
    UIDialog: ui.Dialog,
    UIDialogContent: ui.DialogContent,
    UIDialogDescription: ui.DialogDescription,
    UIDialogHeader: ui.DialogHeader,
    UIDialogTitle: ui.DialogTitle,
    UIInput: ui.Input || benchmarkFallbackComponent("input"),
    UITextarea: ui.Textarea || benchmarkFallbackComponent("textarea"),
    UISelect: ui.Select,
    UISelectContent: ui.SelectContent,
    UISelectItem: ui.SelectItem,
    UISelectTrigger: ui.SelectTrigger,
    UISelectValue: ui.SelectValue,
  };
}

type BenchmarkUiAliases = ReturnType<typeof benchmarkResolveUi>;

function benchmarkCanUseSelect(ui: BenchmarkUiAliases): boolean {
  return Boolean(
    ui.UISelect &&
      ui.UISelectContent &&
      ui.UISelectItem &&
      ui.UISelectTrigger &&
      ui.UISelectValue
  );
}

function benchmarkCanUseDialog(ui: BenchmarkUiAliases): boolean {
  return Boolean(
    ui.UIDialog &&
      ui.UIDialogContent &&
      ui.UIDialogDescription &&
      ui.UIDialogHeader &&
      ui.UIDialogTitle
  );
}

async function benchmarkRunAssistantText(
  systemApi: SystemApi,
  options: {
    assistantId: string;
    question: string;
    systemPrompt?: string;
    context?: string;
  }
): Promise<string> {
  if (!systemApi || typeof systemApi.runAssistantText !== "function") {
    throw new Error("当前插件 SDK 不支持 runAssistantText");
  }
  const result = await systemApi.runAssistantText({
    assistantId: options.assistantId,
    prompt: options.question,
    systemPrompt: options.systemPrompt,
    context: options.context,
  });
  return String((result && result.content) || "").trim();
}

async function benchmarkRunModelText(
  systemApi: SystemApi,
  options: {
    modelId: string;
    question: string;
    systemPrompt?: string;
    context?: string;
  }
): Promise<string> {
  if (!systemApi || typeof systemApi.runModelText !== "function") {
    throw new Error("当前插件 SDK 不支持 runModelText");
  }
  const result = await systemApi.runModelText({
    modelId: options.modelId,
    prompt: options.question,
    systemPrompt: options.systemPrompt,
    context: options.context,
  });
  return String((result && result.content) || "").trim();
}
