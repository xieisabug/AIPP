var ReactRuntime = (window as any).React as typeof import("react");

type PromptOptimizerActionContext = {
  conversationId?: number | null;
  assistantId?: number | null;
  conversationName?: string;
  assistantName?: string;
};

type PromptOptimizerModelOption = {
  value: string;
  label: string;
};

type PromptOptimizerAnalysis = {
  currentPrompt: string;
  promptId?: number;
  shouldUpdate: boolean;
  summary: string;
  reasons: string[];
  proposedPrompt: string;
  rawResponse: string;
};

type PromptOptimizerDiffRow = {
  type: "same" | "add" | "remove";
  text: string;
};

type PromptOptimizerUiAliases = {
  UIAlert: import("react").ComponentType<any>;
  UIAlertDescription: import("react").ComponentType<any>;
  UIButton: import("react").ComponentType<any>;
  UIIconButton?: import("react").ComponentType<any>;
  UICard: import("react").ComponentType<any>;
  UICardContent: import("react").ComponentType<any>;
  UIDialog?: import("react").ComponentType<any>;
  UIDialogContent?: import("react").ComponentType<any>;
  UIDialogDescription?: import("react").ComponentType<any>;
  UIDialogHeader?: import("react").ComponentType<any>;
  UIDialogTitle?: import("react").ComponentType<any>;
  UISelect?: import("react").ComponentType<any>;
  UISelectContent?: import("react").ComponentType<any>;
  UISelectItem?: import("react").ComponentType<any>;
  UISelectTrigger?: import("react").ComponentType<any>;
  UISelectValue?: import("react").ComponentType<any>;
};

var PROMPT_OPTIMIZER_MODEL_KEY = "prompt_optimizer_model_id";

var PROMPT_OPTIMIZER_SYSTEM_PROMPT = [
  "你是 AIPP 的提示词优化评审器。",
  "你会收到当前助手的系统提示词，以及最近一段真实对话。",
  "你的任务是判断当前系统提示词是否需要改进，以更稳定地满足用户需求。",
  "不要无端扩展助手职责，不要引入与当前对话无关的新能力。",
  "如果不需要修改，请保持 proposedPrompt 与 currentPrompt 一致。",
  "必须只返回 JSON，不要输出 markdown 代码块。",
  "返回结构：",
  '{',
  '  "shouldUpdate": boolean,',
  '  "summary": string,',
  '  "reasons": string[],',
  '  "proposedPrompt": string',
  '}'
].join("\n");

function promptOptimizerFallbackComponent(tag: string): import("react").ComponentType<any> {
  return function FallbackComponent(props: any) {
    if (!ReactRuntime || typeof ReactRuntime.createElement !== "function") {
      return null;
    }
    return ReactRuntime.createElement(tag, props, props?.children);
  };
}

function promptOptimizerResolveUi(systemApi: SystemApi | null): PromptOptimizerUiAliases {
  var ui = (systemApi && systemApi.ui) || {};
  return {
    UIAlert: ui.Alert || promptOptimizerFallbackComponent("div"),
    UIAlertDescription: ui.AlertDescription || promptOptimizerFallbackComponent("div"),
    UIButton: ui.Button || promptOptimizerFallbackComponent("button"),
    UIIconButton: ui.IconButton,
    UICard: ui.Card || promptOptimizerFallbackComponent("div"),
    UICardContent: ui.CardContent || promptOptimizerFallbackComponent("div"),
    UIDialog: ui.Dialog,
    UIDialogContent: ui.DialogContent,
    UIDialogDescription: ui.DialogDescription,
    UIDialogHeader: ui.DialogHeader,
    UIDialogTitle: ui.DialogTitle,
    UISelect: ui.Select,
    UISelectContent: ui.SelectContent,
    UISelectItem: ui.SelectItem,
    UISelectTrigger: ui.SelectTrigger,
    UISelectValue: ui.SelectValue,
  };
}

function promptOptimizerCanUseDialog(ui: PromptOptimizerUiAliases): boolean {
  return Boolean(
    ui.UIDialog &&
      ui.UIDialogContent &&
      ui.UIDialogDescription &&
      ui.UIDialogHeader &&
      ui.UIDialogTitle
  );
}

