import { useMemo } from 'react';
import { Message } from '../data/Conversation';
import { CodeArtifact } from '../components/chat-sidebar/types';

// Regex to match markdown code blocks with optional meta info
// Matches ```lang meta\n...``` where meta can contain title="..." or other attributes
// Group 1: language, Group 2: meta info (optional), Group 3: code content
const CODE_BLOCK_REGEX = /```(\w*)([^\n]*)\n([\s\S]*?)```/g;

// Extract title from meta info like: title="App.tsx" or title='App.tsx'
const META_TITLE_REGEX = /title\s*=\s*["']([^"']+)["']/i;

interface UseArtifactExtractorOptions {
    messages: Message[];
}

interface UseArtifactExtractorReturn {
    artifacts: CodeArtifact[];
}

function extractMetaTitle(meta: string): string | undefined {
    const match = meta.match(META_TITLE_REGEX);
    return match ? match[1] : undefined;
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

                // Skip very short code blocks (likely not meaningful artifacts)
                if (code.length < 20) continue;

                blockIndex++;
                const metaTitle = extractMetaTitle(meta);
                
                // Display title: use metaTitle if available, otherwise "消息X - 代码块Y"
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
