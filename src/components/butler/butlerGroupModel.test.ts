import { describe, expect, it } from "vitest";
import type { ButlerGroupMember, ButlerGroupMessage } from "@/data/ButlerGroup";
import {
    applyMention, findMentionQuery, groupPreview, mentionOptions, mergeMessages, showTimeDivider,
} from "./butlerGroupModel";

const member = (id: number, name: string): ButlerGroupMember => ({ id, assistant_id: id + 10, name, conversation_id: id + 100, is_chief: false });
const message = (seq: number, patch: Partial<ButlerGroupMessage> = {}): ButlerGroupMessage => ({
    group_id: 1, seq, sender_member: null, sender_assistant: null, sender_name: "我", kind: "message",
    body: `消息 ${seq}`, reply_to: null, created_at: "2026-11-09 08:00:00", source_conversation_id: null, source_message_id: null, ...patch,
});

describe("butlerGroupModel", () => {
    it("should detect the mention being typed when the caret follows an @", () => {
        expect(findMentionQuery("你好 @写", 5)).toEqual({ start: 3, query: "写" });
        expect(findMentionQuery("@", 1)).toEqual({ start: 0, query: "" });
        expect(findMentionQuery("@写作 助手", 6)).toBeNull();
        expect(findMentionQuery("mail me@host", 12)).toBeNull();
    });

    it("should offer matching members and everyone when filtering mentions", () => {
        const members = [member(1, "写作助手"), member(2, "Coder")];
        expect(mentionOptions(members, "co").map(option => option.label)).toEqual(["Coder"]);
        expect(mentionOptions(members, "").map(option => option.label)).toEqual(["写作助手", "Coder", "所有人"]);
        expect(mentionOptions([members[0]], "").map(option => option.label)).toEqual(["写作助手"]);
    });

    it("should replace the typed query with the chosen name when applying a mention", () => {
        const text = "请 @写 看看";
        const query = findMentionQuery(text, 4)!;
        expect(applyMention(text, query, 4, "写作助手")).toEqual({ text: "请 @写作助手  看看", caret: 8 });
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