function promptOptimizerCanUseSelect(ui: PromptOptimizerUiAliases): boolean {
  return Boolean(
    ui.UISelect &&
      ui.UISelectContent &&
      ui.UISelectItem &&
      ui.UISelectTrigger &&
      ui.UISelectValue
  );
}

function PromptOptimizerSparklesIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="text-icon"
      aria-hidden="true"
    >
      <path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .962 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.582a.5.5 0 0 1 0 .962L15.5 14.064a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.962 0z" />
      <path d="M20 3v4" />
      <path d="M22 5h-4" />
      <path d="M4 17v2" />
      <path d="M5 18H3" />
    </svg>
  );
}

function PromptOptimizerTitleActionButton(props: {
  ui: PromptOptimizerUiAliases;
  disabled?: boolean;
  title: string;
  onClick: () => void;
}) {
  var UIIconButton = props.ui.UIIconButton;
  var icon = <PromptOptimizerSparklesIcon />;

  if (UIIconButton) {
    return (
      <UIIconButton
        icon={icon}
        onClick={props.onClick}
        disabled={props.disabled}
        title={props.title}
        border
        dataAippSlot="chat-conversation-title-prompt-optimizer"
      />
    );
  }

  var UIButton = props.ui.UIButton;
  return (
    <UIButton
      variant="outline"
      size="icon"
      onClick={props.onClick}
      disabled={props.disabled}
      title={props.title}
      data-aipp-slot="chat-conversation-title-prompt-optimizer"
    >
      {icon}
    </UIButton>
  );
}

function promptOptimizerString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function promptOptimizerNormalizeContext(
  context?: Record<string, unknown>
): PromptOptimizerActionContext {
  var rawConversationId = Number(context && context.conversationId);
  var rawAssistantId = Number(context && context.assistantId);
  return {
    conversationId: Number.isFinite(rawConversationId) && rawConversationId > 0 ? rawConversationId : null,
    assistantId: Number.isFinite(rawAssistantId) && rawAssistantId > 0 ? rawAssistantId : null,
    conversationName: promptOptimizerString(context && context.conversationName),
    assistantName: promptOptimizerString(context && context.assistantName),
  };
}

function promptOptimizerModelLabel(model: AippSystemApiModelItem): string {
  var name = promptOptimizerString(model.name, model.code);
  if (model.code && model.code !== name) {
    return name + " (" + model.code + ")";
  }
  return name;
}

function promptOptimizerDefaultModelId(
  assistantDetail: AippSystemApiAssistantDetail | null,
  models: AippSystemApiModelItem[],
  configuredModelId: string | null
): string {
  if (configuredModelId && configuredModelId.trim()) {
    return configuredModelId;
  }
  if (assistantDetail && assistantDetail.model && assistantDetail.model[0]) {
    return (
      promptOptimizerString(assistantDetail.model[0].model_code) +
      "%%" +
      String(assistantDetail.model[0].provider_id)
    );
  }
  if (models[0]) {
    return promptOptimizerString(models[0].code) + "%%" + String(models[0].llm_provider_id);
  }
  return "";
}

function promptOptimizerTrimContent(content: string, maxLength: number): string {
  var normalized = promptOptimizerString(content).trim();
  if (normalized.length <= maxLength) {
    return normalized;
  }
  return normalized.slice(0, maxLength) + "\n...[已截断]";
}

function promptOptimizerBuildTranscript(messages: AippSystemApiMessage[]): string {
  var relevantMessages = (messages || [])
    .filter(function (message) {
      return (
        message &&
        (message.message_type === "user" ||
          message.message_type === "response" ||
          message.message_type === "assistant")
      );
    })
    .slice(-16);

  var totalLength = 0;
  var rows: string[] = [];
  for (var i = 0; i < relevantMessages.length; i += 1) {
    var message = relevantMessages[i];
    var role = message.message_type === "user" ? "用户" : "助手";
    var content = promptOptimizerTrimContent(promptOptimizerString(message.content), 2400);
    var row = "[" + role + "]\n" + content;
    totalLength += row.length;
    if (totalLength > 18000) {
      break;
    }
    rows.push(row);
  }
  return rows.join("\n\n");
}

