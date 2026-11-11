import { describe, expect, it } from "vitest";
import type { ButlerGroupMember, ButlerGroupMessage } from "@/data/ButlerGroup";
import {
    groupPreview, mentionTargets, mergeMessages, showTimeDivider,
} from "./butlerGroupModel";

const member = (id: number, name: string): ButlerGroupMember => ({ id, assistant_id: id + 10, name, conversation_id: id + 100, is_chief: false });
const message = (seq: number, patch: Partial<ButlerGroupMessage> = {}): ButlerGroupMessage => ({
    group_id: 1, seq, sender_member: null, sender_assistant: null, sender_name: "我", kind: "message",
    body: `消息 ${seq}`, reply_to: null, created_at: "2026-11-09 08:00:00", source_conversation_id: null, source_message_id: null, ...patch,
});

describe("butlerGroupModel", () => {
    it("should offer every member and everyone when building mention targets", () => {
        const members = [member(1, "写作助手"), member(2, "Coder")];
        expect(mentionTargets(members).map(target => target.name)).toEqual(["写作助手", "Coder", "所有人"]);
        expect(mentionTargets([members[0]]).map(target => target.name)).toEqual(["写作助手"]);
        // “所有人”的 id 不能和成员撞上，列表用它做 key
        const ids = mentionTargets(members).map(target => target.id);
        expect(new Set(ids).size).toBe(ids.length);
    });

    it("should merge older pages without duplicates when loading history", () => {
        const merged = mergeMessages([message(1), message(2)], [message(2, { body: "新" }), message(3)]);
        expect(merged.map(m => [m.seq, m.body])).toEqual([[1, "消息 1"], [2, "新"], [3, "消息 3"]]);
    });

    it("should show a time divider when messages are five minutes apart", () => {
        const first = message(1);
        expect(showTimeDivider(first)).toBe(true);
        expect(showTimeDivider(message(2, { created_at: "2026-11-09 08:04:00" }), first)).toBe(false);
        expect(showTimeDivider(message(2, { created_at: "2026-11-09 08:05:00" }), first)).toBe(true);
    });

    it("should label the speaker when previewing the latest group message", () => {
        expect(groupPreview(message(1, { body: "大家好" }))).toBe("我：大家好");
        expect(groupPreview(message(2, { sender_member: 3, sender_name: "Coder", body: "收到\n马上看" }))).toBe("Coder：收到 马上看");
        expect(groupPreview(message(3, { kind: "notice", body: "你邀请 Coder 加入了群聊" }))).toBe("你邀请 Coder 加入了群聊");
        expect(groupPreview(null)).toBe("");
    });
});
