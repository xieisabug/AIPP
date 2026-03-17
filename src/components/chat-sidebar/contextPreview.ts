import { invoke } from '@tauri-apps/api/core';
import { ContextItem } from './types';

export interface ContextPreviewPayload {
    lang: string;
    inputStr: string;
}

interface SkillContentFile {
    path: string;
    content: string;
}

interface SkillContentResponse {
    identifier: string;
    content: string;
    additional_files: SkillContentFile[];
}

export function buildPreviewPayloadFromContextItem(item: ContextItem): ContextPreviewPayload | null {
    if (item.previewData?.contentType === 'markdown' && item.previewData.content) {
        return {
            lang: 'markdown',
            inputStr: item.previewData.content,
        };
    }

    return null;
}

export async function hydrateContextPreview(item: ContextItem): Promise<ContextItem> {
    if (item.type !== 'skill' || item.previewStatus !== 'needs_load') {
        return item;
    }

    const identifier = item.previewData?.rawValue || item.details || item.name;
    if (!identifier) {
        throw new Error('Skill identifier is required for preview loading');
    }

    const skillContent = await invoke<SkillContentResponse>('get_skill_content', { identifier });

    return {
        ...item,
        previewStatus: 'ready',
        previewData: {
            ...item.previewData,
            title: item.previewData?.title || item.name,
            subtitle: skillContent.identifier,
            rawValue: skillContent.identifier,
            contentType: 'markdown',
            content: skillContent.content,
            items: skillContent.additional_files.map((file) => ({
                label: file.path,
                value: file.content,
                description: `${file.content.length} 字符`,
            })),
            metadata: {
                ...item.previewData?.metadata,
                标识符: skillContent.identifier,
                附加文件: String(skillContent.additional_files.length),
            },
        },
    };
}
