import { PinyinFilter } from "@/utils/pinyinFilter";
import type { SettingsMenuId } from "./settingsNavigation";

export interface SettingsSearchItem {
    id: string;
    title: string;
    section: string;
    description: string;
    keywords: string[];
    menu: SettingsMenuId;
    subNav?: string;
    desktopOnly?: boolean;
    mobileOnly?: boolean;
}

export const SETTINGS_SEARCH_INDEX: SettingsSearchItem[] = [
    { id: "device-connections", menu: "device-connections", title: "设备互联", section: "能力与扩展",
        description: "在局域网与手机配对，管理设备授权", keywords: ["手机", "配对", "电脑", "二维码"], desktopOnly: true },
    // 核心配置
    {
        id: "providers",
        title: "模型与提供商",
        section: "核心配置",
        description: "管理云端 API 模型提供商与本地 CLI 智能体驱动（OpenAI / Claude / DeepSeek / Codex / Antigravity 等）",
        keywords: ["provider", "llm", "api", "key", "openai", "anthropic", "deepseek", "copilot", "ollama", "codex", "claude", "acp", "pi", "cursor", "opencode", "kiro", "antigravity", "模型", "提供商", "密钥"],
        menu: "llm-provider-config",
    },
    {
        id: "assistants",
        title: "助手管理",
        section: "核心配置",
        description: "配置对话助手与智能体助手的提示词、模型、MCP、Skills、工作区、权限与记忆",
        keywords: ["assistant", "prompt", "agent", "temperature", "top_p", "max_tokens", "助手", "提示词", "角色", "参数", "工作区", "记忆"],
        menu: "assistant-config",
    },
    {
        id: "butler-main",
        title: "总管家模式与模型",
        section: "总管家",
        description: "开启总管家工作台、配置显示名称与形象、主模型、辅助模型与群聊路由模型",
        keywords: ["butler", "chief", "管家", "总管家", "头像", "形象", "群聊", "路由", "决策模型", "辅助模型", "引导"],
        menu: "butler-config",
        subNav: "basic#butler-basic",
    },
    {
        id: "butler-workspaces",
        title: "总管家可信工作区",
        section: "总管家",
        description: "设置主工作区、额外可信目录及信任任何工作区开关",
        keywords: ["workspace", "folder", "trust", "工作区", "目录", "信任", "文件权限"],
        menu: "butler-config",
        subNav: "workspaces#butler-workspaces",
        desktopOnly: true,
    },
    {
        id: "butler-proactive",
        title: "主动整理与关怀",
        section: "总管家",
        description: "配置后台自主整理范围、偏好指令、提醒节律、免打扰时段与每日复盘",
        keywords: ["proactive", "care", "digest", "quiet", "主动整理", "关怀", "免打扰", "复盘", "巡检", "节律"],
        menu: "butler-config",
        subNav: "proactive#butler-proactive",
    },
    {
        id: "butler-feishu",
        title: "飞书机器人接入",
        section: "总管家",
        description: "配置飞书 / Lark 应用凭证、单聊群聊规则与允许列表",
        keywords: ["feishu", "lark", "bot", "飞书", "机器人", "webhook", "open_id", "chat_id"],
        menu: "butler-config",
        subNav: "feishu#butler-feishu",
    },
    {
        id: "butler-controls",
        title: "管家协作与任务熔断开关",
        section: "总管家",
        description: "独立启停新建独立任务、发起成员协作与继续已有工作",
        keywords: ["dispatch", "peer", "collaboration", "continuation", "任务", "协作", "续接", "熔断"],
        menu: "butler-config",
        subNav: "advanced#butler-controls",
    },
    {
        id: "butler-memory",
        title: "总管家与全局记忆",
        section: "总管家",
        description: "查看与管理总管家私有记忆及全局长期记忆",
        keywords: ["memory", "long term", "记忆", "长期记忆", "全局记忆"],
        menu: "butler-config",
        subNav: "memory#butler-memory",
    },

    // 能力与扩展
    {
        id: "apps-tools",
        title: "应用与工具（MCP）",
        section: "能力与扩展",
        description: "连接内置工具、外部服务应用、自定义 MCP 与能力包",
        keywords: ["mcp", "apps", "tools", "search", "office", "应用", "工具", "搜索", "能力包", "连接"],
        menu: "mcp-config",
    },
    {
        id: "skills",
        title: "Skills 技能管理",
        section: "能力与扩展",
        description: "扫描、查看与管理本地及插件提供的 Skills 技能",
        keywords: ["skills", "skill", "技能", "指令"],
        menu: "skills-config",
    },
    {
        id: "plugins",
        title: "插件中心",
        section: "能力与扩展",
        description: "浏览官方插件市场，安装、启停与管理 AIPP 扩展插件（主题/形象/助手类型等）",
        keywords: ["plugin", "market", "theme", "avatar", "插件", "市场", "扩展", "形象包"],
        menu: "plugins-config",
    },
    {
        id: "ai-default-models",
        title: "默认轻量模型与决策审核模型",
        section: "AI 与自动化",
        description: "设置全局默认轻量模型和默认决策/审核模型，供各项辅助任务自动继承",
        keywords: ["default", "light", "decision", "model", "默认模型", "轻量模型", "决策模型", "小模型", "继承"],
        menu: "ai-automation-config",
        subNav: "#default-models",
    },
    {
        id: "ai-title-summary",
        title: "对话标题自动生成",
        section: "AI 与自动化",
        description: "对话开始时自动总结并生成标题，支持自定义 Prompt 与截断长度",
        keywords: ["title", "summary", "标题", "总结", "自动命名"],
        menu: "ai-automation-config",
        subNav: "#title-summary",
    },
    {
        id: "ai-form-autofill",
        title: "表单自动填写",
        section: "AI 与自动化",
        description: "根据对话内容自动填写交互表单字段",
        keywords: ["form", "autofill", "表单", "自动填写"],
        menu: "ai-automation-config",
        subNav: "#form-autofill",
    },
    {
        id: "ai-conversation-summary",
        title: "空闲对话自动总结",
        section: "AI 与自动化",
        description: "对话空闲后后台自动生成摘要，便于检索与回顾",
        keywords: ["conversation", "summary", "对话总结", "摘要", "后台总结"],
        menu: "ai-automation-config",
        subNav: "#conversation-summary",
    },
    {
        id: "ai-auto-review",
        title: "工具调用自动审核",
        section: "AI 与自动化",
        description: "配置自动审核模型、决策放行安全阈值、Schema/Prompt 输出模式检测",
        keywords: ["review", "audit", "approval", "jev", "threshold", "审核", "自动审核", "审批", "阈值", "安全"],
        menu: "ai-automation-config",
        subNav: "#auto-review-model",
    },
    {
        id: "ai-context-compaction",
        title: "上下文自动压缩",
        section: "AI 与自动化",
        description: "为长对话自动压缩较早历史，配置上下文窗口、触发比例与尾部保留比例",
        keywords: ["compact", "compaction", "context", "token", "压缩", "上下文", "窗口", "截断"],
        menu: "ai-automation-config",
        subNav: "#context-compaction",
    },
    {
        id: "ai-dynamic-mcp",
        title: "MCP 动态加载与摘要",
        section: "AI 与自动化",
        description: "开启 MCP 目录摘要按需加载模式，管理摘要模型与后台进度",
        keywords: ["dynamic", "mcp", "summarizer", "动态加载", "按需加载", "mcp摘要"],
        menu: "ai-automation-config",
        subNav: "#dynamic-mcp",
    },
    {
        id: "ai-tool-error-continue",
        title: "工具错误后继续对话",
        section: "AI 与自动化",
        description: "自动运行的工具出错时携带错误信息让模型继续处理",
        keywords: ["tool", "error", "continue", "retry", "工具错误", "失败继续"],
        menu: "ai-automation-config",
        subNav: "#tool-error-continue",
    },
    {
        id: "mobile-tasks",
        title: "本机任务",
        section: "能力与扩展",
        description: "管理移动端的本机定时任务",
        keywords: ["task", "schedule", "任务", "定时"],
        menu: "mobile-tasks",
        mobileOnly: true,
    },

    // 隐私与数据
    {
        id: "vault-items",
        title: "凭据保险箱",
        section: "安全与凭据",
        description: "加密管理 API Key、Token 等个人敏感凭据、使用授权与引用确认",
        keywords: ["vault", "secret", "password", "token", "key", "保险箱", "凭据", "密钥", "密码", "脱敏", "迁移"],
        menu: "vault-config",
        subNav: "vault",
    },
    {
        id: "anti-leakage",
        title: "防泄露模式",
        section: "安全与凭据",
        description: "对界面中的对话标题和敏感内容进行脱敏遮挡保护隐私",
        keywords: ["privacy", "leak", "mask", "hide", "防泄露", "隐私", "打码", "遮挡"],
        menu: "vault-config",
        subNav: "policies#anti-leakage",
    },
    {
        id: "security-overview",
        title: "隐私与安全策略总览",
        section: "安全与凭据",
        description: "统一查看端到端同步加密、工具审核与工作区信任风险状态",
        keywords: ["security", "policy", "safety", "安全", "策略", "风险"],
        menu: "vault-config",
        subNav: "policies#security-overview",
    },
    {
        id: "data-folder",
        title: "本地数据目录",
        section: "数据与同步",
        description: "打开 AIPP 本地 SQLite 数据与附件存储文件夹",
        keywords: ["data", "folder", "sqlite", "storage", "数据目录", "文件夹", "数据库"],
        menu: "data-sync-config",
        subNav: "#local-data",
        desktopOnly: true,
    },
    {
        id: "self-hosted-sync",
        title: "自建跨设备同步",
        section: "数据与同步",
        description: "配置自建 sync-server 地址、访问 Token、自动同步开关与同步状态",
        keywords: ["sync", "server", "cloud", "device", "同步", "自建", "多端", "跨设备"],
        menu: "data-sync-config",
        subNav: "#sync-connection",
    },
    {
        id: "sync-passphrase",
        title: "凭据端到端加密同步口令",
        section: "数据与同步",
        description: "设置或更换用于加密同步 API Key、应用凭据与保险箱条目的口令",
        keywords: ["passphrase", "encrypt", "e2ee", "口令", "同步密码", "加密"],
        menu: "data-sync-config",
        subNav: "#passphrase",
    },
    {
        id: "sync-maintenance",
        title: "同步故障排查与维护",
        section: "数据与同步",
        description: "重试失败项、重试无法应用的变更、清理失效引用与重置同步状态",
        keywords: ["dead letter", "reset", "repair", "dangling", "重置同步", "死信", "清理", "故障"],
        menu: "data-sync-config",
        subNav: "#sync-maintenance",
    },

    { id: "onboarding-setup", title:"后台准备任务", section:"常规与外观", description:"查看或重试 Skills、Office、bun 和 uv 安装", keywords:["安装","准备","重试"], menu:"general-config", subNav:"#onboarding", desktopOnly:true },
    // 通用与系统
    {
        id: "startup-window",
        title: "默认主页窗口与开机自启",
        section: "常规与外观",
        description: "设置应用启动和托盘唤醒时的默认主窗口，以及系统开机自启动",
        keywords: ["home", "window", "autostart", "boot", "startup", "主页", "默认窗口", "开机自启", "自启动", "托盘"],
        menu: "general-config",
        subNav: "#startup",
        desktopOnly: true,
    },
    {
        id: "notifications",
        title: "系统通知提醒",
        section: "常规与外观",
        description: "控制管家回复、AI 消息/Agent 任务完成与等待审批时的系统通知",
        keywords: ["notification", "alert", "bell", "通知", "提醒", "弹窗", "审批通知"],
        menu: "general-config",
        subNav: "#notifications",
    },
    {
        id: "appearance-theme",
        title: "外观主题与代码高亮配色",
        section: "常规与外观",
        description: "切换系统外观主题、浅色/深色/跟随系统模式及代码块高亮主题",
        keywords: ["theme", "dark", "light", "color", "code", "highlight", "主题", "深色", "浅色", "外观", "代码配色"],
        menu: "general-config",
        subNav: "#appearance",
    },
    {
        id: "chat-display",
        title: "对话渲染与消息显示",
        section: "常规与外观",
        description: "配置用户消息 Markdown 渲染、合并助手消息、思考过程展示、工具栏与 MCP 卡片组件",
        keywords: ["markdown", "merge", "thinking", "reasoning", "bubble", "tool", "合并消息", "思考过程", "工具组件", "渲染"],
        menu: "general-config",
        subNav: "#chat-display",
    },
    {
        id: "shortcuts",
        title: "全局与应用内快捷键",
        section: "快捷键",
        description: "自定义唤起 Ask 窗口的全局快捷键以及各窗口内的操作快捷键",
        keywords: ["shortcut", "hotkey", "keybinding", "keyboard", "快捷键", "热键", "组合键"],
        menu: "shortcuts-config",
        desktopOnly: true,
    },
    {
        id: "network-proxy",
        title: "网络代理、超时与重试",
        section: "网络与代理",
        description: "设置请求超时时间、失败重试次数、HTTP/SOCKS 代理与自定义请求头",
        keywords: ["proxy", "timeout", "retry", "header", "http", "socks", "代理", "超时", "重试", "请求头", "网络"],
        menu: "network-config",
        subNav: "#network-general",
    },
    {
        id: "network-openai-responses",
        title: "OpenAI Responses 缓存与续接",
        section: "网络与代理",
        description: "配置 OpenAI Responses 提示词缓存标识、保留时间、旧版策略及服务端会话续接",
        keywords: ["openai", "responses", "cache", "ttl", "stateful", "缓存", "会话续接", "保留时间"],
        menu: "network-config",
        subNav: "#openai-responses",
    },
    {
        id: "runtimes",
        title: "本地运行环境（Bun / uv / Python）",
        section: "运行环境与预览",
        description: "检测、安装与更新用于制品预览和脚本执行的 Bun、uv 与 Python 环境",
        keywords: ["bun", "uv", "python", "runtime", "node", "环境", "运行时", "安装", "更新"],
        menu: "runtime-preview-config",
        subNav: "#runtimes",
        desktopOnly: true,
    },
    {
        id: "preview-whitelist",
        title: "预览外部资源白名单策略",
        section: "运行环境与预览",
        description: "可视化管理 HTML/React/Vue 预览允许加载的外部域名、资源类型与大小限额",
        keywords: ["preview", "whitelist", "domain", "cdn", "csp", "白名单", "外部资源", "预览安全", "域名"],
        menu: "runtime-preview-config",
        subNav: "#preview-whitelist",
        desktopOnly: true,
    },
    {
        id: "about",
        title: "关于 AIPP 与检查更新",
        section: "关于",
        description: "查看当前版本号、检查应用更新及开发者实验室说明",
        keywords: ["about", "version", "update", "lab", "experimental", "关于", "版本", "更新", "实验室"],
        menu: "about-config",
    },
];

