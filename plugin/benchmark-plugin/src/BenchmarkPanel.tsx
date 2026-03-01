var ReactRuntime = (window as any).React as typeof import("react");

interface BenchmarkSelectFieldProps {
  ui: BenchmarkUiAliases;
  value: string;
  placeholder: string;
  options: BenchmarkOption[];
  allowEmpty?: boolean;
  onValueChange: (value: string) => void;
}

function BenchmarkSelectField(props: BenchmarkSelectFieldProps) {
  const {
    ui,
    value,
    placeholder,
    options,
    allowEmpty = true,
    onValueChange,
  } = props;

  if (benchmarkCanUseSelect(ui)) {
    const UISelect = ui.UISelect as import("react").ComponentType<any>;
    const UISelectTrigger = ui.UISelectTrigger as import("react").ComponentType<any>;
    const UISelectValue = ui.UISelectValue as import("react").ComponentType<any>;
    const UISelectContent = ui.UISelectContent as import("react").ComponentType<any>;
    const UISelectItem = ui.UISelectItem as import("react").ComponentType<any>;
    const currentValue = value ? String(value) : "__empty__";
    return (
      <UISelect
        value={currentValue}
        onValueChange={(nextValue: string) =>
          onValueChange(nextValue === "__empty__" ? "" : nextValue)
        }
      >
        <UISelectTrigger className="w-full">
          <UISelectValue placeholder={placeholder} />
        </UISelectTrigger>
        <UISelectContent>
          {allowEmpty ? <UISelectItem value="__empty__">{placeholder}</UISelectItem> : null}
          {options.map((option) => (
            <UISelectItem key={option.value} value={option.value}>
              {option.label}
            </UISelectItem>
          ))}
        </UISelectContent>
      </UISelect>
    );
  }

  return (
    <select
      style={{ width: "100%" }}
      value={value || ""}
      onChange={(event) => onValueChange((event.target as HTMLSelectElement).value)}
    >
      {allowEmpty ? <option value="">{placeholder}</option> : null}
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

function BenchmarkPanel(props: { systemApi: SystemApi | null }) {
  const { systemApi } = props;
  const { useState, useEffect, useMemo, useRef, useCallback } = ReactRuntime;
  const ui = benchmarkResolveUi(systemApi);
  const {
    UIAlert,
    UIAlertDescription,
    UIButton,
    UICard,
    UIDialog,
    UIDialogContent,
    UIDialogDescription,
    UIDialogHeader,
    UIDialogTitle,
    UIInput,
    UITextarea,
  } = ui;
  const hasDialogKit = benchmarkCanUseDialog(ui);

  const [store, setStore] = useState<BenchmarkState>({ sets: [], runs: [] });
  const [assistants, setAssistants] = useState<AippSystemApiAssistantItem[]>([]);
  const [models, setModels] = useState<AippSystemApiModelItem[]>([]);
  const [selectedSetId, setSelectedSetId] = useState("");
  const [newSetName, setNewSetName] = useState("");
  const [newItemQuestion, setNewItemQuestion] = useState("");
  const [newItemReference, setNewItemReference] = useState("");
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [runStatus, setRunStatus] = useState("");
  const [recordsDialogOpen, setRecordsDialogOpen] = useState(false);
  const [leaderboardDialogOpen, setLeaderboardDialogOpen] = useState(false);
  const [errorText, setErrorText] = useState("");
  const initializedRef = useRef(false);
  const csvInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!systemApi) {
      setLoading(false);
      setErrorText("SystemApi 未注入，无法加载 Benchmark 插件");
      return () => {};
    }
    (async () => {
      try {
        const [assistantList, modelList, rawState] = await Promise.all([
          systemApi.listAssistants(),
          systemApi.listModels(),
          systemApi.getData(BENCHMARK_STORAGE_KEY, BENCHMARK_STORAGE_SESSION),
        ]);
        if (cancelled) {
          return;
        }
        const nextState = benchmarkEnsureStateShape(
          benchmarkSafeParse<BenchmarkState>(rawState, { sets: [], runs: [] })
        );
        setStore(nextState);
        setAssistants(Array.isArray(assistantList) ? assistantList : []);
        setModels(Array.isArray(modelList) ? modelList : []);
        if (nextState.sets.length > 0) {
          setSelectedSetId(nextState.sets[0].id);
        }
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        setErrorText(`初始化失败: ${message}`);
      } finally {
        initializedRef.current = true;
        setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [systemApi]);

  useEffect(() => {
    if (!initializedRef.current || !systemApi) {
      return;
    }
    systemApi
      .setData(BENCHMARK_STORAGE_KEY, JSON.stringify(store), BENCHMARK_STORAGE_SESSION)
      .catch((error) => {
        console.error("[BenchmarkPlugin] persist state failed:", error);
      });
  }, [store, systemApi]);

  useEffect(() => {
    if (!selectedSetId) {
      if (store.sets.length > 0) {
        setSelectedSetId(store.sets[0].id);
      }
      return;
    }
    if (!store.sets.some((setItem) => setItem.id === selectedSetId)) {
      setSelectedSetId(store.sets.length > 0 ? store.sets[0].id : "");
    }
  }, [store.sets, selectedSetId]);

  const selectedSet = useMemo(
    () => store.sets.find((setItem) => setItem.id === selectedSetId) || null,
    [store.sets, selectedSetId]
  );

  const modelOptions = useMemo<BenchmarkOption[]>(
    () =>
      models.map((model) => ({
        value: `${model.code}%%${model.llm_provider_id}`,
        label: `${model.name} / ${model.code} (${model.code})`,
      })),
    [models]
  );

  const assistantOptions = useMemo<BenchmarkOption[]>(
    () =>
      assistants.map((assistant) => ({
        value: String(assistant.id),
        label: `${assistant.name} (#${assistant.id})`,
      })),
    [assistants]
  );

  const assistantLabelMap = useMemo(() => {
    const map: Record<string, string> = {};
    assistantOptions.forEach((option) => {
      map[option.value] = option.label;
    });
    return map;
  }, [assistantOptions]);

  const modelLabelMap = useMemo(() => {
    const map: Record<string, string> = {};
    modelOptions.forEach((option) => {
      map[option.value] = option.label;
    });
    return map;
  }, [modelOptions]);

  const runnerLeaderboard = useMemo(
    () => benchmarkBuildLeaderboard(store.runs, selectedSetId),
    [store.runs, selectedSetId]
  );

  const setRuns = useMemo(
    () => store.runs.filter((run) => run.setId === selectedSetId),
    [store.runs, selectedSetId]
  );

  const updateStore = useCallback((updater: (prev: BenchmarkState) => BenchmarkState) => {
    setStore((prev) => benchmarkEnsureStateShape(updater(prev)));
  }, []);

  const updateSelectedSet = useCallback(
    (field: keyof BenchmarkSet, value: string) => {
      if (!selectedSetId) {
        return;
      }
      updateStore((prev) => ({
        ...prev,
        sets: prev.sets.map((setItem) =>
          setItem.id === selectedSetId
            ? {
                ...setItem,
                [field]: value,
                updatedAt: benchmarkNowIso(),
              }
            : setItem
        ),
      }));
    },
    [selectedSetId, updateStore]
  );

  const createSet = useCallback(() => {
    const trimmedName = String(newSetName || "").trim();
    if (!trimmedName) {
      setErrorText("请填写 QA 集名称");
      return;
    }
    const defaultAssistantId = assistantOptions.length > 0 ? assistantOptions[0].value : "";
    const defaultModelId = modelOptions.length > 0 ? modelOptions[0].value : "";
    const setItem = benchmarkEnsureSetShape({
      id: benchmarkUid("set"),
      name: trimmedName,
      judgePrompt: BENCHMARK_DEFAULT_JUDGE_PROMPT,
      runnerType: "assistant",
      runnerAssistantId: defaultAssistantId,
      runnerModelId: defaultModelId,
      judgeType: "assistant",
      judgeAssistantId: defaultAssistantId,
      judgeModelId: defaultModelId,
      items: [],
      createdAt: benchmarkNowIso(),
      updatedAt: benchmarkNowIso(),
    });
    updateStore((prev) => ({
      ...prev,
      sets: [setItem, ...prev.sets],
    }));
    setSelectedSetId(setItem.id);
    setNewSetName("");
    setErrorText("");
  }, [newSetName, assistantOptions, modelOptions, updateStore]);

  const deleteSet = useCallback(() => {
    if (!selectedSet) {
      return;
    }
    updateStore((prev) => ({
      ...prev,
      sets: prev.sets.filter((setItem) => setItem.id !== selectedSet.id),
      runs: prev.runs.filter((run) => run.setId !== selectedSet.id),
    }));
    setErrorText("");
  }, [selectedSet, updateStore]);

  const addManualItem = useCallback(() => {
    if (!selectedSet) {
      setErrorText("请先选择 QA 集");
      return;
    }
    const question = String(newItemQuestion || "").trim();
    const reference = String(newItemReference || "").trim();
    if (!question || !reference) {
      setErrorText("问题和标准答案都不能为空");
      return;
    }
    const item: BenchmarkQaItem = {
      id: benchmarkUid("qa"),
      question,
      reference,
      updatedAt: benchmarkNowIso(),
    };
    updateStore((prev) => ({
      ...prev,
      sets: prev.sets.map((setItem) =>
        setItem.id === selectedSet.id
          ? {
              ...setItem,
              items: [...setItem.items, item],
              updatedAt: benchmarkNowIso(),
            }
          : setItem
      ),
    }));
    setNewItemQuestion("");
    setNewItemReference("");
    setErrorText("");
  }, [selectedSet, newItemQuestion, newItemReference, updateStore]);

  const updateItemField = useCallback(
    (itemId: string, field: "question" | "reference", value: string) => {
      if (!selectedSet) {
        return;
      }
      updateStore((prev) => ({
        ...prev,
        sets: prev.sets.map((setItem) =>
          setItem.id === selectedSet.id
            ? {
                ...setItem,
                items: setItem.items.map((item) =>
                  item.id === itemId
                    ? { ...item, [field]: value, updatedAt: benchmarkNowIso() }
                    : item
                ),
                updatedAt: benchmarkNowIso(),
              }
            : setItem
        ),
      }));
    },
    [selectedSet, updateStore]
  );

  const removeItem = useCallback(
    (itemId: string) => {
      if (!selectedSet) {
        return;
      }
      updateStore((prev) => ({
        ...prev,
        sets: prev.sets.map((setItem) =>
          setItem.id === selectedSet.id
            ? {
                ...setItem,
                items: setItem.items.filter((item) => item.id !== itemId),
                updatedAt: benchmarkNowIso(),
              }
            : setItem
        ),
      }));
    },
    [selectedSet, updateStore]
  );

  const handleCsvImport = useCallback(
    async (event: import("react").ChangeEvent<HTMLInputElement>) => {
      if (!selectedSet) {
        setErrorText("请先选择 QA 集");
        return;
      }
      const file = event.target.files && event.target.files[0];
      if (!file) {
        return;
      }
      try {
        const text = await file.text();
        const importedItems = benchmarkParseCsvItems(text);
        if (importedItems.length === 0) {
          throw new Error("CSV 中未识别到有效 QA 条目");
        }
        updateStore((prev) => ({
          ...prev,
          sets: prev.sets.map((setItem) =>
            setItem.id === selectedSet.id
              ? {
                  ...setItem,
                  items: [...setItem.items, ...importedItems],
                  updatedAt: benchmarkNowIso(),
                }
              : setItem
          ),
        }));
        setErrorText("");
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        setErrorText(`CSV 导入失败: ${message}`);
      } finally {
        event.target.value = "";
      }
    },
    [selectedSet, updateStore]
  );

  const runBenchmark = useCallback(async () => {
    if (!selectedSet || !systemApi) {
      setErrorText("请先选择 QA 集");
      return;
    }
    if (!Array.isArray(selectedSet.items) || selectedSet.items.length === 0) {
      setErrorText("当前 QA 集为空，请先录入题目");
      return;
    }
    if (selectedSet.runnerType === "assistant" && !selectedSet.runnerAssistantId) {
      setErrorText("Runner 为 assistant 模式时必须选择助手");
      return;
    }
    if (selectedSet.runnerType === "model" && !selectedSet.runnerModelId) {
      setErrorText("Runner 为 model 模式时必须选择模型");
      return;
    }
    if (selectedSet.judgeType === "assistant" && !selectedSet.judgeAssistantId) {
      setErrorText("Judge 为 assistant 模式时必须选择助手");
      return;
    }
    if (selectedSet.judgeType === "model" && !selectedSet.judgeModelId) {
      setErrorText("Judge 为 model 模式时必须选择模型");
      return;
    }
    if (selectedSet.judgeType === "model" && !String(selectedSet.judgePrompt || "").trim()) {
      setErrorText("Judge 为 model 模式时必须配置 System Prompt");
      return;
    }

    setRunning(true);
    setRunStatus("Benchmark 运行中...");
    setErrorText("");

    const runId = benchmarkUid("run");
    const runItems: BenchmarkRunItem[] = [];
    let totalScore = 0;
    const judgeSystemPromptUsed =
      selectedSet.judgeType === "model"
        ? String(selectedSet.judgePrompt || "").trim() || BENCHMARK_DEFAULT_JUDGE_PROMPT
        : "";

    try {
      for (let index = 0; index < selectedSet.items.length; index += 1) {
        const item = selectedSet.items[index];
        setRunStatus(`运行中 (${index + 1}/${selectedSet.items.length}) - ${item.id}`);
        const runnerAnswer =
          selectedSet.runnerType === "model"
            ? await benchmarkRunModelText(systemApi, {
                modelId: selectedSet.runnerModelId,
                question: item.question,
              })
            : await benchmarkRunAssistantText(systemApi, {
                assistantId: selectedSet.runnerAssistantId,
                question: item.question,
              });

        const judgeQuestion = benchmarkBuildJudgeQuestion(
          item.question,
          item.reference,
          runnerAnswer
        );
        const judgeRaw =
          selectedSet.judgeType === "model"
            ? await benchmarkRunModelText(systemApi, {
                modelId: selectedSet.judgeModelId,
                question: judgeQuestion,
                systemPrompt: judgeSystemPromptUsed,
              })
            : await benchmarkRunAssistantText(systemApi, {
                assistantId: selectedSet.judgeAssistantId,
                question: judgeQuestion,
              });

        const judge = benchmarkParseJudgeResult(judgeRaw);
        totalScore += judge.score;
        runItems.push({
          itemId: item.id,
          question: item.question,
          reference: item.reference,
          answer: runnerAnswer,
          judgeQuestion,
          score: judge.score,
          reason: judge.reason,
          judgeRaw: String(judgeRaw || "").slice(0, 1000),
        });
      }

      const runRecord: BenchmarkRunRecord = {
        id: runId,
        setId: selectedSet.id,
        setName: selectedSet.name,
        createdAt: benchmarkNowIso(),
        status: "success",
        totalScore,
        avgScore:
          runItems.length > 0 ? Number((totalScore / runItems.length).toFixed(2)) : 0,
        runnerType: selectedSet.runnerType,
        runnerTargetId:
          selectedSet.runnerType === "model"
            ? selectedSet.runnerModelId
            : selectedSet.runnerAssistantId,
        runnerLabel:
          selectedSet.runnerType === "model"
            ? modelLabelMap[selectedSet.runnerModelId] || selectedSet.runnerModelId
            : assistantLabelMap[selectedSet.runnerAssistantId] ||
              selectedSet.runnerAssistantId,
        judgeType: selectedSet.judgeType,
        judgeTargetId:
          selectedSet.judgeType === "model"
            ? selectedSet.judgeModelId
            : selectedSet.judgeAssistantId,
        judgeLabel:
          selectedSet.judgeType === "model"
            ? modelLabelMap[selectedSet.judgeModelId] || selectedSet.judgeModelId
            : assistantLabelMap[selectedSet.judgeAssistantId] ||
              selectedSet.judgeAssistantId,
        judgeSystemPrompt: judgeSystemPromptUsed || undefined,
        items: runItems,
      };

      updateStore((prev) => ({
        ...prev,
        runs: [runRecord, ...prev.runs].slice(0, BENCHMARK_MAX_RUN_RECORDS),
      }));
      setRunStatus(`运行完成：总分 ${runRecord.totalScore}，平均分 ${runRecord.avgScore}`);
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      const failedRecord: BenchmarkRunRecord = {
        id: runId,
        setId: selectedSet.id,
        setName: selectedSet.name,
        createdAt: benchmarkNowIso(),
        status: "failed",
        totalScore,
        avgScore:
          runItems.length > 0 ? Number((totalScore / runItems.length).toFixed(2)) : 0,
        runnerType: selectedSet.runnerType,
        runnerTargetId:
          selectedSet.runnerType === "model"
            ? selectedSet.runnerModelId
            : selectedSet.runnerAssistantId,
        runnerLabel:
          selectedSet.runnerType === "model"
            ? modelLabelMap[selectedSet.runnerModelId] || selectedSet.runnerModelId
            : assistantLabelMap[selectedSet.runnerAssistantId] ||
              selectedSet.runnerAssistantId,
        judgeType: selectedSet.judgeType,
        judgeTargetId:
          selectedSet.judgeType === "model"
            ? selectedSet.judgeModelId
            : selectedSet.judgeAssistantId,
        judgeLabel:
          selectedSet.judgeType === "model"
            ? modelLabelMap[selectedSet.judgeModelId] || selectedSet.judgeModelId
            : assistantLabelMap[selectedSet.judgeAssistantId] ||
              selectedSet.judgeAssistantId,
        judgeSystemPrompt: judgeSystemPromptUsed || undefined,
        items: runItems,
        error: reason,
      };
      updateStore((prev) => ({
        ...prev,
        runs: [failedRecord, ...prev.runs].slice(0, BENCHMARK_MAX_RUN_RECORDS),
      }));
      setRunStatus(`运行失败：${reason}`);
      setErrorText(`Benchmark 运行失败：${reason}`);
    } finally {
      setRunning(false);
    }
  }, [selectedSet, systemApi, updateStore, assistantLabelMap, modelLabelMap]);

  const renderRunItemDetails = (run: BenchmarkRunRecord, keyPrefix: string) => {
    if (run.items.length === 0) {
      return <div className="text-muted-foreground">该次运行暂无题目记录。</div>;
    }
    return run.items.map((item) => (
      <details key={`${keyPrefix}_${run.id}_${item.itemId}`} className="rounded border border-border/70 p-2">
        <summary className="cursor-pointer font-medium">
          {`${item.itemId} | ${item.score} 分 | ${item.reason || "无理由"}`}
        </summary>
        <div className="mt-2 grid grid-cols-1 gap-2">
          <div className="rounded bg-muted/50 p-2">
            <div className="font-medium mb-1">题目</div>
            <pre className="whitespace-pre-wrap break-words">{item.question}</pre>
          </div>
          <div className="rounded bg-muted/50 p-2">
            <div className="font-medium mb-1">标准答案</div>
            <pre className="whitespace-pre-wrap break-words">{item.reference}</pre>
          </div>
          <div className="rounded bg-muted/50 p-2">
            <div className="font-medium mb-1">Runner 回答</div>
            <pre className="whitespace-pre-wrap break-words">{item.answer}</pre>
          </div>
          <div className="rounded bg-muted/50 p-2">
            <div className="font-medium mb-1">Judge 提问</div>
            <pre className="whitespace-pre-wrap break-words">{item.judgeQuestion || "-"}</pre>
          </div>
          <div className="rounded bg-muted/50 p-2">
            <div className="font-medium mb-1">Judge 回答</div>
            <pre className="whitespace-pre-wrap break-words">{item.judgeRaw || "-"}</pre>
          </div>
        </div>
      </details>
    ));
  };

  if (loading) {
    return <div className="p-2 text-sm text-muted-foreground">Benchmark 插件加载中...</div>;
  }

  if (!systemApi) {
    return (
      <UIAlert variant="destructive" className="py-2">
        <UIAlertDescription className="text-xs">
          SystemApi 未注入，无法渲染 Benchmark 插件。
        </UIAlertDescription>
      </UIAlert>
    );
  }

  return (
    <div className="px-1 md:px-2" style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
      <div style={{ display: "flex", gap: "8px", alignItems: "center", flexWrap: "wrap" }}>
        <strong>Benchmark 插件</strong>
        <span className="text-xs text-muted-foreground">
          独立界面：QA 管理 / CSV 导入 / 运行 / 排行榜
        </span>
      </div>

      {errorText ? (
        <UIAlert variant="destructive" className="py-2">
          <UIAlertDescription className="text-xs">{errorText}</UIAlertDescription>
        </UIAlert>
      ) : null}

      <UICard className="shadow-none gap-3 py-3 px-3 md:px-4">
        <strong>QA 集</strong>
        <div style={{ display: "grid", gridTemplateColumns: "2fr 2fr auto", gap: "8px" }}>
          <BenchmarkSelectField
            ui={ui}
            value={selectedSetId}
            placeholder="请选择 QA 集"
            allowEmpty
            options={store.sets.map((setItem) => ({
              value: setItem.id,
              label: setItem.name,
            }))}
            onValueChange={setSelectedSetId}
          />
          <UIInput
            placeholder="新 QA 集名称"
            value={newSetName}
            onChange={(event: any) => setNewSetName(event.target.value)}
          />
          <UIButton size="sm" variant="outline" type="button" onClick={createSet}>
            创建
          </UIButton>
        </div>
        {selectedSet ? (
          <div style={{ display: "grid", gridTemplateColumns: "1fr auto", gap: "8px" }}>
            <UIInput
              value={selectedSet.name}
              onChange={(event: any) => updateSelectedSet("name", event.target.value)}
            />
            <UIButton size="sm" variant="outline" type="button" onClick={deleteSet}>
              删除 QA 集
            </UIButton>
          </div>
        ) : null}
      </UICard>

      {selectedSet ? (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: "10px",
            alignItems: "start",
          }}
        >
          <UICard className="shadow-none gap-2 py-3 px-3 md:px-4">
            <strong>Runner 配置</strong>
            <BenchmarkSelectField
              ui={ui}
              value={selectedSet.runnerType}
              placeholder="选择 Runner 类型"
              allowEmpty={false}
              options={[
                { value: "assistant", label: "assistant" },
                { value: "model", label: "model" },
              ]}
              onValueChange={(value) => updateSelectedSet("runnerType", value)}
            />
            {selectedSet.runnerType === "assistant" ? (
              <BenchmarkSelectField
                ui={ui}
                value={selectedSet.runnerAssistantId}
                placeholder="选择 Runner 助手"
                allowEmpty
                options={assistantOptions}
                onValueChange={(value) => updateSelectedSet("runnerAssistantId", value)}
              />
            ) : (
              <BenchmarkSelectField
                ui={ui}
                value={selectedSet.runnerModelId}
                placeholder="选择 Runner 模型"
                allowEmpty
                options={modelOptions}
                onValueChange={(value) => updateSelectedSet("runnerModelId", value)}
              />
            )}
          </UICard>

          <UICard className="shadow-none gap-2 py-3 px-3 md:px-4">
            <strong>Judge 配置（每个 QA 集独立）</strong>
            <BenchmarkSelectField
              ui={ui}
              value={selectedSet.judgeType}
              placeholder="选择 Judge 类型"
              allowEmpty={false}
              options={[
                { value: "assistant", label: "assistant" },
                { value: "model", label: "model" },
              ]}
              onValueChange={(value) => updateSelectedSet("judgeType", value)}
            />
            {selectedSet.judgeType === "assistant" ? (
              <BenchmarkSelectField
                ui={ui}
                value={selectedSet.judgeAssistantId}
                placeholder="选择 Judge 助手"
                allowEmpty
                options={assistantOptions}
                onValueChange={(value) => updateSelectedSet("judgeAssistantId", value)}
              />
            ) : (
              <BenchmarkSelectField
                ui={ui}
                value={selectedSet.judgeModelId}
                placeholder="选择 Judge 模型"
                allowEmpty
                options={modelOptions}
                onValueChange={(value) => updateSelectedSet("judgeModelId", value)}
              />
            )}
            {selectedSet.judgeType === "model" ? (
              <UITextarea
                style={{ minHeight: "86px" }}
                value={selectedSet.judgePrompt}
                onChange={(event: any) => updateSelectedSet("judgePrompt", event.target.value)}
                placeholder="Judge System Prompt（仅 model 模式）"
              />
            ) : (
              <div className="text-xs text-muted-foreground">
                Judge 为 assistant 模式时，将使用所选助手的 System Prompt。
              </div>
            )}
          </UICard>
        </div>
      ) : null}

      {selectedSet ? (
        <UICard className="shadow-none gap-2 py-3 px-3 md:px-4">
          <strong>QA 条目（手工录入 / CSV 导入）</strong>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "1fr 1fr auto auto",
              gap: "8px",
              alignItems: "center",
            }}
          >
            <UIInput
              value={newItemQuestion}
              onChange={(event: any) => setNewItemQuestion(event.target.value)}
              placeholder="问题"
            />
            <UIInput
              value={newItemReference}
              onChange={(event: any) => setNewItemReference(event.target.value)}
              placeholder="标准答案"
            />
            <UIButton size="sm" variant="outline" type="button" onClick={addManualItem}>
              新增条目
            </UIButton>
            <UIButton
              size="sm"
              variant="outline"
              type="button"
              onClick={() => csvInputRef.current?.click()}
            >
              CSV 导入
            </UIButton>
          </div>

          <input
            ref={csvInputRef}
            type="file"
            accept=".csv,text/csv"
            style={{ display: "none" }}
            onChange={handleCsvImport}
          />

          <div
            style={{
              maxHeight: "260px",
              overflowY: "auto",
              border: "1px solid var(--border)",
              borderRadius: "6px",
            }}
          >
            {selectedSet.items.length === 0 ? (
              <div className="p-2.5 text-xs text-muted-foreground">当前 QA 集暂无条目。</div>
            ) : (
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: "12px" }}>
                <thead>
                  <tr style={{ background: "var(--muted)" }}>
                    <th style={{ textAlign: "left", padding: "8px", width: "24%" }}>ID</th>
                    <th style={{ textAlign: "left", padding: "8px", width: "33%" }}>问题</th>
                    <th style={{ textAlign: "left", padding: "8px", width: "33%" }}>标准答案</th>
                    <th style={{ textAlign: "left", padding: "8px", width: "10%" }}>操作</th>
                  </tr>
                </thead>
                <tbody>
                  {selectedSet.items.map((item) => (
                    <tr key={item.id} style={{ borderTop: "1px solid var(--border)" }}>
                      <td style={{ padding: "6px 8px", verticalAlign: "top" }}>{item.id}</td>
                      <td style={{ padding: "6px 8px", verticalAlign: "top" }}>
                        <UITextarea
                          style={{ minHeight: "58px" }}
                          value={item.question}
                          onChange={(event: any) =>
                            updateItemField(item.id, "question", event.target.value)
                          }
                        />
                      </td>
                      <td style={{ padding: "6px 8px", verticalAlign: "top" }}>
                        <UITextarea
                          style={{ minHeight: "58px" }}
                          value={item.reference}
                          onChange={(event: any) =>
                            updateItemField(item.id, "reference", event.target.value)
                          }
                        />
                      </td>
                      <td style={{ padding: "6px 8px", verticalAlign: "top" }}>
                        <UIButton
                          size="sm"
                          variant="outline"
                          type="button"
                          onClick={() => removeItem(item.id)}
                        >
                          删除
                        </UIButton>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </UICard>
      ) : null}

      {selectedSet ? (
        <UICard className="shadow-none gap-2 py-3 px-3 md:px-4">
          <strong>运行</strong>
          <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
            <UIButton
              size="sm"
              variant={running ? "secondary" : "default"}
              type="button"
              disabled={running}
              onClick={runBenchmark}
            >
              {running ? "运行中..." : "开始 Benchmark"}
            </UIButton>
            <span className="text-xs text-muted-foreground">{runStatus || "未开始运行"}</span>
          </div>
          <div style={{ display: "flex", gap: "8px", flexWrap: "wrap", alignItems: "center" }}>
            <UIButton
              size="sm"
              variant="outline"
              type="button"
              disabled={!hasDialogKit}
              onClick={() => setRecordsDialogOpen(true)}
            >
              运行记录（{setRuns.length}）
            </UIButton>
            <UIButton
              size="sm"
              variant="outline"
              type="button"
              disabled={!hasDialogKit || runnerLeaderboard.length === 0}
              onClick={() => setLeaderboardDialogOpen(true)}
            >
              Runner 排行榜（{runnerLeaderboard.length}）
            </UIButton>
          </div>
          <div className="text-xs text-muted-foreground">
            {`当前按 QA 集「${selectedSet.name}」统计运行记录与排行榜。`}
          </div>
          {!hasDialogKit ? (
            <div className="text-xs text-muted-foreground">
              当前宿主未注入 Dialog 组件，无法弹窗展示运行记录与排行榜。
            </div>
          ) : null}
        </UICard>
      ) : null}

      {selectedSet && hasDialogKit ? (
        <>
          <UIDialog open={recordsDialogOpen} onOpenChange={setRecordsDialogOpen}>
            <UIDialogContent style={{ maxWidth: "min(1200px, calc(100% - 2rem))" }}>
              <UIDialogHeader>
                <UIDialogTitle>运行记录详情</UIDialogTitle>
                <UIDialogDescription>
                  {`QA 集：${selectedSet.name}（${selectedSet.id}）｜展示每次运行的 Runner/Judge 配置和每题问答、判题明细。`}
                </UIDialogDescription>
              </UIDialogHeader>
              <div
                style={{
                  maxHeight: "70vh",
                  overflowY: "auto",
                  paddingRight: "4px",
                  display: "flex",
                  flexDirection: "column",
                  gap: "12px",
                }}
              >
                {setRuns.length === 0 ? (
                  <div className="text-sm text-muted-foreground">暂无运行记录。</div>
                ) : (
                  setRuns.map((run) => (
                    <div key={run.id} className="rounded-md border border-border p-3 space-y-2 text-xs">
                      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                        <span className="font-semibold">{run.createdAt}</span>
                        <span>{run.status === "success" ? "成功" : "失败"}</span>
                        <span>{`总分 ${run.totalScore}`}</span>
                        <span>{`平均 ${run.avgScore}`}</span>
                      </div>
                      <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
                        <div className="rounded border border-border/70 p-2 space-y-1">
                          <div className="font-medium">Runner 配置</div>
                          <div>{`类型: ${run.runnerType || "unknown"}`}</div>
                          <div>{`目标: ${run.runnerLabel || run.runnerTargetId || "-"}`}</div>
                        </div>
                        <div className="rounded border border-border/70 p-2 space-y-1">
                          <div className="font-medium">Judge 配置</div>
                          <div>{`类型: ${run.judgeType || "unknown"}`}</div>
                          <div>{`目标: ${run.judgeLabel || run.judgeTargetId || "-"}`}</div>
                          {run.judgeType === "model" && run.judgeSystemPrompt ? (
                            <details>
                              <summary className="cursor-pointer text-muted-foreground">
                                查看 Judge System Prompt
                              </summary>
                              <pre className="mt-1 whitespace-pre-wrap break-words rounded bg-muted p-2">
                                {run.judgeSystemPrompt}
                              </pre>
                            </details>
                          ) : null}
                        </div>
                      </div>
                      {run.error ? (
                        <div className="rounded border border-destructive/50 bg-destructive/5 p-2 text-destructive">
                          {`失败原因: ${run.error}`}
                        </div>
                      ) : null}
                      <div className="space-y-2">{renderRunItemDetails(run, "records")}</div>
                    </div>
                  ))
                )}
              </div>
            </UIDialogContent>
          </UIDialog>

          <UIDialog open={leaderboardDialogOpen} onOpenChange={setLeaderboardDialogOpen}>
            <UIDialogContent style={{ maxWidth: "min(1200px, calc(100% - 2rem))" }}>
              <UIDialogHeader>
                <UIDialogTitle>排行榜详情</UIDialogTitle>
                <UIDialogDescription>
                  {`QA 集：${selectedSet.name}（${selectedSet.id}）｜按 Runner 的 QA 集平均分排序。`}
                </UIDialogDescription>
              </UIDialogHeader>
              <div
                style={{
                  maxHeight: "70vh",
                  overflowY: "auto",
                  paddingRight: "4px",
                  display: "flex",
                  flexDirection: "column",
                  gap: "12px",
                }}
              >
                {runnerLeaderboard.length === 0 ? (
                  <div className="text-sm text-muted-foreground">暂无可排行的成功运行记录。</div>
                ) : (
                  runnerLeaderboard.map((entry, index) => (
                    <div
                      key={`runner_rank_${entry.runnerKey}`}
                      className="rounded-md border border-border p-3 space-y-2 text-xs"
                    >
                      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                        <span className="font-semibold">{`#${index + 1} ${entry.runnerLabel}`}</span>
                        <span>{`QA 集平均分 ${entry.averageScore}`}</span>
                        <span>{`最佳平均分 ${entry.bestAvgScore}`}</span>
                        <span>{`运行次数 ${entry.runCount}`}</span>
                      </div>
                      <div className="text-muted-foreground">
                        {`Runner 类型: ${entry.runnerType || "unknown"} | Runner 标识: ${
                          entry.runnerTargetId || "-"
                        } | 最近Judge: ${entry.latestJudgeLabel || "-"}`}
                      </div>
                      <div className="space-y-2">
                        {entry.runs.map((run, runIndex) => (
                          <details
                            key={`${entry.runnerKey}_${run.id}_${runIndex}`}
                            className="rounded border border-border/70 p-2"
                          >
                            <summary className="cursor-pointer font-medium">
                              {`${run.createdAt} | ${run.status === "success" ? "成功" : "失败"} | 总分 ${
                                run.totalScore
                              } | 平均 ${run.avgScore} | Judge ${run.judgeLabel}`}
                            </summary>
                            <div className="mt-2 grid grid-cols-1 gap-2">
                              {run.judgeType === "model" && run.judgeSystemPrompt ? (
                                <div className="rounded bg-muted/50 p-2">
                                  <div className="font-medium mb-1">Judge System Prompt</div>
                                  <pre className="whitespace-pre-wrap break-words">
                                    {run.judgeSystemPrompt}
                                  </pre>
                                </div>
                              ) : null}
                              {run.error ? (
                                <div className="rounded border border-destructive/50 bg-destructive/5 p-2 text-destructive">
                                  {`失败原因: ${run.error}`}
                                </div>
                              ) : null}
                              <div className="space-y-2">{renderRunItemDetails(run, entry.runnerKey)}</div>
                            </div>
                          </details>
                        ))}
                      </div>
                    </div>
                  ))
                )}
              </div>
            </UIDialogContent>
          </UIDialog>
        </>
      ) : null}
    </div>
  );
}
