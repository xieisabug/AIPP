import { describe, expect, it } from 'vitest';
import {
    buildPluginEntries, compareVersions, matchesPluginQuery, parsePluginSourceInput, pluginCardAction, pluginDisplayStatus, primaryPluginType,
    type InstalledPlugin, type OfficialPlugin,
} from './pluginCatalog';
import { describeContributions, describePermission } from './pluginDescribe';

const official = (over: Partial<OfficialPlugin> = {}): OfficialPlugin => ({
    id: 'theme', code: 'theme', name: '朱砂', description: '国风主题', version: '0.2.0', pluginTypes: ['themeType', 'interfaceType'], permissions: [],
    source: { type: 'zip', url: 'https://example.com/theme.zip' }, sha256: 'abc', ...over,
});
const installed = (over: Partial<InstalledPlugin> = {}): InstalledPlugin => ({
    pluginId: 1, name: '朱砂', version: '0.1.0', code: 'theme', pluginType: ['themeType', 'interfaceType', 'applicationType'],
    isActive: true, isInstalled: true, installSource: { folderName: 'theme', kind: 'official', officialId: 'theme', sourceLabel: 'x', installedAt: '2026-11-24T00:00:00Z' },
    ...over,
});

describe('pluginCatalog', () => {
    it('should pick the most specific type as primary when a plugin declares several', () => {
        expect(primaryPluginType(['interfaceType', 'applicationType', 'themeType'])).toBe('themeType');
        expect(primaryPluginType(['applicationType', 'toolType'])).toBe('toolType');
        expect(primaryPluginType(['interfaceType', 'applicationType'])).toBe('interfaceType');
        expect(primaryPluginType([])).toBe('assistantType');
    });

    it('should compare dotted versions numerically', () => {
        expect(compareVersions('0.2.0', '0.1.10')).toBe(1);
        expect(compareVersions('v1.0', '1.0.0')).toBe(0);
        expect(compareVersions('0.1.9', '0.1.10')).toBe(-1);
    });

    it('should mark only verified official installs as official and offer updates for them', () => {
        const [entry] = buildPluginEntries([official()], [installed()]);
        expect(entry.verifiedOfficial).toBe(true);
        expect(entry.updateVersion).toBe('0.2.0');
        expect(pluginDisplayStatus(entry)).toMatchObject({ label: '0.2.0 可更新', tone: 'warn', attention: true });
        expect(pluginCardAction(entry, pluginDisplayStatus(entry))).toBe('更新');
    });

    it('should not treat a same-code GitHub install as official or as updatable', () => {
        const source = { folderName: 'theme', kind: 'github', sourceLabel: 'someone/theme#main', installedAt: '2026-11-20T00:00:00Z' };
        const [entry] = buildPluginEntries([official()], [installed({ installSource: source })]);
        expect(entry.verifiedOfficial).toBe(false);
        expect(entry.thirdPartyShadowsOfficial).toBe(true);
        expect(entry.updateVersion).toBeUndefined();
    });

    it('should keep catalog entries official and flag installs without a source record', () => {
        const entries = buildPluginEntries([official({ code: 'other', id: 'other' })], [installed({ installSource: null })]);
        expect(entries.find(e => e.code === 'other')).toMatchObject({ verifiedOfficial: true, installed: undefined });
        expect(entries.find(e => e.code === 'theme')).toMatchObject({ verifiedOfficial: false, unknownSource: true });
    });

    it('should prioritise broken installs over update and enable state', () => {
        const [missing] = buildPluginEntries([official()], [installed({ isInstalled: false })]);
        expect(pluginDisplayStatus(missing)).toMatchObject({ label: '插件目录缺失', tone: 'error' });
        const [failed] = buildPluginEntries([official()], []);
        expect(pluginDisplayStatus(failed, { install: { status: 'failed', error: 'x' } })).toMatchObject({ label: '安装失败', tone: 'error' });
        expect(pluginDisplayStatus(failed)).toMatchObject({ label: '未安装', tone: 'idle' });
    });

    it('should search names, descriptions and type labels', () => {
        const [entry] = buildPluginEntries([official()], []);
        expect(matchesPluginQuery(entry, '国风')).toBe(true);
        expect(matchesPluginQuery(entry, '主题')).toBe(true);
        expect(matchesPluginQuery(entry, '快捷命令')).toBe(false);
    });

    it('should parse GitHub shorthands, tree links and zip links', () => {
        expect(parsePluginSourceInput('owner/repo#dev').source).toEqual({ type: 'github', repo: 'owner/repo', ref: 'dev' });
        expect(parsePluginSourceInput('https://github.com/o/r/tree/main/plugins/demo')).toEqual({
            source: { type: 'github', repo: 'o/r', ref: 'main' }, dirs: [{ from: 'plugins/demo', to: 'demo' }],
        });
        expect(parsePluginSourceInput('https://example.com/a.zip').source).toEqual({ type: 'zip', url: 'https://example.com/a.zip' });
        expect(() => parsePluginSourceInput('https://gitlab.com/o/r')).toThrow('只支持');
    });
});

describe('pluginDescribe', () => {
    it('should translate permissions into plain language', () => {
        expect(describePermission('assistant.prompt.write')).toBe('修改助手提示词');
        expect(describePermission('data.read.llm')).toBe('只读访问本机的模型数据');
        expect(describePermission('network.comfyui')).toBe('访问你配置的 ComfyUI 服务');
        expect(describePermission('custom.thing')).toBe('custom.thing');
    });

    it('should describe real contributions for installed plugins and fall back to types otherwise', () => {
        const [withBangs] = buildPluginEntries([], [installed({ code: 'dir', pluginType: ['toolType'], contributions: { bangs: [{ name: 'dir', aliases: ['directory'] }] } })]);
        expect(describeContributions(withBangs)).toEqual([{ where: '输入框', what: '!dir · !directory' }]);
        const [notInstalled] = buildPluginEntries([official()], []);
        expect(describeContributions(notInstalled).map(line => line.where)).toEqual(['主题', '界面']);
    });
});
