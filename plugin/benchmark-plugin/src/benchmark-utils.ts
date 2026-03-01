var BENCHMARK_STORAGE_SESSION = "benchmark.ui";
var BENCHMARK_STORAGE_KEY = "benchmark_state_v1";
var BENCHMARK_DEFAULT_JUDGE_PROMPT =
  "你是严格评测员。请根据问题、标准答案和候选答案进行评分。评分范围 0-100，必须输出 JSON：{\"score\": 整数, \"reason\": \"简短中文理由\"}。";
var BENCHMARK_MAX_RUN_RECORDS = 100;

function benchmarkNowIso(): string {
  return new Date().toISOString();
}

function benchmarkUid(prefix: string): string {
  return `${prefix}_${Date.now()}_${Math.floor(Math.random() * 100000)}`;
}

function benchmarkSafeParse<T>(raw: string | null, fallbackValue: T): T {
  if (typeof raw !== "string" || raw.trim() === "") {
    return fallbackValue;
  }
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallbackValue;
  }
}

function benchmarkClamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function benchmarkEnsureSetShape(setValue: Partial<BenchmarkSet> | null | undefined): BenchmarkSet {
  const set = setValue && typeof setValue === "object" ? setValue : {};
  const createdAt = typeof set.createdAt === "string" ? set.createdAt : benchmarkNowIso();
  const updatedAt = typeof set.updatedAt === "string" ? set.updatedAt : createdAt;
  return {
    id: String(set.id || benchmarkUid("set")),
    name: String(set.name || "未命名 QA 集"),
    description: String(set.description || ""),
    judgePrompt: String(set.judgePrompt || BENCHMARK_DEFAULT_JUDGE_PROMPT),
    runnerType: set.runnerType === "model" ? "model" : "assistant",
    runnerAssistantId: String(set.runnerAssistantId || ""),
    runnerModelId: String(set.runnerModelId || ""),
    judgeType: set.judgeType === "model" ? "model" : "assistant",
    judgeAssistantId: String(set.judgeAssistantId || ""),
    judgeModelId: String(set.judgeModelId || ""),
    items: Array.isArray(set.items)
      ? set.items
          .filter((item) => item && typeof item === "object")
          .map((item) => ({
            id: String(item.id || benchmarkUid("qa")),
            question: String(item.question || ""),
            reference: String(item.reference || ""),
            updatedAt: typeof item.updatedAt === "string" ? item.updatedAt : benchmarkNowIso(),
          }))
      : [],
    createdAt,
    updatedAt,
  };
}

function benchmarkEnsureRunItemShape(
  itemValue: Partial<BenchmarkRunItem> | null | undefined
): BenchmarkRunItem {
  const item = itemValue && typeof itemValue === "object" ? itemValue : {};
  return {
    itemId: String(item.itemId || benchmarkUid("qa")),
    question: String(item.question || ""),
    reference: String(item.reference || ""),
    answer: String(item.answer || ""),
    judgeQuestion: String(item.judgeQuestion || ""),
    score: benchmarkClamp(Math.round(Number(item.score || 0)), 0, 100),
    reason: String(item.reason || ""),
    judgeRaw: String(item.judgeRaw || ""),
  };
}

