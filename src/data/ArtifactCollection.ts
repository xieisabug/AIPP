export interface ArtifactCollectionItem {
    id: number;
    name: string;
    icon: string;
    description: string;
    artifact_type: string;
    tags?: string;
    created_time: string;
    last_used_time?: string;
    use_count: number;
    db_id?: string;
    assistant_id?: number;
}

export interface ArtifactCollection extends ArtifactCollectionItem {
    code: string;
}

export interface SaveArtifactRequest {
    name: string;
    icon: string;
    description: string;
    artifact_type: string;
    code: string;
    tags?: string;
    db_id?: string;
    assistant_id?: number;
}

export interface UpdateArtifactRequest {
    id: number;
    name?: string;
    icon?: string;
    description?: string;
    tags?: string;
    db_id?: string;
    assistant_id?: number;
}

export interface FilteredArtifact extends ArtifactCollectionItem {
    matchType: 'exact' | 'pinyin' | 'initial' | 'fuzzy';
    highlightIndices: number[];
}

export interface ArtifactMetadata {
    name: string;
    description: string;
    tags: string;
    emoji_category: string;
}

// Artifact Bridge Types
export interface ArtifactBridgeConfig {
    db_id?: string;
    assistant_id?: number;
    artifact_id?: number;
    artifact_name?: string;
}

export interface DbQueryResult {
    columns: string[];
    rows: unknown[][];
    row_count: number;
}

export interface DbExecuteResult {
    rows_affected: number;
    last_insert_rowid: number;
}

export interface DbTableInfo {
    name: string;
    sql: string;
}

export interface AssistantBasicInfo {
    id: number;
    name: string;
    description: string;
    icon: string;
}

export interface AiAskResponse {
    content: string;
    model: string;
    usage?: {
        prompt_tokens?: number;
        completion_tokens?: number;
        total_tokens?: number;
    };
}