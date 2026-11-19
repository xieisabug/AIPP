import { describe, expect, it } from "vitest";
import type { ButlerGroupMessage } from "@/data/ButlerGroup";
import { groupVaultRequests } from "./groupVaultRequests";

const message = (seq: number, conversation = 101, source = seq + 1000): ButlerGroupMessage => ({
    group_id: 7, seq, sender_member: 1, sender_assistant: 11, sender_name: "成员", kind: "message", body: `message ${seq}`,
    reply_to: null, created_at: "2026-11-19 00:00:00", source_conversation_id: conversation, source_message_id: source,
});
const request = (requestId: string, seq: number) => ({ requestId, conversationId: 101, messageId: 9999, groupAnchor: { groupId: 7, seq } });

describe("group Vault request anchors", () => {
    it("should keep multiple cards on their own messages when new messages arrive", () => {
        const requests = [request("a", 1), request("b", 2), request("c", 1)];
        for (const messages of [[message(1), message(2)], [message(1), message(2), message(3)]]) {
            const result = groupVaultRequests(7, messages, requests);
            expect(result.bySeq.get(1)?.map(r => r.requestId)).toEqual(["a", "c"]);
            expect(result.bySeq.get(2)?.map(r => r.requestId)).toEqual(["b"]);
            expect(result.bySeq.has(3)).toBe(false);
        }
    });
    it("should wait for the exact unloaded target and render it once after loading", () => {
        const pending = request("a", 1);
        const initial = groupVaultRequests(7, [message(2)], [pending, pending]);
        expect(initial.bySeq.size).toBe(0);
        expect(initial.pending).toEqual([pending]);
        const loaded = groupVaultRequests(7, [message(1), message(2)], [pending, pending]);
        expect(loaded.pending).toEqual([]);
        expect(loaded.bySeq.get(1)).toEqual([pending]);
    });
    it("should never approximate source ids, cross conversations or cross groups", () => {
        const source = { requestId: "legacy", conversationId: 101, messageId: 1001 };
        expect(groupVaultRequests(7, [message(2)], [source]).pending).toEqual([source]);
        expect(groupVaultRequests(7, [message(1, 102)], [source]).pending).toEqual([source]);
        expect(groupVaultRequests(7, [message(1)], [{ ...request("wrong-group", 1), groupAnchor: { groupId: 8, seq: 1 } }]).bySeq.size).toBe(0);
        expect(groupVaultRequests(7, [message(1)], [source]).bySeq.get(1)).toEqual([source]);
    });
    it("should remove completed or cancelled cards and ignore late target loading", () => {
        expect(groupVaultRequests(7, [message(1)], []).bySeq.size).toBe(0);
        expect(groupVaultRequests(7, [message(1), message(2)], [request("remaining", 2)]).bySeq.has(1)).toBe(false);
    });
});