function benchmarkEnsureRunRecordShape(
  runValue: Partial<BenchmarkRunRecord> | null | undefined
): BenchmarkRunRecord {
  const run = runValue && typeof runValue === "object" ? runValue : {};
  return {
    id: String(run.id || benchmarkUid("run")),
    setId: String(run.setId || ""),
    setName: String(run.setName || ""),
    createdAt: typeof run.createdAt === "string" ? run.createdAt : benchmarkNowIso(),
    status: run.status === "failed" ? "failed" : "success",
    totalScore: Number(run.totalScore || 0),
    avgScore: Number(run.avgScore || 0),
    runnerType: run.runnerType === "model" ? "model" : run.runnerType === "assistant" ? "assistant" : undefined,
    runnerTargetId: run.runnerTargetId ? String(run.runnerTargetId) : undefined,
    runnerLabel: String(run.runnerLabel || ""),
    judgeType: run.judgeType === "model" ? "model" : run.judgeType === "assistant" ? "assistant" : undefined,
    judgeTargetId: run.judgeTargetId ? String(run.judgeTargetId) : undefined,
    judgeLabel: String(run.judgeLabel || ""),
    judgeSystemPrompt: run.judgeSystemPrompt ? String(run.judgeSystemPrompt) : undefined,
    items: Array.isArray(run.items)
      ? run.items
          .filter((item) => item && typeof item === "object")
          .map((item) => benchmarkEnsureRunItemShape(item))
      : [],
    error: run.error ? String(run.error) : undefined,
  };
}

function benchmarkEnsureStateShape(
  value: Partial<BenchmarkState> | null | undefined
): BenchmarkState {
  const state = value && typeof value === "object" ? value : {};
  const sets = Array.isArray(state.sets)
    ? state.sets.map((setItem) => benchmarkEnsureSetShape(setItem))
    : [];
  const runs = Array.isArray(state.runs)
    ? state.runs
        .filter((run) => run && typeof run === "object")
        .map((run) => benchmarkEnsureRunRecordShape(run))
    : [];
  return { sets, runs };
}

function benchmarkExtractJsonCandidate(rawText: string): string | null {
  const jsonCode = rawText.match(/```json\s*([\s\S]*?)```/i);
  if (jsonCode && jsonCode[1]) {
    return jsonCode[1].trim();
  }
  const objectBlock = rawText.match(/\{[\s\S]*\}/);
  if (objectBlock) {
    return objectBlock[0].trim();
  }
  return null;
}

function benchmarkParseJudgeResult(rawText: string): { score: number; reason: string } {
  const fallbackReason = String(rawText || "").slice(0, 180) || "未提供评分理由";
  const candidate = benchmarkExtractJsonCandidate(rawText || "");
  if (candidate) {
    try {
      const parsed = JSON.parse(candidate) as { score?: number; reason?: string };
      return {
        score: benchmarkClamp(Math.round(Number(parsed.score || 0)), 0, 100),
        reason: String(parsed.reason || fallbackReason).slice(0, 180),
      };
    } catch {
      // ignore parse error
    }
  }
  const scoreMatch = String(rawText || "").match(/(?:score|评分)[^0-9]{0,10}([0-9]{1,3})/i);
  if (scoreMatch) {
    return {
      score: benchmarkClamp(Number.parseInt(scoreMatch[1], 10), 0, 100),
      reason: fallbackReason,
    };
  }
  return { score: 0, reason: fallbackReason };
}

function benchmarkBuildJudgeQuestion(
  question: string,
  reference: string,
  candidateAnswer: string
): string {
  return [
    "[问题]",
    question,
    "",
    "[标准答案]",
    reference,
    "",
    "[候选答案]",
    candidateAnswer,
  ].join("\n");
}

