var ComfyReact = (window as any).React as typeof import("react");

type ComfyActionContext = { conversationId?: number | null; messageId?: number | null; messageType?: string; messageContent?: string };
type ComfyConfig = { modelId: string; instruction: string; baseUrl: string };
type ComfyHookContext = { conversationId?: number; assistantMessageId?: number | null };
type ComfyPluginInstance = {
  requireApi(): SystemApi;
  showError(error: unknown): void;
  loadConfig(): Promise<ComfyConfig>;
  saveConfig(config: ComfyConfig): Promise<void>;
  latestJobStatus(): Promise<string>;
  getAutoEnabled(conversationId: number): Promise<boolean>;
  setAutoEnabled(conversationId: number, enabled: boolean): Promise<void>;
  generateLatest(conversationId: number): Promise<void>;
  generateMessage(conversationId: number, messageId: number, messageContent?: string): Promise<void>;
};

var COMFY_MODEL_KEY = "prompt_model_id";
var COMFY_INSTRUCTION_KEY = "prompt_instruction";
var COMFY_URL_KEY = "comfyui_base_url";
var COMFY_AUTO_KEY = "auto_enabled";
var COMFY_DEFAULT_INSTRUCTION = "请把下面的 AI 回复转换为适合文生图模型的中文提示词。只输出提示词，不要解释。\n\n{{assistant_reply}}";

function comfyError(error: unknown): string {
  return error instanceof Error ? error.message : String(error || "未知错误");
}

function comfyModelId(model: AippSystemApiModelItem): string {
  return String(model.code) + "%%" + String(model.llm_provider_id);
}

function comfyModelLabel(model: AippSystemApiModelItem): string {
  return model.name && model.name !== model.code ? model.name + " (" + model.code + ")" : model.code;
}

function comfySessionId(conversationId: number): string {
  return String(conversationId);
}

function comfyRenderInstruction(template: string, assistantReply: string): string {
  if (!template.includes("{{assistant_reply}}")) {
    throw new Error("提示词生成指令必须包含 {{assistant_reply}}");
  }
  return template.split("{{assistant_reply}}").join(assistantReply);
}

function comfyFindMessage(messages: AippSystemApiMessage[], messageId: number): AippSystemApiMessage | null {
  return (messages || []).find(function (message) {
    return message.id === messageId && (message.message_type === "assistant" || message.message_type === "response");
  }) || null;
}

function comfyLatestAssistantMessage(messages: AippSystemApiMessage[]): AippSystemApiMessage | null {
  var candidates = (messages || []).filter(function (message) {
    return (message.message_type === "assistant" || message.message_type === "response") && String(message.content || "").trim();
  });
  return candidates.length ? candidates[candidates.length - 1] : null;
}

function ComfyImageIcon(props: { active?: boolean; play?: boolean }) {
  return <svg className="text-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <circle cx="9" cy="9" r="2" />
    <path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21" />
    {props.play ? <path d="m15 8 4 2.5-4 2.5z" fill="currentColor" /> : <circle cx="18" cy="6" r="2" fill={props.active ? "currentColor" : "none"} />}
  </svg>;
}

function ComfyIconButton(props: { systemApi: SystemApi; title: string; active?: boolean; disabled?: boolean; play?: boolean; onClick: () => void }) {
  var IconButton = props.systemApi.ui && props.systemApi.ui.IconButton;
  var icon = <ComfyImageIcon active={props.active} play={props.play} />;
  if (IconButton) return <IconButton icon={icon} title={props.title} onClick={props.onClick} disabled={props.disabled} className={props.active ? "bg-accent" : ""} />;
  var Button = props.systemApi.ui && props.systemApi.ui.Button;
  return Button ? <Button variant={props.active ? "secondary" : "outline"} size="icon" title={props.title} onClick={props.onClick} disabled={props.disabled}>{icon}</Button> : null;
}

function ComfyToggleAction(props: { plugin: ComfyPluginInstance; context: ComfyActionContext }) {
  var conversationId = Number(props.context.conversationId || 0);
  var [enabled, setEnabled] = ComfyReact.useState(false);
  var [loading, setLoading] = ComfyReact.useState(true);
  ComfyReact.useEffect(function () {
    var active = true;
    if (!conversationId) { setLoading(false); return function () { active = false; }; }
    props.plugin.getAutoEnabled(conversationId).then(function (value) { if (active) setEnabled(value); }).finally(function () { if (active) setLoading(false); });
    return function () { active = false; };
  }, [conversationId]);
  return <ComfyIconButton systemApi={props.plugin.requireApi()} active={enabled} disabled={!conversationId || loading} title={enabled ? "关闭当前会话自动生图" : "开启当前会话自动生图"} onClick={function () {
    setLoading(true);
    props.plugin.setAutoEnabled(conversationId, !enabled).then(function () { setEnabled(!enabled); }).catch(function (error) { props.plugin.showError(error); }).finally(function () { setLoading(false); });
  }} />;
}