function promptOptimizerBuildUserPrompt(input: {
  conversationName: string;
  assistantName: string;
  currentPrompt: string;
  transcript: string;
}): string {
  return [
    "请基于以下信息评估是否需要优化当前助手的系统提示词。",
    "",
    "对话名称：",
    input.conversationName || "未命名对话",
    "",
    "助手名称：",
    input.assistantName || "未知助手",
    "",
    "当前系统提示词：",
    input.currentPrompt || "(空)",
    "",
    "最近对话：",
    input.transcript || "(没有可用对话内容)",
    "",
    "请给出是否应更新提示词、原因摘要、原因列表，以及完整的 proposedPrompt。"
  ].join("\n");
}

function promptOptimizerExtractJson(rawText: string): any {
  var text = promptOptimizerString(rawText).trim();
  if (!text) {
    throw new Error("模型没有返回内容");
  }
  try {
    return JSON.parse(text);
  } catch (_error) {
    // noop
  }

  var fencedMatch = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fencedMatch && fencedMatch[1]) {
    try {
      return JSON.parse(fencedMatch[1].trim());
    } catch (_error2) {
      // noop
    }
  }

  var firstBrace = text.indexOf("{");
  var lastBrace = text.lastIndexOf("}");
  if (firstBrace >= 0 && lastBrace > firstBrace) {
    return JSON.parse(text.slice(firstBrace, lastBrace + 1));
  }

  throw new Error("无法从模型返回中解析 JSON");
}

function promptOptimizerNormalizeReasons(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value
    .map(function (item) {
      return promptOptimizerString(item).trim();
    })
    .filter(Boolean)
    .slice(0, 6);
}

function promptOptimizerNormalizeAnalysis(
  rawText: string,
  currentPrompt: string,
  promptId?: number
): PromptOptimizerAnalysis {
  var parsed = promptOptimizerExtractJson(rawText) || {};
  var proposedPrompt = promptOptimizerString(parsed.proposedPrompt, currentPrompt).trim();
  var shouldUpdate = parsed.shouldUpdate === true;
  if (!proposedPrompt) {
    proposedPrompt = currentPrompt;
    shouldUpdate = false;
  }
  if (proposedPrompt === currentPrompt) {
    shouldUpdate = false;
  }
  return {
    currentPrompt: currentPrompt,
    promptId: promptId,
    shouldUpdate: shouldUpdate,
    summary: promptOptimizerString(
      parsed.summary,
      shouldUpdate ? "建议调整当前提示词。" : "当前提示词暂时不需要修改。"
    ),
    reasons: promptOptimizerNormalizeReasons(parsed.reasons),
    proposedPrompt: proposedPrompt,
    rawResponse: rawText,
  };
}

function promptOptimizerBuildDiffRows(
  currentPrompt: string,
  proposedPrompt: string
): PromptOptimizerDiffRow[] {
  var oldLines = promptOptimizerString(currentPrompt).split(/\r?\n/);
  var newLines = promptOptimizerString(proposedPrompt).split(/\r?\n/);
  var rows: PromptOptimizerDiffRow[] = [];
  var lcs: number[][] = [];
  var i = 0;
  var j = 0;

  for (i = 0; i <= oldLines.length; i += 1) {
    lcs[i] = [];
    for (j = 0; j <= newLines.length; j += 1) {
      lcs[i][j] = 0;
    }
  }

  for (i = oldLines.length - 1; i >= 0; i -= 1) {
    for (j = newLines.length - 1; j >= 0; j -= 1) {
      if (oldLines[i] === newLines[j]) {
        lcs[i][j] = lcs[i + 1][j + 1] + 1;
      } else {
        lcs[i][j] = Math.max(lcs[i + 1][j], lcs[i][j + 1]);
      }
    }
  }

  i = 0;
  j = 0;
  while (i < oldLines.length && j < newLines.length) {
    if (oldLines[i] === newLines[j]) {
      rows.push({ type: "same", text: oldLines[i] });
      i += 1;
      j += 1;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      rows.push({ type: "remove", text: oldLines[i] });
      i += 1;
    } else {
      rows.push({ type: "add", text: newLines[j] });
      j += 1;
    }
  }

  while (i < oldLines.length) {
    rows.push({ type: "remove", text: oldLines[i] });
    i += 1;
  }

  while (j < newLines.length) {
    rows.push({ type: "add", text: newLines[j] });
    j += 1;
  }

  if (rows.length === 0) {
    rows.push({ type: "same", text: "" });
  }

  return rows;
}

