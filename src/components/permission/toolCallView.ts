/** 把各通道权限请求里的工具调用整理成「工具 / 用途 / 参数」三段，供审批卡展示。 */
export interface ToolCallView {
    /** 实际执行的工具名（已去掉 MCP 前缀与动态调用包装）。 */
    name: string;
    /** 工具的中文名，未收录时为空。 */
    label?: string;
    /** 工具所在的服务（AIPP 内部工具不显示）。 */
    server?: string;
    /** 助手说明的本次调用目的。 */
    purpose?: string;
    args: [string, unknown][];
}

export function isRecord(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === "object" && !Array.isArray(value);
}

export const historySearchTitle = "搜索管家聊天记录（含归档会话）";

const toolLabels = new Map(Object.entries({
    execute_bash: "运行命令", get_bash_output: "查看命令输出", kill_bash: "结束命令",
    read_file: "读取文件", write_file: "写入文件", edit_file: "编辑文件", list_directory: "列出目录",
    search_web: "网络搜索", search_http: "网络搜索", fetch_url: "抓取网页",
    load_mcp_server: "加载 MCP 服务", load_mcp_tool: "加载 MCP 工具", load_skill: "加载 Skill",
    search_bot_history: historySearchTitle, search_own_history: "搜索自己的聊天记录",
    read_bot_message_context: "读取消息前后文", read_group_history: "读取本群聊天记录",
    read_work_context: "查看工作上下文", list_assistants: "查询助手列表",
    get_assistant_details: "查看助手资料", get_assistant_exchange: "查看助手协作记录",
    message_bot: "给助手发消息", schedule_task: "定时任务",
    butler_memory: "管家长期记忆", superadmin_execute: "执行管家管理操作", superadmin_batch: "批量执行管家管理操作",
    // Claude Code 内置工具
    Bash: "运行命令", Read: "读取文件", Write: "写入文件", Edit: "编辑文件", MultiEdit: "编辑文件",
    Glob: "查找文件", Grep: "搜索文件内容", WebFetch: "抓取网页", WebSearch: "网络搜索", NotebookEdit: "编辑笔记本",
}));

const argumentLabels = new Map(Object.entries({
    command: "命令", CommandLine: "命令", description: "说明", timeout: "超时（毫秒）", run_in_background: "后台运行",
    cwd: "工作目录", Cwd: "工作目录", file_path: "路径", path: "路径", content: "内容",
    old_string: "原内容", new_string: "新内容", replace_all: "全部替换", pattern: "匹配模式", recursive: "包含子目录",
    offset: "起始行", limit: "数量上限", bash_id: "命令编号", filter: "过滤", url: "网址", prompt: "要求",
    query: "搜索词", bot_id: "助手编号", before_message_id: "早于消息编号", assistant_id: "助手编号",
    message_id: "消息编号", group_id: "群编号", action: "操作", message: "消息",
    server_name: "服务", tool_name: "工具",
}));

/** 这些参数按代码展示（等宽、保留换行）。 */
const codeArguments = new Set(["command", "CommandLine", "content", "old_string", "new_string", "code", "script"]);
const commandTools = new Set(["execute_bash", "Bash"]);

export function argumentLabel(key: string, overrides?: Map<string, string>) {
    return overrides?.get(key) || argumentLabels.get(key) || key;
}

export function isCodeArgument(key: string, value: string) {
    return codeArguments.has(key) || value.includes("\n") || value.length > 160;
}

/** 去掉通道附加的前缀：Antigravity 的 `Run mcp_aipp_aipp_t62_x`、Claude 的 `mcp__aipp__aipp_t62_x`、AIPP 的 `aipp_t62_x`。 */
function splitToolName(raw: string): { name: string; server?: string } {
    const name = raw.trim().replace(/^Run (?=[\w.-]+$)/, "");
    const aipp = /^(?:mcp_aipp_|mcp__aipp__)?aipp_t\d+_([\w-]+)$/.exec(name);
    if (aipp) return { name: aipp[1] };
    const claudeMcp = /^mcp__(.+?)__(.+)$/.exec(name);
    if (claudeMcp) return { name: claudeMcp[2], server: claudeMcp[1] };
    return { name };
}

export function describeToolCall(rawName: string, input: unknown, server?: string): ToolCallView {
    let { name, server: parsedServer } = splitToolName(rawName);
    let args: Record<string, unknown> = isRecord(input) ? { ...input } : {};
    let toolServer = server ?? parsedServer;
    // 动态 MCP 入口只是包装，展示里面真正要调用的工具和参数。
    if (name === "call_mcp_tool" && typeof args.tool_name === "string" && args.tool_name.trim()) {
        const { server_name, tool_name, parameters, ...rest } = args;
        name = splitToolName(tool_name as string).name;
        toolServer = typeof server_name === "string" && server_name.trim() ? server_name : undefined;
        args = { ...rest, ...(isRecord(parameters) ? parameters : parameters === undefined ? {} : { parameters }) };
    }
    let purpose: string | undefined;
    const take = (key: string) => {
        const value = args[key];
        if (typeof value !== "string" || !value.trim()) return;
        delete args[key];
        purpose ??= value.trim();
        if (purpose !== value.trim()) args[key] = value;
    };
    take("toolSummary");
    if (commandTools.has(name)) take("description");
    const entries = Object.entries(args).filter(([, value]) => value !== undefined && value !== null && value !== "");
    // 代码类参数（命令、内容）放在最前，其余保持原顺序。
    entries.sort(([a], [b]) => Number(codeArguments.has(b)) - Number(codeArguments.has(a)));
    return { name, label: toolLabels.get(name), server: toolServer, purpose, args: entries };
}

export function parseJson(text: string | undefined): unknown {
    if (text == null) return undefined;
    try { return JSON.parse(text); } catch { return text; }
}
