import { describe, expect, it, vi } from 'vitest';

vi.mock('@/services/PluginRuntime', () => ({ pluginRuntime: { listThemeVariablesOf: () => [] } }));
vi.mock('@/components/avatar/AnimatedAvatar', () => ({ default: () => null }));

import { previewImageSources } from './PluginPreview';
import type { InstalledPlugin, OfficialPlugin, PluginEntry } from './pluginCatalog';

const remote = 'https://aipp-helper.xiejingyang.com/plugin-previews/think-markdown-plugin-0.2.0-1.webp';
const official = { id: 'think-markdown-plugin', code: 'think-markdown-plugin', previewImages: [remote, 'https://evil.example/x.webp'] } as OfficialPlugin;
const installed = (preview: string[], isInstalled = true) => ({ code: 'think-markdown-plugin', isInstalled, preview } as InstalledPlugin);
const entry = (i?: InstalledPlugin): PluginEntry => ({ code: 'think-markdown-plugin', official, installed: i } as PluginEntry);

describe('previewImageSources', () => {
    it('uses trusted catalog images when the plugin is not installed', () => {
        expect(previewImageSources(entry())).toEqual({ local: [], remote: [remote] });
    });

    it('falls back to catalog images for an older install without its own preview', () => {
        expect(previewImageSources(entry(installed([])))).toEqual({ local: [], remote: [remote] });
    });

    it('does not repeat catalog images when the installed package ships the same previews', () => {
        expect(previewImageSources(entry(installed(['preview/1.webp'])))).toEqual({ local: ['preview/1.webp'], remote: [] });
    });

    it('ignores the local preview of a plugin whose folder is missing', () => {
        expect(previewImageSources(entry(installed(['preview/1.webp'], false)))).toEqual({ local: [], remote: [remote] });
    });
});
