var ComfyReact = (window as any).React as typeof import("react");

type ComfyActionContext = { conversationId?: number | null; messageId?: number | null; messageType?: string; messageContent?: string };
type KieModel = "qwen3/pro-text-to-image" | "z-image";
type ImageConfig = { provider: "comfyui" | "kie"; modelId: string; instruction: string; baseUrl: string; apiKey: string; kieModel: KieModel; kieAspectRatio: string; kieResolution: string; kieImageSize: string; kieOutputFormat: string; kiePromptExtend: boolean; kieNsfwChecker: boolean; kieNegativePrompt: string; kieSeed: string; promptNodeId: string; promptInputName: string };
type ComfyHookContext = { conversationId?: number; assistantMessageId?: number | null };
type ComfyPluginInstance = {
  requireApi(): SystemApi;
  showError(error: unknown): void;
  loadConfig(): Promise<ImageConfig>;
  saveConfig(config: ImageConfig): Promise<void>;
  latestJobStatus(): Promise<string>;
  getAutoEnabled(conversationId: number): Promise<boolean>;
  setAutoEnabled(conversationId: number, enabled: boolean): Promise<void>;
  generateLatest(conversationId: number): Promise<void>;
  generateMessage(conversationId: number, messageId: number, messageContent?: string): Promise<void>;
};