function ComfyManualAction(props: { plugin: ComfyPluginInstance; context: ComfyActionContext }) {
  var conversationId = Number(props.context.conversationId || 0);
  var messageId = Number(props.context.messageId || 0);
  var [loading, setLoading] = ComfyReact.useState(false);
  return <ComfyIconButton systemApi={props.plugin.requireApi()} play disabled={!conversationId || !messageId || loading} title={loading ? "正在生成图片" : "为这条回复立即生图"} onClick={function () {
    setLoading(true);
    props.plugin.generateMessage(conversationId, messageId, props.context.messageContent).then(function () { props.plugin.requireApi().toast?.success("图片已附加到这条 AI 回复"); }).catch(function (error) { props.plugin.showError(error); }).finally(function () { setLoading(false); });
  }} />;
}

function ComfyConfigPanel(props: { plugin: ComfyPluginInstance }) {
  var api = props.plugin.requireApi();
  var ui = api.ui || {};
  var Button = ui.Button || "button" as any;
  var Input = ui.Input || "input" as any;
  var Textarea = ui.Textarea || "textarea" as any;
  var Card = ui.Card || "div" as any;
  var CardHeader = ui.CardHeader || "div" as any;
  var CardTitle = ui.CardTitle || "h3" as any;
  var CardDescription = ui.CardDescription || "p" as any;
  var CardContent = ui.CardContent || "div" as any;
  var Select = ui.Select;
  var SelectTrigger = ui.SelectTrigger;
  var SelectValue = ui.SelectValue;
  var SelectContent = ui.SelectContent;
  var SelectItem = ui.SelectItem;
  var [models, setModels] = ComfyReact.useState<AippSystemApiModelItem[]>([]);
  var [modelId, setModelId] = ComfyReact.useState("");
  var [instruction, setInstruction] = ComfyReact.useState(COMFY_DEFAULT_INSTRUCTION);
  var [baseUrl, setBaseUrl] = ComfyReact.useState("http://127.0.0.1:8188");
  var [status, setStatus] = ComfyReact.useState("");
  var [busy, setBusy] = ComfyReact.useState(false);
  ComfyReact.useEffect(function () {
    Promise.all([api.listModels(), props.plugin.loadConfig(), props.plugin.latestJobStatus()]).then(function (values) {
      setModels(values[0]); setModelId(values[1].modelId); setInstruction(values[1].instruction || COMFY_DEFAULT_INSTRUCTION); setBaseUrl(values[1].baseUrl || "http://127.0.0.1:8188"); setStatus(values[2]);
      if (values[1].modelId && !values[0].some(function (model) { return comfyModelId(model) === values[1].modelId; })) setStatus("错误：已配置的提示词模型不存在，请重新选择并保存。");
    }).catch(function (error) { setStatus("错误：" + comfyError(error)); });
  }, []);
  ComfyReact.useEffect(function () {
    var timer = setInterval(function () {
      props.plugin.latestJobStatus().then(setStatus).catch(function () {});
    }, 2000);
    return function () { clearInterval(timer); };
  }, []);
  function validate(): ComfyConfig {
    var config = { modelId: modelId.trim(), instruction: instruction.trim(), baseUrl: baseUrl.trim() };
    if (!config.modelId) throw new Error("请选择提示词生成模型");
    if (!models.some(function (model) { return comfyModelId(model) === config.modelId; })) throw new Error("提示词生成模型不存在，请重新选择");
    if (!config.instruction) throw new Error("请填写提示词生成指令");
    if (!config.instruction.includes("{{assistant_reply}}")) throw new Error("提示词生成指令必须包含 {{assistant_reply}}");
    if (!/^https?:\/\//i.test(config.baseUrl)) throw new Error("ComfyUI 地址必须使用 http 或 https");
    return config;
  }
  return <Card className="w-full">
    <CardHeader><CardTitle>ComfyUI 自动生图</CardTitle><CardDescription>AI 回复完成后生成图片，并附加在同一条回复底部。</CardDescription></CardHeader>
    <CardContent className="space-y-4">
      <div className="space-y-2"><label className="text-sm font-medium">提示词生成模型</label>
        {Select && SelectTrigger && SelectValue && SelectContent && SelectItem ? <Select value={modelId} onValueChange={setModelId}><SelectTrigger><SelectValue placeholder="选择模型" /></SelectTrigger><SelectContent>{models.map(function (model) { return <SelectItem value={comfyModelId(model)}>{comfyModelLabel(model)}</SelectItem>; })}</SelectContent></Select> : <select className="w-full" value={modelId} onChange={function (event) { setModelId(event.target.value); }}><option value="">选择模型</option>{models.map(function (model) { return <option key={comfyModelId(model)} value={comfyModelId(model)}>{comfyModelLabel(model)}</option>; })}</select>}
      </div>
      <div className="space-y-2"><label className="text-sm font-medium">提示词生成指令</label><Textarea value={instruction} rows={7} onChange={function (event: any) { setInstruction(event.target.value); }} /><p className="text-xs text-muted-foreground">必须包含 {"{{assistant_reply}}"}，运行时替换为目标 AI 回复。</p></div>
      <div className="space-y-2"><label className="text-sm font-medium">ComfyUI 地址</label><Input value={baseUrl} onChange={function (event: any) { setBaseUrl(event.target.value); }} placeholder="http://127.0.0.1:8188" /></div>
      <div className="flex gap-2"><Button disabled={busy} onClick={function () { var address = baseUrl.trim(); if (!/^https?:\/\//i.test(address)) { setStatus("错误：ComfyUI 地址必须使用 http 或 https"); return; } setBusy(true); api.comfyui.testConnection({ baseUrl: address }).then(function () { setStatus("连接成功"); api.toast?.success("ComfyUI 连接成功"); }).catch(function (error) { setStatus("错误：" + comfyError(error)); }).finally(function () { setBusy(false); }); }}>测试连接</Button><Button disabled={busy} onClick={function () { try { var config = validate(); setBusy(true); props.plugin.saveConfig(config).then(function () { setStatus("配置已保存"); api.toast?.success("ComfyUI 生图配置已保存"); }).catch(function (error) { setStatus("错误：" + comfyError(error)); }).finally(function () { setBusy(false); }); } catch (error) { setStatus("错误：" + comfyError(error)); } }}>保存</Button></div>
      {status ? <div className="rounded-md border px-3 py-2 text-sm whitespace-pre-wrap">最近状态：{status}</div> : null}
    </CardContent>
  </Card>;
}

var ComfyUiImagePlugin = class ComfyUiImagePlugin {
  private systemApi: SystemApi | null = null;
  config() { return { name: "ComfyUI 自动生图", type: ["interfaceType", "applicationType"] }; }
  async onPluginLoad(systemApi: SystemApi) {
    this.systemApi = systemApi;
    await systemApi.storage.execute({ sql: "CREATE TABLE IF NOT EXISTS generation_jobs (job_key TEXT PRIMARY KEY, conversation_id INTEGER NOT NULL, message_id INTEGER NOT NULL, trigger_kind TEXT NOT NULL, status TEXT NOT NULL, generated_prompt TEXT, comfyui_prompt_id TEXT, error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)" });
    systemApi.hooks.unregister("chat.afterResponseCompleted");
    systemApi.hooks.register("chat.afterResponseCompleted", (raw) => {
      var context = raw as ComfyHookContext;
      void this.handleAutomatic(context).catch((error) => console.error("[comfyui-image-plugin] automatic generation failed", error));
    });
  }
  requireApi(): SystemApi { if (!this.systemApi) throw new Error("ComfyUI 插件尚未加载"); return this.systemApi; }
  showError(error: unknown) { this.requireApi().toast?.error(comfyError(error)); }
  async loadConfig(): Promise<ComfyConfig> { var api = this.requireApi(); return { modelId: (await api.getData(COMFY_MODEL_KEY)) || "", instruction: (await api.getData(COMFY_INSTRUCTION_KEY)) || "", baseUrl: (await api.getData(COMFY_URL_KEY)) || "" }; }
  async saveConfig(config: ComfyConfig) { var api = this.requireApi(); await Promise.all([api.setData(COMFY_MODEL_KEY, config.modelId), api.setData(COMFY_INSTRUCTION_KEY, config.instruction), api.setData(COMFY_URL_KEY, config.baseUrl)]); }
  async getAutoEnabled(conversationId: number) { return (await this.requireApi().getData(COMFY_AUTO_KEY, comfySessionId(conversationId))) === "true"; }
  async setAutoEnabled(conversationId: number, enabled: boolean) { await this.requireApi().setData(COMFY_AUTO_KEY, enabled ? "true" : "false", comfySessionId(conversationId)); this.requireApi().toast?.success(enabled ? "已开启当前会话自动生图" : "已关闭当前会话自动生图"); }
  private async validatedConfig(): Promise<ComfyConfig> { var config = await this.loadConfig(); if (!config.modelId || !config.instruction || !config.baseUrl) throw new Error("ComfyUI 生图配置不完整，请先在插件中心保存配置"); var models = await this.requireApi().listModels(); if (!models.some(function (model) { return comfyModelId(model) === config.modelId; })) throw new Error("已配置的提示词生成模型不存在，请在插件中心重新选择"); comfyRenderInstruction(config.instruction, "校验"); return config; }
  private async claimJob(key: string, conversationId: number, messageId: number, trigger: string): Promise<boolean> { var result = await this.requireApi().storage.execute({ sql: "INSERT OR IGNORE INTO generation_jobs (job_key, conversation_id, message_id, trigger_kind, status) VALUES (?, ?, ?, ?, 'queued')", params: [key, conversationId, messageId, trigger] }); return result.rowsAffected === 1; }
  private async updateJob(key: string, status: string, prompt?: string | null, promptId?: string | null, error?: string | null) { await this.requireApi().storage.execute({ sql: "UPDATE generation_jobs SET status = ?, generated_prompt = COALESCE(?, generated_prompt), comfyui_prompt_id = COALESCE(?, comfyui_prompt_id), error = ?, updated_at = CURRENT_TIMESTAMP WHERE job_key = ?", params: [status, prompt || null, promptId || null, error || null, key] }); }
  async latestJobStatus(): Promise<string> { var result = await this.requireApi().storage.query({ sql: "SELECT status, trigger_kind, message_id, comfyui_prompt_id, error, updated_at FROM generation_jobs ORDER BY updated_at DESC, rowid DESC LIMIT 1", maxRows: 1 }); if (!result.rows.length) return "暂无生图任务"; var row = result.rows[0]; return String(row[0]) + " · " + String(row[1]) + " · 消息 " + String(row[2]) + (row[3] ? " · prompt_id " + String(row[3]) : "") + (row[4] ? "\n" + String(row[4]) : "") + " · " + String(row[5]); }
  private async generate(conversationId: number, message: AippSystemApiMessage, key: string, trigger: string) { if (!(await this.claimJob(key, conversationId, message.id, trigger))) return; try { await this.updateJob(key, "building_prompt"); var config = await this.validatedConfig(); var requestPrompt = comfyRenderInstruction(config.instruction, String(message.content || "")); var result = await this.requireApi().runModelText({ modelId: config.modelId, prompt: requestPrompt }); var imagePrompt = String(result.content || "").trim(); if (!imagePrompt) throw new Error("提示词生成模型返回了空内容"); await this.updateJob(key, "generating", imagePrompt); var generated = await this.requireApi().comfyui.generateAndAttach({ baseUrl: config.baseUrl, workflow: comfyUiBuildWorkflow(imagePrompt), prompt: imagePrompt, conversationId: conversationId, messageId: message.id }); await this.updateJob(key, "completed", imagePrompt, generated.promptId, null); } catch (error) { await this.updateJob(key, "failed", null, null, comfyError(error)); throw error; } }
  private async handleAutomatic(context: ComfyHookContext) { var conversationId = Number(context.conversationId || 0); var messageId = Number(context.assistantMessageId || 0); if (!conversationId || !messageId || !(await this.getAutoEnabled(conversationId))) return; var conversation = await this.requireApi().conversations.getWithMessages(conversationId); var message = comfyFindMessage(conversation.messages || [], messageId); if (!message || !String(message.content || "").trim()) throw new Error("回复完成事件中的 assistantMessageId 无法定位有效 AI 回复"); await this.generate(conversationId, message, "auto:" + messageId, "auto"); }
  async generateLatest(conversationId: number) { var conversation = await this.requireApi().conversations.getWithMessages(conversationId); var message = comfyLatestAssistantMessage(conversation.messages || []); if (!message) throw new Error("当前会话没有可用于生图的 AI 回复"); await this.generateMessage(conversationId, message.id, message.content); }
  async generateMessage(conversationId: number, messageId: number, messageContent?: string) { var conversation = await this.requireApi().conversations.getWithMessages(conversationId); var message = comfyFindMessage(conversation.messages || [], messageId); if (!message) throw new Error("指定消息不是当前会话中的有效 AI 回复"); if (messageContent !== undefined && String(message.content || "") !== String(messageContent || "")) { message = { ...message, content: messageContent } as AippSystemApiMessage; } if (!String(message.content || "").trim()) throw new Error("指定的 AI 回复为空，无法生图"); var key = "manual:" + message.id + ":" + Date.now() + ":" + Math.random().toString(36).slice(2); await this.generate(conversationId, message, key, "manual"); }
  renderView(viewId: string) { return viewId === "comfyui-image-config" ? <ComfyConfigPanel plugin={this} /> : null; }
  renderAction(actionId: string, context?: Record<string, unknown>) { var actionContext = (context || {}) as ComfyActionContext; if (actionId === "comfyui-toggle-auto") return <ComfyToggleAction plugin={this} context={actionContext} />; if (actionId === "comfyui-generate-now" && (actionContext.messageType === "assistant" || actionContext.messageType === "response")) return <ComfyManualAction plugin={this} context={actionContext} />; return null; }
};

(window as any)["comfyui-image-plugin"] = ComfyUiImagePlugin;
(window as any).ComfyUiImagePlugin = ComfyUiImagePlugin;