export function searchSettings(
    query: string,
    options: { isMobilePlatform: boolean; availableMenus?: Set<string> }
): SettingsSearchItem[] {
    const trimmed = query.trim().toLowerCase();
    if (!trimmed) return [];

    const scored: Array<{ item: SettingsSearchItem; score: number }> = [];
    for (const item of SETTINGS_SEARCH_INDEX) {
        if (options.isMobilePlatform && item.desktopOnly) continue;
        if (!options.isMobilePlatform && item.mobileOnly) continue;
        if (options.availableMenus && !options.availableMenus.has(item.menu)) continue;

        let score = 0;
        const titleLower = item.title.toLowerCase();
        if (titleLower === trimmed) score += 100;
        else if (titleLower.includes(trimmed)) score += 60;
        else if (PinyinFilter.matches(item.title, trimmed)) score += 50;

        if (item.keywords.some((kw) => kw.toLowerCase() === trimmed)) score += 45;
        else if (item.keywords.some((kw) => kw.toLowerCase().includes(trimmed) || PinyinFilter.matches(kw, trimmed))) score += 30;

        if (item.description.toLowerCase().includes(trimmed) || PinyinFilter.matches(item.description, trimmed)) score += 15;
        if (item.section.toLowerCase().includes(trimmed) || PinyinFilter.matches(item.section, trimmed)) score += 10;

        if (score > 0) {
            scored.push({ item, score });
        }
    }

    scored.sort((a, b) => b.score - a.score);
    return scored.map((entry) => entry.item);
}
