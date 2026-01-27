// Chat Sidebar Types
// These types are used for the right sidebar in Chat UI

export interface TodoItem {
    content: string;
    status: 'pending' | 'in_progress' | 'completed';
    activeForm: string;
}

export interface TodoUpdateEvent {
    conversation_id: number;
    todos: TodoItem[];
}

export interface CodeArtifact {
    id: string;
    language: string;
    code: string;
    messageId: number;
    // Message index (1-based for display)
    messageIndex: number;
    // Block index within the message (1-based for display)
    blockIndex: number;
    // Meta title from code block (e.g., ```ts title="App.tsx")
    metaTitle?: string;
    // Computed display title
    title: string;
}

export interface SearchResultItem {
    title: string;
    url: string;
    snippet?: string;
    displayUrl?: string;
    rank?: number;
}

export interface ContextItem {
    id: string;
    type: 'user_file' | 'read_file' | 'search' | 'list_directory' | 'other';
    name: string;
    // Optional details like file path, search query
    details?: string;
    // Optional search results for search context items
    searchResults?: SearchResultItem[];
    // Source: user input, MCP tool call
    source: 'user' | 'mcp';
    // Optional timestamp
    timestamp?: Date;
    // Attachment data for user files
    attachmentData?: {
        type: 'Image' | 'Text' | 'PDF' | 'Word' | 'PowerPoint' | 'Excel' | string;
        content?: string; // base64 for images
        url?: string; // file path
    };
}

// Tool names that represent context operations
export const CONTEXT_TOOL_NAMES = [
    'read_file',
    'get_file_contents',
    'list_directory',
    'list_allowed_directories',
    'search_files',
    'search',
    'grep',
    'glob',
    'find',
    'view',
];

export function getContextTypeFromToolName(toolName: string): ContextItem['type'] {
    const normalizedName = toolName.toLowerCase();
    if (normalizedName.includes('read') || normalizedName.includes('get_file') || normalizedName === 'view') {
        return 'read_file';
    }
    if (normalizedName.includes('search') || normalizedName.includes('grep') || normalizedName.includes('find') || normalizedName.includes('glob')) {
        return 'search';
    }
    if (normalizedName.includes('directory') || normalizedName.includes('list')) {
        return 'list_directory';
    }
    return 'other';
}