var IMAGE_PROVIDER_KEY = "image_provider";
var COMFY_MODEL_KEY = "prompt_model_id";
var COMFY_INSTRUCTION_KEY = "prompt_instruction";
var COMFY_URL_KEY = "comfyui_base_url";
var KIE_URL_KEY = "kie_base_url";
var KIE_API_KEY = "kie_api_key";
var KIE_MODEL_KEY = "kie_model";
var COMFY_PROMPT_NODE_KEY = "comfyui_prompt_node_id";
var COMFY_PROMPT_INPUT_KEY = "comfyui_prompt_input_name";
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
  var [provider, setProvider] = ComfyReact.useState<"comfyui" | "kie">("comfyui");
  var [apiKey, setApiKey] = ComfyReact.useState("");
  var [kieModel, setKieModel] = ComfyReact.useState<KieModel>("qwen3/pro-text-to-image");
  var [kieAspectRatio, setKieAspectRatio] = ComfyReact.useState("1:1");
  var [kieResolution, setKieResolution] = ComfyReact.useState("1K");
  var [kieImageSize, setKieImageSize] = ComfyReact.useState("1:1");
  var [kieOutputFormat, setKieOutputFormat] = ComfyReact.useState("png");
  var [kiePromptExtend, setKiePromptExtend] = ComfyReact.useState(true);
  var [kieNsfwChecker, setKieNsfwChecker] = ComfyReact.useState(false);
  var [kieNegativePrompt, setKieNegativePrompt] = ComfyReact.useState("");
  var [kieSeed, setKieSeed] = ComfyReact.useState("");
  var [promptNodeId, setPromptNodeId] = ComfyReact.useState("57:27");
  var [promptInputName, setPromptInputName] = ComfyReact.useState("text");
  var [status, setStatus] = ComfyReact.useState("");
  var [busy, setBusy] = ComfyReact.useState(false);
  ComfyReact.useEffect(function () {
    Promise.all([api.listModels(), props.plugin.loadConfig(), props.plugin.latestJobStatus()]).then(function (values) {
      setModels(values[0]); setProvider(values[1].provider); setModelId(values[1].modelId); setInstruction(values[1].instruction || COMFY_DEFAULT_INSTRUCTION); setBaseUrl(values[1].baseUrl); setApiKey(values[1].apiKey); setKieModel(values[1].kieModel); setKieAspectRatio(values[1].kieAspectRatio); setKieResolution(values[1].kieResolution); setKieImageSize(values[1].kieImageSize); setKieOutputFormat(values[1].kieOutputFormat); setKiePromptExtend(values[1].kiePromptExtend); setKieNsfwChecker(values[1].kieNsfwChecker); setKieNegativePrompt(values[1].kieNegativePrompt); setKieSeed(values[1].kieSeed); setPromptNodeId(values[1].promptNodeId || "57:27"); setPromptInputName(values[1].promptInputName || "text"); setStatus(values[2]);
      if (values[1].modelId && !values[0].some(function (model) { return comfyModelId(model) === values[1].modelId; })) setStatus("错误：已配置的提示词模型不存在，请重新选择并保存。");
    }).catch(function (error) { setStatus("错误：" + comfyError(error)); });
  }, []);
  ComfyReact.useEffect(function () {
    var timer = setInterval(function () {
      props.plugin.latestJobStatus().then(setStatus).catch(function () {});
    }, 2000);
    return function () { clearInterval(timer); };
  }, []);
  function validate(): ImageConfig {
    var config = { provider: provider, modelId: modelId.trim(), instruction: instruction.trim(), baseUrl: baseUrl.trim(), apiKey: apiKey.trim(), kieModel: kieModel, kieAspectRatio: kieAspectRatio, kieResolution: kieResolution, kieImageSize: kieImageSize, kieOutputFormat: kieOutputFormat, kiePromptExtend: kiePromptExtend, kieNsfwChecker: kieNsfwChecker, kieNegativePrompt: kieNegativePrompt.trim(), kieSeed: kieSeed.trim(), promptNodeId: promptNodeId.trim(), promptInputName: promptInputName.trim() };
    if (!config.modelId) throw new Error("请选择提示词生成模型");
    if (!models.some(function (model) { return comfyModelId(model) === config.modelId; })) throw new Error("提示词生成模型不存在，请重新选择");
    if (!config.instruction) throw new Error("请填写提示词生成指令");
    if (!config.instruction.includes("{{assistant_reply}}")) throw new Error("提示词生成指令必须包含 {{assistant_reply}}");
    if (!/^https?:\/\//i.test(config.baseUrl)) throw new Error("地址必须使用 http 或 https");
    if (config.provider === "kie" && !config.apiKey) throw new Error("请填写 Kie API Key");
    if (config.provider === "comfyui" && (!config.promptNodeId || !config.promptInputName)) throw new Error("请填写 ComfyUI Prompt 节点和参数");
    return config;
  }
  return <Card className="w-full">
    <CardHeader><CardTitle>AI 图片生成</CardTitle><CardDescription>AI 回复完成后生成图片，并附加在同一条回复底部。</CardDescription></CardHeader>
    <CardContent className="space-y-4">
      <div className="space-y-2"><label className="text-sm font-medium">提示词生成模型</label>
        {Select && SelectTrigger && SelectValue && SelectContent && SelectItem ? <Select value={modelId} onValueChange={setModelId}><SelectTrigger><SelectValue placeholder="选择模型" /></SelectTrigger><SelectContent>{models.map(function (model) { return <SelectItem value={comfyModelId(model)}>{comfyModelLabel(model)}</SelectItem>; })}</SelectContent></Select> : <select className="w-full" value={modelId} onChange={function (event) { setModelId(event.target.value); }}><option value="">选择模型</option>{models.map(function (model) { return <option key={comfyModelId(model)} value={comfyModelId(model)}>{comfyModelLabel(model)}</option>; })}</select>}
      </div>
      <div className="space-y-2"><label className="text-sm font-medium">提示词生成指令</label><Textarea value={instruction} rows={7} onChange={function (event: any) { setInstruction(event.target.value); }} /><p className="text-xs text-muted-foreground">必须包含 {"{{assistant_reply}}"}，运行时替换为目标 AI 回复。</p></div>
      <div className="space-y-2"><label className="text-sm font-medium">图片生成方式</label><select className="w-full" value={provider} onChange={function (event: any) { var next = event.target.value; setProvider(next); setBaseUrl(next === "kie" ? "https://api.kie.ai" : "http://127.0.0.1:8188"); }}><option value="comfyui">ComfyUI</option><option value="kie">Kie</option></select></div>
      <div className="space-y-2"><label className="text-sm font-medium">{provider === "kie" ? "Kie 地址" : "ComfyUI 地址"}</label><Input value={baseUrl} onChange={function (event: any) { setBaseUrl(event.target.value); }} placeholder={provider === "kie" ? "https://api.kie.ai" : "http://127.0.0.1:8188"} /></div>
      {provider === "kie" ? <><div className="space-y-2"><label className="text-sm font-medium">Kie API Key</label><Input type="password" value={apiKey} onChange={function (event: any) { setApiKey(event.target.value); }} /></div><div className="space-y-2"><label className="text-sm font-medium">Kie 模型</label><select className="w-full" value={kieModel} onChange={function (event: any) { setKieModel(event.target.value); }}><option value="qwen3/pro-text-to-image">Qwen3 Pro Text to Image</option><option value="z-image">Z-Image</option></select></div>{kieModel === "z-image" ? <div className="space-y-2"><label className="text-sm font-medium">图片比例</label><select className="w-full" value={kieAspectRatio} onChange={function (event: any) { setKieAspectRatio(event.target.value); }}><option>1:1</option><option>4:3</option><option>3:4</option><option>16:9</option><option>9:16</option></select></div> : <div className="grid grid-cols-2 gap-3"><div><label className="text-sm font-medium">分辨率</label><select className="w-full" value={kieResolution} onChange={function (event: any) { setKieResolution(event.target.value); }}><option>1K</option><option>2K</option></select></div><div><label className="text-sm font-medium">图片尺寸</label><select className="w-full" value={kieImageSize} onChange={function (event: any) { setKieImageSize(event.target.value); }}><option>1:1</option><option>3:2</option><option>2:3</option><option>4:3</option><option>3:4</option><option>16:9</option><option>9:16</option><option>21:9</option></select></div></div>}<div className="grid grid-cols-2 gap-3"><div><label className="text-sm font-medium">负面提示词</label><Input value={kieNegativePrompt} onChange={function (event: any) { setKieNegativePrompt(event.target.value); }} /></div><div><label className="text-sm font-medium">Seed（可选）</label><Input value={kieSeed} onChange={function (event: any) { setKieSeed(event.target.value); }} /></div></div><label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={kiePromptExtend} onChange={function (event: any) { setKiePromptExtend(event.target.checked); }} />智能扩写提示词（Qwen3）</label><label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={kieNsfwChecker} onChange={function (event: any) { setKieNsfwChecker(event.target.checked); }} />启用 NSFW 检查</label></> : <div className="grid grid-cols-2 gap-3"><div className="space-y-2"><label className="text-sm font-medium">Prompt 节点 ID</label><Input value={promptNodeId} onChange={function (event: any) { setPromptNodeId(event.target.value); }} placeholder="57:27" /></div><div className="space-y-2"><label className="text-sm font-medium">Prompt 参数名</label><Input value={promptInputName} onChange={function (event: any) { setPromptInputName(event.target.value); }} placeholder="text" /></div></div>}
      <div className="flex gap-2"><Button disabled={busy} onClick={function () { var address = baseUrl.trim(); if (!/^https?:\/\//i.test(address)) { setStatus("错误：地址必须使用 http 或 https"); return; } setBusy(true); api.imageGeneration.testConnection(provider === "kie" ? { provider: "kie", baseUrl: address, apiKey: apiKey.trim() } : { provider: "comfyui", baseUrl: address }).then(function () { var message = provider === "kie" ? "Kie 配置格式有效" : "ComfyUI 连接成功"; setStatus(message); api.toast?.success(message); }).catch(function (error) { setStatus("错误：" + comfyError(error)); }).finally(function () { setBusy(false); }); }}>{provider === "kie" ? "验证配置" : "测试连接"}</Button><Button disabled={busy} onClick={function () { try { var config = validate(); setBusy(true); props.plugin.saveConfig(config).then(function () { setStatus("配置已保存"); api.toast?.success("图片生成配置已保存"); }).catch(function (error) { setStatus("错误：" + comfyError(error)); }).finally(function () { setBusy(false); }); } catch (error) { setStatus("错误：" + comfyError(error)); } }}>保存</Button></div>
      {status ? <div className="rounded-md border px-3 py-2 text-sm whitespace-pre-wrap">最近状态：{status}</div> : null}
    </CardContent>
  </Card>;
}

var ComfyUiImagePlugin = class ComfyUiImagePlugin {
  private systemApi: SystemApi | null = null;
  config() { return { name: "AI 图片生成", type: ["interfaceType", "applicationType"] }; }
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
  async loadConfig(): Promise<ImageConfig> { var api = this.requireApi(); var provider = ((await api.getData(IMAGE_PROVIDER_KEY)) || "comfyui") as "comfyui" | "kie"; if (provider !== "comfyui" && provider !== "kie") throw new Error("暂不支持图片生成供应商: " + provider); var storedModel = (await api.getData(KIE_MODEL_KEY)) || "qwen3/pro-text-to-image"; if (storedModel === "qwen3-pro") storedModel = "qwen3/pro-text-to-image"; return { provider: provider, modelId: (await api.getData(COMFY_MODEL_KEY)) || "", instruction: (await api.getData(COMFY_INSTRUCTION_KEY)) || "", baseUrl: (await api.getData(provider === "kie" ? KIE_URL_KEY : COMFY_URL_KEY)) || (provider === "kie" ? "https://api.kie.ai" : "http://127.0.0.1:8188"), apiKey: (await api.getData(KIE_API_KEY)) || "", kieModel: storedModel as KieModel, kieAspectRatio: (await api.getData("kie_aspect_ratio")) || "1:1", kieResolution: (await api.getData("kie_resolution")) || "1K", kieImageSize: (await api.getData("kie_image_size")) || "1:1", kieOutputFormat: (await api.getData("kie_output_format")) || "png", kiePromptExtend: (await api.getData("kie_prompt_extend")) !== "false", kieNsfwChecker: (await api.getData("kie_nsfw_checker")) === "true", kieNegativePrompt: (await api.getData("kie_negative_prompt")) || "", kieSeed: (await api.getData("kie_seed")) || "", promptNodeId: (await api.getData(COMFY_PROMPT_NODE_KEY)) || "57:27", promptInputName: (await api.getData(COMFY_PROMPT_INPUT_KEY)) || "text" }; }
  async saveConfig(config: ImageConfig) { var api = this.requireApi(); await Promise.all([api.setData(IMAGE_PROVIDER_KEY, config.provider), api.setData(COMFY_MODEL_KEY, config.modelId), api.setData(COMFY_INSTRUCTION_KEY, config.instruction), api.setData(config.provider === "kie" ? KIE_URL_KEY : COMFY_URL_KEY, config.baseUrl), api.setData(KIE_API_KEY, config.apiKey), api.setData(KIE_MODEL_KEY, config.kieModel), api.setData("kie_aspect_ratio", config.kieAspectRatio), api.setData("kie_resolution", config.kieResolution), api.setData("kie_image_size", config.kieImageSize), api.setData("kie_output_format", config.kieOutputFormat), api.setData("kie_prompt_extend", String(config.kiePromptExtend)), api.setData("kie_nsfw_checker", String(config.kieNsfwChecker)), api.setData("kie_negative_prompt", config.kieNegativePrompt), api.setData("kie_seed", config.kieSeed), api.setData(COMFY_PROMPT_NODE_KEY, config.promptNodeId), api.setData(COMFY_PROMPT_INPUT_KEY, config.promptInputName)]); }
  async getAutoEnabled(conversationId: number) { return (await this.requireApi().getData(COMFY_AUTO_KEY, comfySessionId(conversationId))) === "true"; }
  async setAutoEnabled(conversationId: number, enabled: boolean) { await this.requireApi().setData(COMFY_AUTO_KEY, enabled ? "true" : "false", comfySessionId(conversationId)); this.requireApi().toast?.success(enabled ? "已开启当前会话自动生图" : "已关闭当前会话自动生图"); }
  private async validatedConfig(): Promise<ImageConfig> { var config = await this.loadConfig(); if (!config.modelId || !config.instruction || !config.baseUrl) throw new Error("图片生成配置不完整，请先在插件中心保存配置"); if (config.provider === "kie" && !config.apiKey) throw new Error("Kie 生图配置缺少 API Key"); if (config.provider === "comfyui" && (!config.promptNodeId || !config.promptInputName)) throw new Error("ComfyUI 生图配置缺少 Prompt 节点或参数"); var models = await this.requireApi().listModels(); if (!models.some(function (model) { return comfyModelId(model) === config.modelId; })) throw new Error("已配置的提示词模型不存在，请在插件中心重新选择"); comfyRenderInstruction(config.instruction, "校验"); return config; }
  private async claimJob(key: string, conversationId: number, messageId: number, trigger: string): Promise<boolean> { var result = await this.requireApi().storage.execute({ sql: "INSERT OR IGNORE INTO generation_jobs (job_key, conversation_id, message_id, trigger_kind, status) VALUES (?, ?, ?, ?, 'queued')", params: [key, conversationId, messageId, trigger] }); return result.rowsAffected === 1; }
  private async updateJob(key: string, status: string, prompt?: string | null, promptId?: string | null, error?: string | null) { await this.requireApi().storage.execute({ sql: "UPDATE generation_jobs SET status = ?, generated_prompt = COALESCE(?, generated_prompt), comfyui_prompt_id = COALESCE(?, comfyui_prompt_id), error = ?, updated_at = CURRENT_TIMESTAMP WHERE job_key = ?", params: [status, prompt || null, promptId || null, error || null, key] }); }
  async latestJobStatus(): Promise<string> { var result = await this.requireApi().storage.query({ sql: "SELECT status, trigger_kind, message_id, comfyui_prompt_id, error, updated_at FROM generation_jobs ORDER BY updated_at DESC, rowid DESC LIMIT 1", maxRows: 1 }); if (!result.rows.length) return "暂无生图任务"; var row = result.rows[0]; return String(row[0]) + " · " + String(row[1]) + " · 消息 " + String(row[2]) + (row[3] ? " · prompt_id " + String(row[3]) : "") + (row[4] ? "\n" + String(row[4]) : "") + " · " + String(row[5]); }
  private async generate(conversationId: number, message: AippSystemApiMessage, key: string, trigger: string) { if (!(await this.claimJob(key, conversationId, message.id, trigger))) return; try { await this.updateJob(key, "building_prompt"); var config = await this.validatedConfig(); var requestPrompt = comfyRenderInstruction(config.instruction, String(message.content || "")); var result = await this.requireApi().runModelText({ modelId: config.modelId, prompt: requestPrompt }); var imagePrompt = String(result.content || "").trim(); if (!imagePrompt) throw new Error("提示词生成模型返回了空内容"); await this.updateJob(key, "generating", imagePrompt); var kieInput: Record<string, unknown> = config.kieModel === "z-image" ? { prompt: imagePrompt, aspect_ratio: config.kieAspectRatio, nsfw_checker: config.kieNsfwChecker } : { prompt: imagePrompt, resolution: config.kieResolution, image_size: config.kieImageSize, output_format: config.kieOutputFormat, prompt_extend: config.kiePromptExtend, nsfw_checker: config.kieNsfwChecker }; if (config.kieNegativePrompt) kieInput.negative_prompt = config.kieNegativePrompt; if (config.kieSeed) { var seed = Number(config.kieSeed); if (!Number.isInteger(seed) || seed < 0) throw new Error("Kie Seed 必须是非负整数"); kieInput.seed = seed; } var generated = config.provider === "kie" ? await this.requireApi().imageGeneration.executeTask({ create: { method: "POST", url: config.baseUrl + "/api/v1/jobs/createTask", headers: { Authorization: "Bearer " + config.apiKey, "Content-Type": "application/json" }, body: { model: config.kieModel, input: kieInput } }, poll: { request: { method: "GET", url: config.baseUrl + "/api/v1/jobs/recordInfo", headers: { Authorization: "Bearer " + config.apiKey }, query: { taskId: "$task_id" } }, taskIdPath: "/data/taskId", statusPath: "/data/state", failureValues: ["fail", "failed", "error"], resultPath: "/data/resultJson", resultUrlsPath: "/resultUrls", parseJsonString: true, intervalMs: 1000, timeoutMs: 180000 }, conversationId: conversationId, messageId: message.id }) : await this.requireApi().imageGeneration.generateAndAttach({ provider: "comfyui", baseUrl: config.baseUrl, workflow: comfyUiBuildWorkflow(imagePrompt, config.promptNodeId, config.promptInputName), prompt: imagePrompt, promptNodeId: config.promptNodeId, promptInputName: config.promptInputName, conversationId: conversationId, messageId: message.id }); var generatedId = "promptId" in generated ? generated.promptId : generated.taskId; await this.updateJob(key, "completed", imagePrompt, generatedId || null, null); } catch (error) { await this.updateJob(key, "failed", null, null, comfyError(error)); throw error; } }
  private async handleAutomatic(context: ComfyHookContext) { var conversationId = Number(context.conversationId || 0); var messageId = Number(context.assistantMessageId || 0); if (!conversationId || !messageId || !(await this.getAutoEnabled(conversationId))) return; var conversation = await this.requireApi().conversations.getWithMessages(conversationId); var message = comfyFindMessage(conversation.messages || [], messageId); if (!message || !String(message.content || "").trim()) throw new Error("回复完成事件中的 assistantMessageId 无法定位有效 AI 回复"); await this.generate(conversationId, message, "auto:" + messageId, "auto"); }
  async generateLatest(conversationId: number) { var conversation = await this.requireApi().conversations.getWithMessages(conversationId); var message = comfyLatestAssistantMessage(conversation.messages || []); if (!message) throw new Error("当前会话没有可用于生图的 AI 回复"); await this.generateMessage(conversationId, message.id, message.content); }
  async generateMessage(conversationId: number, messageId: number, messageContent?: string) { var conversation = await this.requireApi().conversations.getWithMessages(conversationId); var message = comfyFindMessage(conversation.messages || [], messageId); if (!message) throw new Error("指定消息不是当前会话中的有效 AI 回复"); if (messageContent !== undefined && String(message.content || "") !== String(messageContent || "")) { message = { ...message, content: messageContent } as AippSystemApiMessage; } if (!String(message.content || "").trim()) throw new Error("指定的 AI 回复为空，无法生图"); var key = "manual:" + message.id + ":" + Date.now() + ":" + Math.random().toString(36).slice(2); await this.generate(conversationId, message, key, "manual"); }
  renderView(viewId: string) { return viewId === "comfyui-image-config" ? <ComfyConfigPanel plugin={this} /> : null; }
  renderAction(actionId: string, context?: Record<string, unknown>) { var actionContext = (context || {}) as ComfyActionContext; if (actionId === "comfyui-toggle-auto") return <ComfyToggleAction plugin={this} context={actionContext} />; if (actionId === "comfyui-generate-now" && (actionContext.messageType === "assistant" || actionContext.messageType === "response")) return <ComfyManualAction plugin={this} context={actionContext} />; return null; }
};

(window as any)["comfyui-image-plugin"] = ComfyUiImagePlugin;
(window as any).ComfyUiImagePlugin = ComfyUiImagePlugin;
