import { pluginTypeLabel, type InstalledPlugin, type PluginEntry } from './pluginCatalog';

/** 把 manifest 里的 contributions / permissions 翻译成用户能看懂的话；不显示原始字段名。 */

const DATA_LABELS: Record<string, string> = {
    conversation: '对话', llm: '模型', assistant: '助手', system: '系统设置', plugin: '插件', mcp: '应用与工具', artifacts: '制品',
};

export function describePermission(permission: string): string {
    const p = permission.trim();
    switch (p) {
        case 'conversation.read': return '读取对话内容';
        case 'conversation.write': return '在对话里新增或修改消息';
        case 'assistant.read': return '读取助手信息';
        case 'assistant.config': return '为每个助手保存插件自己的设置';
        case 'assistant.prompt.write': return '修改助手提示词';
        case 'plugin.storage': return '使用插件自己的本地数据库';
        case 'markdown.register': return '自定义消息里某些内容的显示方式';
        case 'bang.register': return '注册 ! 快捷命令';
    }
    if (p.startsWith('data.read.')) {
        const target = p.slice('data.read.'.length);
        return `只读访问本机的${DATA_LABELS[target] ?? target}数据`;
    }
    if (p.startsWith('network.')) {
        const target = p.slice('network.'.length);
        return target === 'comfyui' ? '访问你配置的 ComfyUI 服务' : `访问网络（${target}）`;
    }
    if (p.startsWith('hook.')) return '在发送消息、请求模型等对话环节运行';
    return p;
}

export function describePermissions(permissions: string[] | undefined): string[] {
    const lines = (permissions ?? []).map(describePermission);
    return [...new Set(lines)];
}

export interface ContributionLine { where: string; what: string }

const VIEW_LOCATIONS: Record<string, string> = {
    'config.plugin-center': '插件详情的「设置」页',
};

/** 「装上后会出现在」：已安装用真实 contributions，未安装只能按类型说明。 */
export function describeContributions(entry: PluginEntry): ContributionLine[] {
    const c = entry.installed?.contributions;
    const lines: ContributionLine[] = [];
    if (c) {
        if (c.bangs?.length) lines.push({ where: '输入框', what: c.bangs.flatMap(b => [b.name, ...(b.aliases ?? [])]).map(name => `!${name}`).join(' · ') });
        if (c.avatars?.length || c.accessories?.length) {
            lines.push({ where: '形象', what: [c.avatars?.length && `${c.avatars.length} 个助手形象`, c.accessories?.length && `${c.accessories.length} 件配饰`].filter(Boolean).join(' · ') + '，在助手资料的「形象」里选择' });
        }
        for (const view of c.views ?? []) lines.push({ where: '界面', what: VIEW_LOCATIONS[view.location] ? `${view.title}（${VIEW_LOCATIONS[view.location]}）` : view.title });
        if (c.actions?.length) lines.push({ where: '操作', what: c.actions.map(a => a.title).join('、') });
        if (c.assistantFormFields?.length) lines.push({ where: '助手设置', what: c.assistantFormFields.map(f => f.label).join('、') });
        if (c.hooks?.length) lines.push({ where: '对话流程', what: `${c.hooks.length} 个环节（${c.hooks.map(h => h.name).join('、')}）` });
    }
    if (!lines.length) {
        for (const type of entry.types) {
            const what = TYPE_EFFECTS[type];
            if (what) lines.push({ where: pluginTypeLabel(type), what });
        }
    }
    return lines;
}

const TYPE_EFFECTS: Record<string, string> = {
    themeType: '在「显示」设置里多一套可选主题',
    avatarType: '在助手资料的「形象」里多出可选形象',
    markdownType: '改变消息里特定内容的显示方式',
    toolType: '在输入框添加 ! 快捷命令',
    messageType: '新增一种消息类型',
    exportType: '新增一种对话导出格式',
    assistantType: '提供带自有流程的助手',
    interfaceType: '新增独立界面或设置面板',
    applicationType: '在后台处理数据或参与对话流程',
};

export function contributionSummary(plugin: InstalledPlugin): string {
    const c = plugin.contributions;
    if (!c) return '';
    return [
        c.views?.length && `${c.views.length} 视图`,
        c.actions?.length && `${c.actions.length} 操作`,
        c.hooks?.length && `${c.hooks.length} Hook`,
        c.bangs?.length && c.bangs.map(b => `!${b.name}`).join(' · '),
        c.avatars?.length && `${c.avatars.length} 形象`,
        c.accessories?.length && `${c.accessories.length} 配饰`,
    ].filter(Boolean).join(' · ');
}
