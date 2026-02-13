import { useMemo } from 'react';
import { Message } from '../data/Conversation';
import { CodeArtifact } from '../components/chat-sidebar/types';
import { parseCodeBlockMeta } from '../react-markdown/remarkCodeBlockMeta';

// Regex to match markdown code blocks with optional meta info
// Matches ```lang meta\n...``` where meta can contain title="..." or other attributes
// Group 1: language, Group 2: meta info (optional), Group 3: code content
const CODE_BLOCK_REGEX = /```(\w*)([^\n]*)\n([\s\S]*?)```/g;


const ARTIFACT_LIST_LANGUAGES = new Set([
    'vue',
    'react',
    'jsx',
    'tsx',
    'html',
    'markdown',
    'md',
    'mermaid',
    'drawio',
]);

interface UseArtifactExtractorOptions {
    messages: Message[];
}

interface UseArtifactExtractorReturn {
    artifacts: CodeArtifact[];
}

export function useArtifactExtractor({ messages }: UseArtifactExtractorOptions): UseArtifactExtractorReturn {
    const artifacts = useMemo(() => {
        const result: CodeArtifact[] = [];
        
        // Filter to only response/assistant messages first
        const responseMessages = messages.filter(
            m => m.message_type === 'assistant' || m.message_type === 'response'
        );

        responseMessages.forEach((message, messageIdx) => {
            if (!message.content) return;

            let match;
            let blockIndex = 0;
            CODE_BLOCK_REGEX.lastIndex = 0; // Reset regex state

            while ((match = CODE_BLOCK_REGEX.exec(message.content)) !== null) {
                const language = match[1] || 'text';
                const meta = match[2] || '';
                const code = match[3].trim();

                const normalizedLanguage = language.toLowerCase();
                if (!ARTIFACT_LIST_LANGUAGES.has(normalizedLanguage)) continue;

                // Skip very short code blocks (likely not meaningful artifacts)
                if (code.length < 20) continue;

                blockIndex++;
                const metaInfo = meta.trim() ? parseCodeBlockMeta(meta) : {};
                const metaTitle = metaInfo.title;
                
                // Display title: use meta title if available, otherwise fall back to default
                const displayTitle = metaTitle || `消息${messageIdx + 1} - 代码块${blockIndex}`;

                result.push({
                    id: `artifact-${message.id}-${blockIndex}`,
                    language,
                    code,
                    messageId: message.id,
                    messageIndex: messageIdx + 1,
                    blockIndex,
                    metaTitle,
                    title: displayTitle,
                });
            }
        });

        console.log('[useArtifactExtractor] Extracted artifacts:', result.length, 'from', responseMessages.length, 'assistant/response messages');
        return result;
    }, [messages]);

    return { artifacts };
}