function PromptOptimizerSelectField(props: {
  ui: PromptOptimizerUiAliases;
  value: string;
  options: PromptOptimizerModelOption[];
  placeholder: string;
  disabled?: boolean;
  onValueChange: (value: string) => void;
}) {
  var ui = props.ui;
  if (promptOptimizerCanUseSelect(ui)) {
    var UISelect = ui.UISelect as import("react").ComponentType<any>;
    var UISelectTrigger = ui.UISelectTrigger as import("react").ComponentType<any>;
    var UISelectValue = ui.UISelectValue as import("react").ComponentType<any>;
    var UISelectContent = ui.UISelectContent as import("react").ComponentType<any>;
    var UISelectItem = ui.UISelectItem as import("react").ComponentType<any>;
    var currentValue = props.value || "__empty__";
    return (
      <UISelect
        value={currentValue}
        onValueChange={(nextValue: string) =>
          props.onValueChange(nextValue === "__empty__" ? "" : nextValue)
        }
        disabled={props.disabled}
      >
        <UISelectTrigger className="w-full">
          <UISelectValue placeholder={props.placeholder} />
        </UISelectTrigger>
        <UISelectContent>
          <UISelectItem value="__empty__">{props.placeholder}</UISelectItem>
          {props.options.map(function (option) {
            return (
              <UISelectItem key={option.value} value={option.value}>
                {option.label}
              </UISelectItem>
            );
          })}
        </UISelectContent>
      </UISelect>
    );
  }

  return (
    <select
      disabled={props.disabled}
      value={props.value || ""}
      onChange={function (event) {
        props.onValueChange((event.target as HTMLSelectElement).value);
      }}
      style={{ width: "100%", minHeight: 36 }}
    >
      <option value="">{props.placeholder}</option>
      {props.options.map(function (option) {
        return (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        );
      })}
    </select>
  );
}

