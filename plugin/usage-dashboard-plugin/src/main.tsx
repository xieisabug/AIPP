type UsageMetric = {
  label: string;
  value: number;
  hint?: string;
};

type UsageBarItem = {
  label: string;
  value: number;
  sublabel?: string;
};

type UsageTrendPoint = {
  day: string;
  messages: number;
  userMessages: number;
  totalTokens: number;
};

type UsageDashboardSnapshot = {
  generatedAt: string;
  metrics: UsageMetric[];
  assistantUsage: UsageBarItem[];
  modelUsage: UsageBarItem[];
  sourceUsage: UsageBarItem[];
  dailyTrend: UsageTrendPoint[];
};

var UsageDashboardPlugin = function () {
  this.systemApi = null;
};

UsageDashboardPlugin.prototype.config = function () {
  return {
    name: "Usage Dashboard Plugin",
    type: ["interfaceType", "applicationType"],
  };
};

UsageDashboardPlugin.prototype.onPluginLoad = async function (systemApi: SystemApi) {
  this.systemApi = systemApi || null;
  if (!this.systemApi) {
    return;
  }
  await ensureUsageDashboardCache(this.systemApi);
};

UsageDashboardPlugin.prototype.renderView = function (viewId: string) {
  if (viewId !== "usage-dashboard") {
    return null;
  }
  var ReactGlobal = (window as any).React;
  if (!ReactGlobal) {
    return null;
  }
  return ReactGlobal.createElement(UsageDashboardPanel, {
    systemApi: this.systemApi,
  });
};

async function ensureUsageDashboardCache(systemApi: SystemApi): Promise<void> {
  await systemApi.storage.execute({
    sql: "CREATE TABLE IF NOT EXISTS dashboard_cache (cache_key TEXT PRIMARY KEY, payload TEXT NOT NULL, updated_at TEXT NOT NULL)",
    params: [],
  });
}

async function readCachedSnapshot(systemApi: SystemApi): Promise<UsageDashboardSnapshot | null> {
  var result = await systemApi.storage.query({
    sql: "SELECT payload FROM dashboard_cache WHERE cache_key = ?",
    params: ["usage-overview-v1"],
    maxRows: 1,
  });
  var payload = result.rows[0] && result.rows[0][0];
  if (typeof payload !== "string" || !payload.trim()) {
    return null;
  }
  try {
    return JSON.parse(payload) as UsageDashboardSnapshot;
  } catch (_error) {
    return null;
  }
}

async function writeCachedSnapshot(systemApi: SystemApi, snapshot: UsageDashboardSnapshot): Promise<void> {
  var payload = JSON.stringify(snapshot);
  await systemApi.storage.execute({
    sql: "INSERT INTO dashboard_cache (cache_key, payload, updated_at) VALUES (?, ?, ?) ON CONFLICT(cache_key) DO UPDATE SET payload = excluded.payload, updated_at = excluded.updated_at",
    params: ["usage-overview-v1", payload, snapshot.generatedAt],
  });
}

