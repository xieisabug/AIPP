import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
    ButlerGroupDetail, ButlerGroupMessage, ButlerGroupStopSummary, ButlerGroupSummary,
} from "@/data/ButlerGroup";

export const BUTLER_GROUP_CHANGED_EVENT = "butler_group_changed";

export const listButlerGroups = () => invoke<ButlerGroupSummary[]>("list_butler_groups");

export const getButlerGroup = (groupId: number) => invoke<ButlerGroupDetail>("get_butler_group", { groupId });

export const listOlderButlerGroupMessages = (groupId: number, before: number, limit = 80) =>
    invoke<ButlerGroupMessage[]>("list_butler_group_messages", { groupId, before, limit });

export const createButlerGroup = (name: string, assistantIds: number[], includeChief: boolean) =>
    invoke<ButlerGroupSummary>("create_butler_group", { name: name.trim() || null, assistantIds, includeChief });

/** 相同 requestKey 的重试不会重复发送。 */
export const postButlerGroupMessage = (groupId: number, requestKey: string, body: string, replyTo: number | null) =>
    invoke<ButlerGroupMessage>("post_butler_group_message", { groupId, requestKey, body, replyTo });

export const stopButlerGroupReplies = (groupId: number) =>
    invoke<ButlerGroupStopSummary>("stop_butler_group_replies", { groupId });

export const addButlerGroupMembers = (groupId: number, assistantIds: number[], includeChief: boolean) =>
    invoke<void>("add_butler_group_members", { groupId, assistantIds, includeChief });

export const removeButlerGroupMember = (groupId: number, memberId: number) =>
    invoke<void>("remove_butler_group_member", { groupId, memberId });

export const renameButlerGroup = (groupId: number, name: string) =>
    invoke<void>("rename_butler_group", { groupId, name });

export const deleteButlerGroup = (groupId: number) => invoke<void>("delete_butler_group", { groupId });

/** 任一群发生变化时回调；groupId 为变化的群。 */
export const onButlerGroupChanged = (callback: (groupId: number) => void) =>
    listen<{ group_id: number }>(BUTLER_GROUP_CHANGED_EVENT, ({ payload }) => callback(payload.group_id));
