import { describe, expect, it, beforeEach } from 'vitest';
import { clearAllMockHandlers, mockInvokeHandler } from '@/__tests__/mocks/tauri';
import { buildPreviewPayloadFromContextItem, hydrateContextPreview } from './contextPreview';
import { ContextItem } from './types';

describe('contextPreview helpers', () => {
    beforeEach(() => {
        clearAllMockHandlers();
    });

    it('hydrates skill preview data with SKILL.md content from backend', async () => {
        mockInvokeHandler('get_skill_content', () => ({
            identifier: 'agents:skill-creator',
            content: '# Skill Creator\n\nBuild and test a skill.',
            additional_files: [
                {
                    path: 'template.md',
                    content: '## Template',
                },
            ],
        }));

        const item: ContextItem = {
            id: 'skill-1',
            type: 'skill',
            name: 'skill-creator',
            details: 'agents:skill-creator',
            source: 'user',
            previewStatus: 'needs_load',
            previewData: {
                title: 'skill-creator',
                subtitle: 'agents:skill-creator',
                rawValue: 'agents:skill-creator',
                contentType: 'file-meta',
            },
        };

        const hydrated = await hydrateContextPreview(item);

        expect(hydrated.previewStatus).toBe('ready');
        expect(hydrated.previewData).toEqual(
            expect.objectContaining({
                rawValue: 'agents:skill-creator',
                contentType: 'markdown',
                content: '# Skill Creator\n\nBuild and test a skill.',
                metadata: expect.objectContaining({
                    标识符: 'agents:skill-creator',
                    附加文件: '1',
                }),
            }),
        );
        expect(hydrated.previewData?.items).toEqual([
            expect.objectContaining({
                label: 'template.md',
                value: '## Template',
            }),
        ]);
    });

    it('builds markdown preview payloads from hydrated context items', () => {
        const item: ContextItem = {
            id: 'skill-1',
            type: 'skill',
            name: 'skill-creator',
            source: 'user',
            previewStatus: 'ready',
            previewData: {
                title: 'skill-creator',
                rawValue: 'agents:skill-creator',
                contentType: 'markdown',
                content: '# Skill Creator',
            },
        };

        expect(buildPreviewPayloadFromContextItem(item)).toEqual({
            lang: 'markdown',
            inputStr: '# Skill Creator',
        });
    });
});