function PromptDiffView(props: {
  currentPrompt: string;
  proposedPrompt: string;
}) {
  var rows = promptOptimizerBuildDiffRows(props.currentPrompt, props.proposedPrompt);
  return (
    <div
      className="max-h-[320px] overflow-auto rounded-md border bg-muted/20"
      style={{ fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace" }}
    >
      {rows.map(function (row, index) {
        var prefix = " ";
        var className = "px-3 py-1 text-xs whitespace-pre-wrap break-words border-b border-border/40";
        if (row.type === "add") {
          prefix = "+";
          className += " bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
        } else if (row.type === "remove") {
          prefix = "-";
          className += " bg-red-500/10 text-red-700 dark:text-red-300";
        } else {
          className += " text-muted-foreground";
        }
        return (
          <div key={String(index)} className={className}>
            {prefix} {row.text || " "}
          </div>
        );
      })}
    </div>
  );
}

function PromptOptimizerAction(props: {
  systemApi: SystemApi | null;
  context?: Record<string, unknown>;
}) {
  var context = promptOptimizerNormalizeContext(props.context);
  var systemApi = props.systemApi;
  var ui = promptOptimizerResolveUi(systemApi);
  var UIAlert = ui.UIAlert;
  var UIAlertDescription = ui.UIAlertDescription;
  var UIButton = ui.UIButton;
  var UICard = ui.UICard;
  var UICardContent = ui.UICardContent;
  var UIDialog = ui.UIDialog as import("react").ComponentType<any> | undefined;
  var UIDialogContent = ui.UIDialogContent as import("react").ComponentType<any> | undefined;
  var UIDialogDescription = ui.UIDialogDescription as import("react").ComponentType<any> | undefined;
  var UIDialogHeader = ui.UIDialogHeader as import("react").ComponentType<any> | undefined;
  var UIDialogTitle = ui.UIDialogTitle as import("react").ComponentType<any> | undefined;
  var hooks = ReactRuntime;
  var useState = hooks.useState;
  var useEffect = hooks.useEffect;
  var useMemo = hooks.useMemo;
  var useCallback = hooks.useCallback;
  var useRef = hooks.useRef;

  var [open, setOpen] = useState(false);
  var [models, setModels] = useState<AippSystemApiModelItem[]>([]);
  var [assistantDetail, setAssistantDetail] = useState<AippSystemApiAssistantDetail | null>(null);
  var [selectedModelId, setSelectedModelId] = useState("");
  var [analysis, setAnalysis] = useState<PromptOptimizerAnalysis | null>(null);
  var [loading, setLoading] = useState(false);
  var [analyzing, setAnalyzing] = useState(false);
  var [applying, setApplying] = useState(false);
  var [errorText, setErrorText] = useState("");
  var [successText, setSuccessText] = useState("");
  var initializedKeyRef = useRef("");

  var modelOptions = useMemo(function () {
    return (models || []).map(function (model) {
      return {
        value: promptOptimizerString(model.code) + "%%" + String(model.llm_provider_id),
        label: promptOptimizerModelLabel(model),
      };
    });
  }, [models]);

  var loadDialogData = useCallback(async function () {
    if (!systemApi || !context.assistantId) {
      return;
    }
    var loadKey = String(context.assistantId) + ":" + String(context.conversationId || "");
    if (initializedKeyRef.current === loadKey && assistantDetail && models.length > 0) {
      return;
    }

    setLoading(true);
    setErrorText("");
    try {
      var result = await Promise.all([
        systemApi.listModels(),
        systemApi.assistants.getDetail(context.assistantId),
        systemApi.assistantConfig.get(context.assistantId, PROMPT_OPTIMIZER_MODEL_KEY),
      ]);
      var nextModels = result[0] || [];
      var nextAssistantDetail = result[1] || null;
      var configuredModelId = result[2];
      var defaultModelId = promptOptimizerDefaultModelId(
        nextAssistantDetail,
        nextModels,
        configuredModelId
      );
      setModels(nextModels);
      setAssistantDetail(nextAssistantDetail);
      setSelectedModelId(defaultModelId);
      initializedKeyRef.current = loadKey;
    } catch (error) {
      setErrorText("加载优化器配置失败：" + String(error));
    } finally {
      setLoading(false);
    }
  }, [assistantDetail, context.assistantId, context.conversationId, models.length, systemApi]);

  useEffect(function () {
    if (!open) {
      return;
    }
    void loadDialogData();
  }, [loadDialogData, open]);

  var handleModelChange = useCallback(
    function (nextValue: string) {
      setSelectedModelId(nextValue);
      if (systemApi && context.assistantId) {
        void systemApi.assistantConfig.set(
          context.assistantId,
          PROMPT_OPTIMIZER_MODEL_KEY,
          nextValue || null
        );
      }
    },
    [context.assistantId, systemApi]
  );

  var handleAnalyze = useCallback(async function () {
    if (!systemApi) {
      setErrorText("当前插件运行环境不可用。");
      return;
    }
    if (!context.assistantId || !context.conversationId) {
      setErrorText("当前对话没有可用的助手信息。");
      return;
    }
    if (!selectedModelId) {
      setErrorText("请先选择一个评估模型。");
      return;
    }

    setAnalyzing(true);
    setErrorText("");
    setSuccessText("");
    try {
      var currentAssistantDetail =
        assistantDetail || (await systemApi.assistants.getDetail(context.assistantId));
      var conversationData = await systemApi.conversations.getWithMessages(context.conversationId);
      var currentPrompt = promptOptimizerString(
        currentAssistantDetail &&
          currentAssistantDetail.prompts &&
          currentAssistantDetail.prompts[0] &&
          currentAssistantDetail.prompts[0].prompt
      ).trim();
      var promptId =
        currentAssistantDetail &&
        currentAssistantDetail.prompts &&
        currentAssistantDetail.prompts[0]
          ? currentAssistantDetail.prompts[0].id
          : undefined;

      var transcript = promptOptimizerBuildTranscript(conversationData.messages || []);
      var rawResponse = await systemApi.runModelText({
        modelId: selectedModelId,
        systemPrompt: PROMPT_OPTIMIZER_SYSTEM_PROMPT,
        prompt: promptOptimizerBuildUserPrompt({
          conversationName: context.conversationName || "",
          assistantName: context.assistantName || "",
          currentPrompt: currentPrompt,
          transcript: transcript,
        }),
      });

      setAssistantDetail(currentAssistantDetail);
      setAnalysis(promptOptimizerNormalizeAnalysis(rawResponse.content, currentPrompt, promptId));
      await systemApi.assistantConfig.set(
        context.assistantId,
        PROMPT_OPTIMIZER_MODEL_KEY,
        selectedModelId
      );
    } catch (error) {
      setErrorText("分析失败：" + String(error));
    } finally {
      setAnalyzing(false);
    }
  }, [
    assistantDetail,
    context.assistantId,
    context.assistantName,
    context.conversationId,
    context.conversationName,
    selectedModelId,
    systemApi,
  ]);

  var handleAccept = useCallback(async function () {
    if (!systemApi || !context.assistantId || !analysis) {
      return;
    }
    setApplying(true);
    setErrorText("");
    setSuccessText("");
    try {
      var updatedPrompt = await systemApi.assistants.updatePrompt({
        assistantId: context.assistantId,
        prompt: analysis.proposedPrompt,
        expectedPromptId: analysis.promptId,
        expectedOldPrompt: analysis.currentPrompt,
      });
      setAnalysis({
        currentPrompt: updatedPrompt.prompt,
        promptId: updatedPrompt.id,
        shouldUpdate: false,
        summary: "已将建议提示词写回当前助手。",
        reasons: analysis.reasons,
        proposedPrompt: updatedPrompt.prompt,
        rawResponse: analysis.rawResponse,
      });
      if (assistantDetail) {
        var nextPrompt: AippSystemApiAssistantPrompt = {
          id: updatedPrompt.id,
          assistant_id: updatedPrompt.assistant_id,
          prompt: updatedPrompt.prompt,
          created_time: updatedPrompt.created_time,
        };
        setAssistantDetail({
          ...assistantDetail,
          prompts: [nextPrompt].concat((assistantDetail.prompts || []).slice(1)),
        });
      }
      setSuccessText("提示词已更新，后续新的对话轮次会使用新提示词。");
      setOpen(false);
      systemApi.toast?.success("提示词修改成功");
    } catch (error) {
      setErrorText("写回提示词失败：" + String(error));
    } finally {
      setApplying(false);
    }
  }, [analysis, assistantDetail, context.assistantId, systemApi]);

  if (!context.assistantId || !context.conversationId) {
    return null;
  }

  if (!promptOptimizerCanUseDialog(ui) || !UIDialog || !UIDialogContent || !UIDialogHeader || !UIDialogTitle || !UIDialogDescription) {
    return (
      <PromptOptimizerTitleActionButton
        ui={ui}
        disabled
        title="当前插件运行环境不支持 Dialog"
        onClick={function () {}}
      />
    );
  }

  return (
    <>
      <PromptOptimizerTitleActionButton
        ui={ui}
        title="优化提示词"
        onClick={function () {
          setOpen(true);
        }}
      />
      <UIDialog open={open} onOpenChange={setOpen}>
        <UIDialogContent className="sm:max-w-3xl">
          <UIDialogHeader>
            <UIDialogTitle>优化助手提示词</UIDialogTitle>
            <UIDialogDescription>
              选择一个模型评估当前对话，并生成可确认的提示词修改建议。
            </UIDialogDescription>
          </UIDialogHeader>
          <div className="space-y-4">
            <div className="grid gap-3 md:grid-cols-[1fr_auto]">
              <div className="space-y-2">
                <div className="text-sm font-medium">评估模型</div>
                <PromptOptimizerSelectField
                  ui={ui}
                  value={selectedModelId}
                  options={modelOptions}
                  placeholder="选择模型"
                  disabled={loading || analyzing || applying}
                  onValueChange={handleModelChange}
                />
              </div>
              <div className="flex items-end">
                <UIButton
                  onClick={function () {
                    void handleAnalyze();
                  }}
                  disabled={loading || analyzing || applying || !selectedModelId}
                >
                  {analyzing ? "分析中..." : "开始分析"}
                </UIButton>
              </div>
            </div>

            {loading ? (
              <UICard className="shadow-none">
                <UICardContent className="py-4 text-sm text-muted-foreground">
                  正在加载模型和助手信息...
                </UICardContent>
              </UICard>
            ) : null}

            {errorText ? (
              <UIAlert variant="destructive">
                <UIAlertDescription>{errorText}</UIAlertDescription>
              </UIAlert>
            ) : null}

            {successText ? (
              <UIAlert>
                <UIAlertDescription>{successText}</UIAlertDescription>
              </UIAlert>
            ) : null}

            {analysis ? (
              <div className="space-y-4">
                <UICard className="shadow-none">
                  <UICardContent className="space-y-3 py-4">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-sm font-medium">评估结论</span>
                      <span
                        className={
                          "inline-flex items-center rounded-full border px-2 py-0.5 text-xs " +
                          (analysis.shouldUpdate
                            ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
                            : "border-border bg-muted text-muted-foreground")
                        }
                      >
                        {analysis.shouldUpdate ? "建议修改" : "建议保持不变"}
                      </span>
                    </div>
                    <div className="text-sm leading-6">{analysis.summary}</div>
                    {analysis.reasons.length > 0 ? (
                      <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
                        {analysis.reasons.map(function (reason, index) {
                          return <li key={String(index)}>{reason}</li>;
                        })}
                      </ul>
                    ) : null}
                  </UICardContent>
                </UICard>

                <div className="space-y-2">
                  <div className="text-sm font-medium">提示词 diff</div>
                  <PromptDiffView
                    currentPrompt={analysis.currentPrompt}
                    proposedPrompt={analysis.proposedPrompt}
                  />
                </div>

                <div className="grid gap-4 md:grid-cols-2">
                  <div className="space-y-2">
                    <div className="text-sm font-medium">当前提示词</div>
                    <textarea
                      readOnly
                      value={analysis.currentPrompt}
                      className="min-h-[180px] w-full rounded-md border bg-muted/20 p-3 text-xs"
                    />
                  </div>
                  <div className="space-y-2">
                    <div className="text-sm font-medium">建议提示词</div>
                    <textarea
                      readOnly
                      value={analysis.proposedPrompt}
                      className="min-h-[180px] w-full rounded-md border bg-muted/20 p-3 text-xs"
                    />
                  </div>
                </div>

                <div className="flex justify-end gap-2">
                  <UIButton
                    variant="outline"
                    onClick={function () {
                      setOpen(false);
                    }}
                    disabled={applying}
                  >
                    不接受
                  </UIButton>
                  <UIButton
                    onClick={function () {
                      void handleAccept();
                    }}
                    disabled={!analysis.shouldUpdate || applying}
                  >
                    {applying ? "写入中..." : "接受并更新"}
                  </UIButton>
                </div>
              </div>
            ) : null}
          </div>
        </UIDialogContent>
      </UIDialog>
    </>
  );
}

var PromptOptimizerPlugin = function () {
  this.systemApi = null;
};

PromptOptimizerPlugin.prototype.config = function () {
  return {
    name: "Prompt Optimizer Plugin",
    type: ["interfaceType"],
  };
};

PromptOptimizerPlugin.prototype.onPluginLoad = function (systemApi: SystemApi) {
  this.systemApi = systemApi || null;
};

PromptOptimizerPlugin.prototype.renderAction = function (
  actionId: string,
  context?: Record<string, unknown>
) {
  if (actionId !== "prompt-optimizer") {
    return null;
  }
  if (!ReactRuntime || !this.systemApi) {
    return null;
  }
  return ReactRuntime.createElement(PromptOptimizerAction, {
    systemApi: this.systemApi,
    context: context,
  });
};

(window as any)["prompt-optimizer-plugin"] = PromptOptimizerPlugin;
(window as any).PromptOptimizerPlugin = PromptOptimizerPlugin;