function numberValue(value: unknown): number {
  var parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function stringValue(value: unknown, fallback = ""): string {
  return typeof value === "string" && value.trim() ? value : fallback;
}

function formatCount(value: number): string {
  return new Intl.NumberFormat("zh-CN").format(value);
}

function formatDateTime(value?: string): string {
  if (!value) {
    return "暂无";
  }
  var date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function fillDailyTrend(rows: unknown[][]): UsageTrendPoint[] {
  var byDay = new Map<string, UsageTrendPoint>();
  for (var offset = 13; offset >= 0; offset -= 1) {
    var date = new Date();
    date.setHours(0, 0, 0, 0);
    date.setDate(date.getDate() - offset);
    var day = date.toISOString().slice(0, 10);
    byDay.set(day, {
      day: day,
      messages: 0,
      userMessages: 0,
      totalTokens: 0,
    });
  }

  rows.forEach(function (row) {
    var day = stringValue(row[0]);
    if (!byDay.has(day)) {
      return;
    }
    byDay.set(day, {
      day: day,
      messages: numberValue(row[1]),
      userMessages: numberValue(row[2]),
      totalTokens: numberValue(row[3]),
    });
  });

  return Array.from(byDay.values());
}

async function buildUsageDashboardSnapshot(systemApi: SystemApi): Promise<UsageDashboardSnapshot> {
  var results = await Promise.all([
    systemApi.data.query({
      database: "conversation",
      sql: "SELECT COUNT(*) AS total_conversations, COALESCE(SUM(CASE WHEN updated_time >= datetime('now', '-7 day') THEN 1 ELSE 0 END), 0) AS active_conversations_7d, COALESCE(SUM(CASE WHEN conversation_kind LIKE 'butler%' THEN 1 ELSE 0 END), 0) AS butler_conversations FROM conversation",
      maxRows: 1,
    }),
    systemApi.data.query({
      database: "conversation",
      sql: "SELECT COUNT(*) AS total_messages, COALESCE(SUM(CASE WHEN message_type = 'user' THEN 1 ELSE 0 END), 0) AS user_messages, COALESCE(SUM(CASE WHEN message_type = 'assistant' THEN 1 ELSE 0 END), 0) AS assistant_messages, COALESCE(SUM(input_token_count), 0) AS prompt_tokens, COALESCE(SUM(output_token_count), 0) AS completion_tokens, COALESCE(SUM(token_count), 0) AS total_tokens, COALESCE(SUM(CASE WHEN tool_calls_json IS NOT NULL AND tool_calls_json != '' AND tool_calls_json != '[]' THEN 1 ELSE 0 END), 0) AS tool_call_messages FROM message",
      maxRows: 1,
    }),
    systemApi.data.query({
      database: "conversation",
      sql: "SELECT date(created_time) AS day, COUNT(*) AS message_count, COALESCE(SUM(CASE WHEN message_type = 'user' THEN 1 ELSE 0 END), 0) AS user_count, COALESCE(SUM(token_count), 0) AS total_tokens FROM message WHERE created_time >= datetime('now', '-13 day') GROUP BY date(created_time) ORDER BY day ASC",
      maxRows: 32,
    }),
    systemApi.data.query({
      database: "conversation",
      sql: "SELECT assistant_id, COUNT(*) AS conversation_count FROM conversation WHERE assistant_id IS NOT NULL GROUP BY assistant_id ORDER BY conversation_count DESC LIMIT 8",
      maxRows: 8,
    }),
    systemApi.data.query({
      database: "conversation",
      sql: "SELECT c.assistant_id, COUNT(m.id) AS message_count FROM conversation c LEFT JOIN message m ON m.conversation_id = c.id WHERE c.assistant_id IS NOT NULL GROUP BY c.assistant_id ORDER BY message_count DESC LIMIT 8",
      maxRows: 8,
    }),
    systemApi.data.query({
      database: "conversation",
      sql: "SELECT COALESCE(channel_source, 'chat') AS source_name, COUNT(*) AS conversation_count FROM conversation GROUP BY COALESCE(channel_source, 'chat') ORDER BY conversation_count DESC LIMIT 6",
      maxRows: 6,
    }),
    systemApi.data.query({
      database: "conversation",
      sql: "SELECT COALESCE(NULLIF(llm_model_name, ''), '未知模型') AS model_name, llm_model_id, COUNT(*) AS assistant_message_count, COALESCE(SUM(output_token_count), 0) AS completion_tokens FROM message WHERE message_type = 'assistant' GROUP BY COALESCE(NULLIF(llm_model_name, ''), '未知模型'), llm_model_id ORDER BY assistant_message_count DESC LIMIT 8",
      maxRows: 8,
    }),
    systemApi.data.query({
      database: "assistant",
      sql: "SELECT id, name FROM assistant",
      maxRows: 512,
    }),
    systemApi.data.query({
      database: "assistant",
      sql: "SELECT COUNT(*) AS total_assistants FROM assistant",
      maxRows: 1,
    }),
    systemApi.data.query({
      database: "llm",
      sql: "SELECT m.id, m.name, m.code, COALESCE(p.name, '') AS provider_name FROM llm_model m LEFT JOIN llm_provider p ON p.id = m.llm_provider_id",
      maxRows: 512,
    }),
    systemApi.data.query({
      database: "llm",
      sql: "SELECT COUNT(*) AS total_models FROM llm_model",
      maxRows: 1,
    }),
    systemApi.data.query({
      database: "llm",
      sql: "SELECT COUNT(*) AS enabled_provider_count FROM llm_provider WHERE is_enabled = 1",
      maxRows: 1,
    }),
  ]);

  var conversationSummary = results[0].rows[0] || [];
  var messageSummary = results[1].rows[0] || [];
  var dailyTrend = fillDailyTrend(results[2].rows);
  var assistantConversationRows = results[3].rows;
  var assistantMessageRows = results[4].rows;
  var sourceRows = results[5].rows;
  var modelUsageRows = results[6].rows;
  var assistantRows = results[7].rows;
  var assistantCountRow = results[8].rows[0] || [];
  var llmRows = results[9].rows;
  var modelCountRow = results[10].rows[0] || [];
  var enabledProviderRow = results[11].rows[0] || [];

  var assistantNameMap = new Map<number, string>();
  assistantRows.forEach(function (row) {
    assistantNameMap.set(numberValue(row[0]), stringValue(row[1], "未命名助手"));
  });

  var modelNameMap = new Map<number, string>();
  llmRows.forEach(function (row) {
    var modelId = numberValue(row[0]);
    var name = stringValue(row[1]);
    var code = stringValue(row[2]);
    var providerName = stringValue(row[3]);
    var label = name || code || "未知模型";
    if (providerName) {
      label = providerName + " / " + label;
    }
    modelNameMap.set(modelId, label);
  });

  var assistantMessageCountMap = new Map<number, number>();
  assistantMessageRows.forEach(function (row) {
    assistantMessageCountMap.set(numberValue(row[0]), numberValue(row[1]));
  });

  var assistantUsage = assistantConversationRows.map(function (row) {
    var assistantId = numberValue(row[0]);
    var conversationCount = numberValue(row[1]);
    return {
      label: assistantNameMap.get(assistantId) || ("助手 #" + assistantId),
      value: conversationCount,
      sublabel: "消息 " + formatCount(assistantMessageCountMap.get(assistantId) || 0),
    };
  });

  var modelUsage = modelUsageRows.map(function (row) {
    var rawModelName = stringValue(row[0], "未知模型");
    var modelId = numberValue(row[1]);
    var assistantMessageCount = numberValue(row[2]);
    var completionTokens = numberValue(row[3]);
    return {
      label: modelNameMap.get(modelId) || rawModelName,
      value: assistantMessageCount,
      sublabel: "输出 Token " + formatCount(completionTokens),
    };
  });

  var sourceUsage = sourceRows.map(function (row) {
    return {
      label: stringValue(row[0], "chat"),
      value: numberValue(row[1]),
    };
  });

  return {
    generatedAt: new Date().toISOString(),
    metrics: [
      {
        label: "总会话数",
        value: numberValue(conversationSummary[0]),
        hint: "近 7 天活跃 " + formatCount(numberValue(conversationSummary[1])),
      },
      {
        label: "总消息数",
        value: numberValue(messageSummary[0]),
        hint: "用户 " + formatCount(numberValue(messageSummary[1])) + " / 助手 " + formatCount(numberValue(messageSummary[2])),
      },
      {
        label: "总 Token",
        value: numberValue(messageSummary[5]),
        hint: "输入 " + formatCount(numberValue(messageSummary[3])) + " / 输出 " + formatCount(numberValue(messageSummary[4])),
      },
      {
        label: "工具调用回答",
        value: numberValue(messageSummary[6]),
        hint: "Butler 会话 " + formatCount(numberValue(conversationSummary[2])),
      },
      {
        label: "助手数量",
        value: numberValue(assistantCountRow[0]),
        hint: "已配置助手总数",
      },
      {
        label: "模型数量",
        value: numberValue(modelCountRow[0]),
        hint: "启用 Provider " + formatCount(numberValue(enabledProviderRow[0])),
      },
    ],
    assistantUsage: assistantUsage,
    modelUsage: modelUsage,
    sourceUsage: sourceUsage,
    dailyTrend: dailyTrend,
  };
}

function UsageMetricCard(props: { metric: UsageMetric }) {
  return (
    <div className="rounded-lg border border-border/60 bg-background px-4 py-3 shadow-sm">
      <div className="text-xs text-muted-foreground">{props.metric.label}</div>
      <div className="mt-1 text-2xl font-semibold text-foreground">{formatCount(props.metric.value)}</div>
      <div className="mt-1 text-xs text-muted-foreground">{props.metric.hint || " "}</div>
    </div>
  );
}

function UsageBarList(props: { title: string; items: UsageBarItem[]; emptyText: string }) {
  var maxValue = props.items.reduce(function (current, item) {
    return Math.max(current, item.value);
  }, 0);
  return (
    <div className="rounded-lg border border-border/60 bg-background p-4 shadow-sm">
      <div className="text-sm font-medium text-foreground">{props.title}</div>
      {props.items.length === 0 ? (
        <div className="mt-3 text-sm text-muted-foreground">{props.emptyText}</div>
      ) : (
        <div className="mt-4 space-y-3">
          {props.items.map(function (item) {
            var width = maxValue > 0 ? Math.max(8, Math.round((item.value / maxValue) * 100)) : 0;
            return (
              <div key={item.label} className="space-y-1">
                <div className="flex items-center justify-between gap-3 text-sm">
                  <span className="truncate text-foreground">{item.label}</span>
                  <span className="text-muted-foreground">{formatCount(item.value)}</span>
                </div>
                <div className="h-2 rounded-full bg-muted">
                  <div
                    className="h-2 rounded-full bg-foreground/70 transition-all"
                    style={{ width: width + "%" }}
                  />
                </div>
                {item.sublabel ? (
                  <div className="text-xs text-muted-foreground">{item.sublabel}</div>
                ) : null}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function UsageTrendChart(props: { points: UsageTrendPoint[] }) {
  var maxMessages = props.points.reduce(function (current, item) {
    return Math.max(current, item.messages);
  }, 0);
  return (
    <div className="rounded-lg border border-border/60 bg-background p-4 shadow-sm">
      <div className="text-sm font-medium text-foreground">近 14 天消息趋势</div>
      {props.points.length === 0 ? (
        <div className="mt-3 text-sm text-muted-foreground">暂无趋势数据。</div>
      ) : (
        <div className="mt-4 grid grid-cols-7 gap-2 md:grid-cols-14">
          {props.points.map(function (point) {
            var height = maxMessages > 0 ? Math.max(10, Math.round((point.messages / maxMessages) * 96)) : 10;
            return (
              <div key={point.day} className="flex flex-col items-center gap-2">
                <div className="flex h-28 w-full items-end justify-center rounded-md bg-muted/60 px-1">
                  <div
                    className="w-full rounded-sm bg-foreground/70"
                    style={{ height: height + "px" }}
                    title={point.day + "：消息 " + formatCount(point.messages) + "，用户 " + formatCount(point.userMessages)}
                  />
                </div>
                <div className="text-[10px] text-muted-foreground">
                  {point.day.slice(5).replace("-", "/")}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function UsageDashboardPanel(props: { systemApi: SystemApi | null }) {
  var systemApi = props.systemApi;
  var useCallback = React.useCallback;
  var useEffect = React.useEffect;
  var useMemo = React.useMemo;
  var useState = React.useState;

  var _a = useState<UsageDashboardSnapshot | null>(null), snapshot = _a[0], setSnapshot = _a[1];
  var _b = useState<string | null>(null), error = _b[0], setError = _b[1];
  var _c = useState<boolean>(true), loading = _c[0], setLoading = _c[1];
  var _d = useState<boolean>(false), refreshing = _d[0], setRefreshing = _d[1];

  var loadDashboard = useCallback(
    async function (preferCacheOnly?: boolean) {
      if (!systemApi) {
        return;
      }
      setError(null);
      if (!preferCacheOnly) {
        setRefreshing(true);
      }
      try {
        await ensureUsageDashboardCache(systemApi);
        if (!snapshot) {
          var cachedSnapshot = await readCachedSnapshot(systemApi);
          if (cachedSnapshot) {
            setSnapshot(cachedSnapshot);
          }
        }
        if (preferCacheOnly) {
          return;
        }
        var freshSnapshot = await buildUsageDashboardSnapshot(systemApi);
        setSnapshot(freshSnapshot);
        await writeCachedSnapshot(systemApi, freshSnapshot);
      } catch (loadError) {
        setError(loadError instanceof Error ? loadError.message : String(loadError));
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    [systemApi]
  );

  useEffect(
    function () {
      setLoading(true);
      void loadDashboard(false);
    },
    [loadDashboard]
  );

  var metrics = useMemo(function () {
    return snapshot ? snapshot.metrics : [];
  }, [snapshot]);

  if (!systemApi) {
    return <div className="text-sm text-destructive">插件尚未获取到宿主 System API。</div>;
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div>
          <div className="text-sm font-medium text-foreground">本地使用统计</div>
          <div className="text-xs text-muted-foreground">
            最近更新时间：{snapshot ? formatDateTime(snapshot.generatedAt) : "暂无"}
          </div>
        </div>
        <button
          type="button"
          className="inline-flex h-9 items-center justify-center rounded-md border border-border bg-background px-3 text-sm font-medium text-foreground transition-colors hover:bg-muted disabled:cursor-not-allowed disabled:opacity-60"
          onClick={function () {
            void loadDashboard(false);
          }}
          disabled={refreshing}
        >
          {refreshing ? "刷新中..." : "刷新统计"}
        </button>
      </div>

      {error ? (
        <div className="rounded-lg border border-destructive/40 bg-destructive/5 px-4 py-3 text-sm text-destructive">
          读取统计失败：{error}
        </div>
      ) : null}

      {!snapshot && loading ? (
        <div className="rounded-lg border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">
          正在汇总本地数据...
        </div>
      ) : null}

      {snapshot ? (
        <>
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
            {metrics.map(function (metric) {
              return <UsageMetricCard key={metric.label} metric={metric} />;
            })}
          </div>

          <UsageTrendChart points={snapshot.dailyTrend} />

          <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
            <UsageBarList
              title="助手使用分布"
              items={snapshot.assistantUsage}
              emptyText="暂无助手使用数据。"
            />
            <UsageBarList
              title="模型使用分布"
              items={snapshot.modelUsage}
              emptyText="暂无模型使用数据。"
            />
            <UsageBarList
              title="来源分布"
              items={snapshot.sourceUsage}
              emptyText="暂无来源统计。"
            />
          </div>
        </>
      ) : null}
    </div>
  );
}

(window as any)["usage-dashboard-plugin"] = UsageDashboardPlugin;
(window as any).UsageDashboardPlugin = UsageDashboardPlugin;
