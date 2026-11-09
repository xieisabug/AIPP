import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
    ButlerGroup, ButlerGroupDetail, ButlerGroupMember, ButlerGroupMessage,
    ButlerGroupPost, ButlerGroupRound, ButlerGroupStopResult, ButlerGroupTaskRequest,
} from "@/data/ButlerGroup";

export const listButlerGroups = () => invoke<ButlerGroup[]>("list_butler_groups");

export const createButlerGroup = (name: string, goal: string, assistantIds: number[]) =>
    invoke<{ group: ButlerGroup; members: ButlerGroupMember[] }>("create_butler_group", { name, goal, assistantIds });

export const getButlerGroup = (groupId: number) =>
    invoke<ButlerGroupDetail>("get_butler_group", { groupId });

export const listButlerGroupMessages = (groupId: number, after = 0, limit = 100) =>
    invoke<ButlerGroupMessage[]>("list_butler_group_messages", { groupId, after, limit });

export const postButlerGroupMessage = (groupId: number, message: ButlerGroupPost) =>
    invoke<ButlerGroupMessage>("post_butler_group_message", { groupId, message });

export const addButlerGroupMember = (groupId: number, assistantId: number) =>
    invoke<ButlerGroupMember>("add_butler_group_member", { groupId, assistantId });

export const removeButlerGroupMember = (groupId: number, memberId: number) =>
    invoke<ButlerGroupStopResult>("remove_butler_group_member", { groupId, memberId });

export const shareButlerGroupHistory = (groupId: number, memberId: number, messageSeqs: number[], requestKey: string) =>
    invoke<void>("share_butler_group_history", { groupId, memberId, messageSeqs, requestKey });

export const startButlerGroupRound = (groupId: number, messageSeq: number, memberIds: number[], requestKey: string) =>
    invoke<ButlerGroupRound>("start_butler_group_round", { groupId, messageSeq, memberIds, requestKey });

export const stopButlerGroup = (groupId: number) =>
    invoke<ButlerGroupStopResult[]>("stop_butler_group", { groupId });

export const stopButlerGroupMember = (groupId: number, memberId: number) =>
    invoke<ButlerGroupStopResult>("stop_butler_group_member", { groupId, memberId });

export const startButlerGroupTask = (groupId: number, request: ButlerGroupTaskRequest) =>
    invoke("start_butler_group_task", { groupId, request });

export const stopButlerGroupTask = (groupId: number, conversationId: number) =>
    invoke<ButlerGroupStopResult[]>("stop_butler_group_task", { groupId, conversationId });

export const replaceButlerGroupGoal = (groupId: number, goal: string, requestKey: string) =>
    invoke<ButlerGroupStopResult[]>("replace_butler_group_goal", { groupId, goal, requestKey });

export const setButlerGroupWaitLimit = (groupId: number, seconds: number, requestKey: string) =>
    invoke<void>("set_butler_group_wait_limit", { groupId, seconds, requestKey });

export interface GroupCapabilities {
    member_id: number;
    dynamic_loading: boolean;
    connectivity_verified: boolean;
    servers: { id: number; name: string; summary: string; tools: { name: string; description: string; is_enabled: boolean }[] }[];
}
export const getButlerGroupMemberCapabilities = (groupId: number, memberId: number) =>
    invoke<GroupCapabilities>("get_butler_group_member_capabilities", { groupId, memberId });

/** Subscription is bound to the selected group, not a mutable global selection. */
export const onButlerGroupChanged = (groupId: number, refresh: () => void) =>
    listen<{ group_id: number }>("butler_group_changed", ({ payload }) => {
        if (payload.group_id === groupId) refresh();
    });
