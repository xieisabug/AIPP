import { describe, expect, it } from "vitest";
import { recoveryNotices } from "./MobileRecoveryNotice";

describe("mobile recovery observations", () => {
    it("should explain unknown execution and missing attachments independently", () => {
        const result = recoveryNotices({ conversation_id: 1, local_running: false, unknown_tool_ids: [3], incomplete_message_ids: [4], missing_attachment_ids: [5] });
        expect(result).toHaveLength(3);
        expect(result[0]).toContain("结果未知");
        expect(result[1]).toContain("不会自动重发");
        expect(result[2]).toContain("重新导入");
    });
    it("should remain quiet when no unresolved history exists", () => {
        expect(recoveryNotices({ conversation_id: 1, local_running: true, unknown_tool_ids: [], incomplete_message_ids: [], missing_attachment_ids: [] })).toEqual([]);
    });
});
