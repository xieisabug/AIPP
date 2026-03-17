import { Conversation, ConversationRuntimeState } from "./Conversation";

export interface ButlerTaskListItem {
    butler_conversation_id: number;
    task_conversation_id: number;
    title: string;
    goal: string;
    status: string;
    executor_assistant_id: number;
    executor_assistant_name: string;
    last_summary: string | null;
    created_time: string;
    updated_time: string;
    finalized_at: string | null;
    is_finalized: boolean;
    has_pending_permission: boolean;
    is_running: boolean;
}

export interface ButlerTaskDefinition {
    id: number;
    butler_conversation_id: number;
    task_conversation_id: number;
    title: string;
    goal: string;
    executor_assistant_id: number;
    executor_assistant_source: string;
    permission_template_source: string | null;
    handoff_contract_json: string | null;
    result_handling_mode: string | null;
    notification_policy: string | null;
    created_time: string;
}

export interface ButlerTaskResult {
    id: number;
    task_conversation_id: number;
    handoff_mode: string | null;
    payload_json: string | null;
    summary: string | null;
    structured_output_json: string | null;
    evidence_json: string | null;
    artifact_refs_json: string | null;
    followup_suggestions_json: string | null;
    final_message_id: number | null;
    created_time: string;
    updated_time: string;
}

export interface ButlerTaskDetailResponse {
    task: ButlerTaskListItem;
    conversation: Conversation;
    definition: ButlerTaskDefinition;
    result: ButlerTaskResult | null;
    runtime_state: ConversationRuntimeState;
}

export interface ButlerMainLoadResponse {
    conversation: Conversation;
    model_id: string;
    model_display_name: string;
    tasks: ButlerTaskListItem[];
}

export interface SpawnButlerTaskRequest {
    butler_conversation_id: number;
    title: string;
    goal: string;
    executor_assistant_id?: number | null;
    executor_assistant_name?: string | null;
    handoff_contract_json?: string | null;
    result_handling_mode?: string | null;
    notification_policy?: string | null;
}

export interface SpawnButlerTaskResponse {
    butler_conversation_id: number;
    task_conversation_id: number;
    title: string;
    status: string;
    executor_assistant_id: number;
    executor_assistant_name: string;
}

export interface ButlerTaskResultAvailableEvent {
    task: ButlerTaskListItem;
    result: ButlerTaskResult;
}

export interface ButlerNotificationEvent {
    butler_conversation_id: number;
    task_conversation_id: number;
    notification_type: string;
    title: string;
    body: string;
    importance: string;
}