function benchmarkBuildLeaderboard(runs: BenchmarkRunRecord[], setId: string) {
  const grouped: Record<
    string,
    {
      runnerKey: string;
      runnerType?: "assistant" | "model";
      runnerTargetId?: string;
      runnerLabel: string;
      runCount: number;
      totalAvgScore: number;
      bestAvgScore: number;
      latestRunAt: string;
      latestJudgeLabel: string;
      runs: BenchmarkRunRecord[];
    }
  > = {};

  runs
    .filter((run) => run.setId === setId && run.status === "success")
    .forEach((run) => {
      const runnerKey = `${run.runnerType || "unknown"}::${run.runnerTargetId || run.runnerLabel || run.id}`;
      if (!grouped[runnerKey]) {
        grouped[runnerKey] = {
          runnerKey,
          runnerType: run.runnerType,
          runnerTargetId: run.runnerTargetId,
          runnerLabel: run.runnerLabel || run.runnerTargetId || "未命名 Runner",
          runCount: 0,
          totalAvgScore: 0,
          bestAvgScore: 0,
          latestRunAt: run.createdAt,
          latestJudgeLabel: run.judgeLabel || "",
          runs: [],
        };
      }
      const entry = grouped[runnerKey];
      entry.runCount += 1;
      entry.totalAvgScore += Number(run.avgScore || 0);
      entry.bestAvgScore = Math.max(entry.bestAvgScore, Number(run.avgScore || 0));
      if (String(run.createdAt).localeCompare(String(entry.latestRunAt)) > 0) {
        entry.latestRunAt = run.createdAt;
        entry.latestJudgeLabel = run.judgeLabel || "";
      }
      entry.runs.push(run);
    });

  return Object.values(grouped)
    .map((entry) => ({
      ...entry,
      averageScore:
        entry.runCount > 0
          ? Number((entry.totalAvgScore / entry.runCount).toFixed(2))
          : 0,
      runs: [...entry.runs].sort((a, b) =>
        String(b.createdAt).localeCompare(String(a.createdAt))
      ),
    }))
    .sort((a, b) => {
      if (b.averageScore !== a.averageScore) {
        return b.averageScore - a.averageScore;
      }
      if (b.bestAvgScore !== a.bestAvgScore) {
        return b.bestAvgScore - a.bestAvgScore;
      }
      return String(b.latestRunAt).localeCompare(String(a.latestRunAt));
    });
}

function benchmarkParseCsv(csvText: string): string[][] {
  const rows: string[][] = [];
  let currentRow: string[] = [];
  let currentCell = "";
  let inQuotes = false;
  for (let i = 0; i < csvText.length; i += 1) {
    const char = csvText[i];
    const next = csvText[i + 1];
    if (char === '"') {
      if (inQuotes && next === '"') {
        currentCell += '"';
        i += 1;
      } else {
        inQuotes = !inQuotes;
      }
    } else if (char === "," && !inQuotes) {
      currentRow.push(currentCell);
      currentCell = "";
    } else if ((char === "\n" || char === "\r") && !inQuotes) {
      if (char === "\r" && next === "\n") {
        i += 1;
      }
      currentRow.push(currentCell);
      rows.push(currentRow);
      currentRow = [];
      currentCell = "";
    } else {
      currentCell += char;
    }
  }
  if (currentCell !== "" || currentRow.length > 0) {
    currentRow.push(currentCell);
    rows.push(currentRow);
  }
  return rows;
}

function benchmarkParseCsvItems(csvText: string): BenchmarkQaItem[] {
  const rows = benchmarkParseCsv(csvText).map((row) =>
    row.map((cell) => String(cell || "").trim())
  );
  if (rows.length === 0) {
    return [];
  }
  const normalizedHeader = rows[0].map((name) => name.toLowerCase());
  const hasHeader = normalizedHeader.some((name) =>
    ["question", "问题", "reference", "answer", "答案", "标准答案"].includes(name)
  );
  const questionIndex = normalizedHeader.findIndex(
    (name) => name === "question" || name === "问题"
  );
  const referenceIndex = normalizedHeader.findIndex(
    (name) =>
      name === "reference" || name === "answer" || name === "答案" || name === "标准答案"
  );

  const qIndex = questionIndex >= 0 ? questionIndex : 0;
  const rIndex = referenceIndex >= 0 ? referenceIndex : 1;
  const dataRows = hasHeader ? rows.slice(1) : rows;

  return dataRows
    .map((row) => ({
      id: benchmarkUid("qa"),
      question: String(row[qIndex] || "").trim(),
      reference: String(row[rIndex] || "").trim(),
      updatedAt: benchmarkNowIso(),
    }))
    .filter((item) => item.question && item.reference);
}
