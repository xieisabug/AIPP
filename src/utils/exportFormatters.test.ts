import { describe, it, expect } from "vitest";
import { getLatestBranchMessages } from "./exportFormatters";

type TestMessage = {
    id: number;
    created_time: string;
    generation_group_id?: string | null;
    parent_group_id?: string | null;
};

const makeMessage = (
    id: number,
    created_time: string,
    generation_group_id?: string | null,
    parent_group_id?: string | null,
): TestMessage => ({
    id,
    created_time,
    generation_group_id: generation_group_id ?? null,
    parent_group_id: parent_group_id ?? null,
});

const makeTime = (seconds: number) =>
    new Date(Date.UTC(2024, 0, 1, 0, 0, seconds)).toISOString();

describe("getLatestBranchMessages (BDD)", () => {
    it("Given a linear conversation, when selecting latest branch, then keeps all messages", () => {
        const messages = [
            makeMessage(2, "2024-01-01T00:00:01Z"),
            makeMessage(1, "2024-01-01T00:00:00Z"),
            makeMessage(3, "2024-01-01T00:00:02Z", "g1"),
        ];

        const result = getLatestBranchMessages(messages);
        expect(result.map((msg) => msg.id)).toEqual([1, 2, 3]);
    });

    it("Given a regenerated response, when selecting latest branch, then drops parent group", () => {
        const messages = [
            makeMessage(1, "2024-01-01T00:00:00Z"),
            makeMessage(2, "2024-01-01T00:00:01Z", "g1"),
            makeMessage(3, "2024-01-01T00:00:02Z", "g2"),
            makeMessage(4, "2024-01-01T00:00:03Z", "g2b", "g2"),
        ];

        const result = getLatestBranchMessages(messages);
        expect(result.map((msg) => msg.id)).toEqual([1, 2, 4]);
    });

    it("Given repeated group ids, when selecting latest branch, then keeps only the newest group entry", () => {
        const messages = [
            makeMessage(1, "2024-01-01T00:00:00Z"),
            makeMessage(2, "2024-01-01T00:00:01Z", "g1"),
            makeMessage(3, "2024-01-01T00:00:02Z", "g1"),
        ];

        const result = getLatestBranchMessages(messages);
        expect(result.map((msg) => msg.id)).toEqual([1, 3]);
    });

    it("Given a parent group id not present, when selecting latest branch, then keeps all messages", () => {
        const messages = [
            makeMessage(1, "2024-01-01T00:00:00Z"),
            makeMessage(2, "2024-01-01T00:00:01Z", "g1"),
            makeMessage(3, "2024-01-01T00:00:02Z", "g2", "missing"),
        ];

        const result = getLatestBranchMessages(messages);
        expect(result.map((msg) => msg.id)).toEqual([1, 2, 3]);
    });

    it("Given equal timestamps, when selecting latest branch, then uses id order", () => {
        const messages = [
            makeMessage(2, "2024-01-01T00:00:00Z"),
            makeMessage(1, "2024-01-01T00:00:00Z"),
            makeMessage(3, "2024-01-01T00:00:00Z", "g1"),
        ];

        const result = getLatestBranchMessages(messages);
        expect(result.map((msg) => msg.id)).toEqual([1, 2, 3]);
    });

    it("Given a long conversation with mid regeneration, when selecting latest branch, then truncates tail", () => {
        const messages: TestMessage[] = [makeMessage(1, makeTime(0))];
        let id = 2;
        let sec = 1;
        for (let i = 1; i <= 10; i += 1) {
            messages.push(makeMessage(id, makeTime(sec)));
            id += 1;
            sec += 1;
            messages.push(makeMessage(id, makeTime(sec), `g${i}`));
            id += 1;
            sec += 1;
        }
        const regenId = id;
        messages.push(makeMessage(regenId, makeTime(sec), "g5b", "g5"));

        const result = getLatestBranchMessages(messages);
        expect(result.map((msg) => msg.id)).toEqual([
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            regenId,
        ]);
    });

    it("Given regeneration then follow-up messages, when selecting latest branch, then keeps new tail", () => {
        const messages = [
            makeMessage(1, makeTime(0)),
            makeMessage(2, makeTime(1)),
            makeMessage(3, makeTime(2), "g1"),
            makeMessage(4, makeTime(3)),
            makeMessage(5, makeTime(4), "g2"),
            makeMessage(6, makeTime(5), "g2b", "g2"),
            makeMessage(7, makeTime(6)),
            makeMessage(8, makeTime(7), "g3"),
        ];

        const result = getLatestBranchMessages(messages);
        expect(result.map((msg) => msg.id)).toEqual([1, 2, 3, 4, 6, 7, 8]);
    });
});
