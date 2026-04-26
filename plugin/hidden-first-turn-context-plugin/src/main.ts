interface HiddenFirstTurnHookContext {
  conversationId?: unknown;
  assistantId?: unknown;
  messages?: Array<{
    index?: number;
    sourceIndex?: number;
    messageType?: string;
    content?: string;
  }>;
  metadata?: Record<string, unknown>;
}

type HiddenFirstTurnConfig = {
  enabled: boolean;
  hiddenContext: string;
  injectionRole: "system" | "user";
};

var HiddenFirstTurnContextPlugin = function () {
  this.systemApi = null;
  this.boundHook = null;
};

HiddenFirstTurnContextPlugin.prototype.config = function () {
  return {
    name: "Hidden First Turn Context Plugin",
    type: ["applicationType"],
  };
};

HiddenFirstTurnContextPlugin.prototype.onPluginLoad = function (systemApi: SystemApi) {
  this.systemApi = systemApi || null;
  if (!this.systemApi || !this.systemApi.hooks) {
    return;
  }
  if (typeof this.systemApi.hooks.unregister === "function") {
    this.systemApi.hooks.unregister("chat.beforeModelRequest");
  }
  this.boundHook = this.handleBeforeModelRequest.bind(this);
  this.systemApi.hooks.register("chat.beforeModelRequest", this.boundHook);
};

HiddenFirstTurnContextPlugin.prototype.normalizePositiveNumber = function (value: unknown): number | null {
  var parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return null;
  }
  return parsed;
};

HiddenFirstTurnContextPlugin.prototype.normalizeConfig = function (
  rawConfig: Record<string, string | null>
): HiddenFirstTurnConfig {
  var hiddenContext = String(rawConfig.hiddenContext || "").trim();
  var injectionRole: "system" | "user" = rawConfig.injectionRole === "user" ? "user" : "system";
  var enabledValue = String(rawConfig.enabled || "").toLowerCase();
  return {
    enabled: enabledValue === "true" || enabledValue === "1" || enabledValue === "yes" || enabledValue === "on",
    hiddenContext: hiddenContext,
    injectionRole: injectionRole,
  };
};

HiddenFirstTurnContextPlugin.prototype.isFirstUserTurn = async function (conversationId: number): Promise<boolean> {
  if (!this.systemApi) {
    return false;
  }
  var result = await this.systemApi.data.query({
    database: "conversation",
    sql: "SELECT COUNT(*) AS total FROM message WHERE conversation_id = ? AND message_type = 'user'",
    params: [conversationId],
    maxRows: 1,
  });
  var total = Number(result.rows[0] && result.rows[0][0]);
  return Number.isFinite(total) && total === 1;
};

HiddenFirstTurnContextPlugin.prototype.buildInjectedContent = function (hiddenContext: string): string {
  return [
    "<plugin_hidden_context>",
    hiddenContext.trim(),
    "</plugin_hidden_context>",
  ].join("\n");
};

HiddenFirstTurnContextPlugin.prototype.cloneMessages = function (
  messages: HiddenFirstTurnHookContext["messages"]
) {
  return Array.isArray(messages)
    ? messages.map(function (message) {
        return {
          index: typeof message.index === "number" ? message.index : undefined,
          sourceIndex: typeof message.sourceIndex === "number" ? message.sourceIndex : undefined,
          messageType: typeof message.messageType === "string" ? message.messageType : undefined,
          content: typeof message.content === "string" ? message.content : undefined,
        };
      })
    : [];
};

HiddenFirstTurnContextPlugin.prototype.handleBeforeModelRequest = async function (
  rawContext: unknown
): Promise<AippSystemApiHookResult | void> {
  if (!this.systemApi) {
    return;
  }
  var context = (rawContext || {}) as HiddenFirstTurnHookContext;
  var assistantId = this.normalizePositiveNumber(context.assistantId);
  var conversationId = this.normalizePositiveNumber(context.conversationId);
  if (!assistantId || !conversationId) {
    return;
  }

  var config = this.normalizeConfig(await this.systemApi.assistantConfig.getAll(assistantId));
  if (!config.enabled || !config.hiddenContext) {
    return;
  }

  var isFirstTurn = await this.isFirstUserTurn(conversationId);
  if (!isFirstTurn) {
    return;
  }

  var messages = this.cloneMessages(context.messages);
  var firstUserIndex = messages.findIndex(function (message) {
    return String(message && message.messageType || "").toLowerCase() === "user";
  });
  if (firstUserIndex < 0) {
    return;
  }

  var injectedMessage = {
    messageType: config.injectionRole,
    content: this.buildInjectedContent(config.hiddenContext),
  };
  messages.splice(firstUserIndex + 1, 0, injectedMessage);

  return {
    action: "replace",
    context: {
      conversationId: conversationId,
      assistantId: assistantId,
      messageCount: messages.length,
      messages: messages,
      metadata: {
        ...(context.metadata || {}),
        hiddenFirstTurnContextInjected: true,
        hiddenFirstTurnContextRole: config.injectionRole,
      },
    },
  };
};

(window as any)["hidden-first-turn-context-plugin"] = HiddenFirstTurnContextPlugin;
(window as any).HiddenFirstTurnContextPlugin = HiddenFirstTurnContextPlugin;
