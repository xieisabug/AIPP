import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Message } from "@/data/Conversation";
import { useMessageGroups } from "@/hooks/useMessageGroups";
import { useMessageProcessing } from "@/hooks/useMessageProcessing";

function makeMessage(
    id: number,
    messageType: Message["message_type"],
    createdTime: string,
    generationGroupId: string | null = null,
): Message {
    return {
        id,
        conversation_id: 1,
        message_type: messageType,
        content: `${messageType}-${id}`,
        llm_model_id: null,
        created_time: new Date(createdTime),
        start_time: null,
        finish_time: null,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: generationGroupId,
        parent_group_id: null,
        parent_id: null,
        regenerate: null,
    };
}

function makeGenerationGroups(messages: Message[]): Map<string, any> {
    const groups = new Map<string, any>();
    for (const message of messages) {
        if (!message.generation_group_id) {
            continue;
        }
        groups.set(message.generation_group_id, {
            versions: [
                {
                    versionId: message.generation_group_id,
                    messages: messages.filter(
                        (candidate) =>
                            candidate.generation_group_id === message.generation_group_id,
                    ),
                    timestamp: new Date(message.created_time),
                },
            ],
        });
    }
    return groups;
}

describe("useMessageProcessing ordering", () => {
    const messages = [
        makeMessage(1, "user", "2024-01-01T00:00:00.000Z"),
        makeMessage(2, "response", "2024-01-01T00:00:01.000Z", "g1"),
        makeMessage(3, "response", "2024-01-01T00:00:02.000Z", "g2"),
        makeMessage(4, "reasoning", "2024-01-01T00:00:02.000Z", "g2"),
        makeMessage(5, "response", "2024-01-01T00:00:02.000Z", "g3"),
        makeMessage(6, "reasoning", "2024-01-01T00:00:02.000Z", "g4"),
    ];

    it("keeps chronological order when no generation groups are provided", () => {
        const { result } = renderHook(() =>
            useMessageProcessing({
                messages,
                streamingMessages: new Map(),
                conversation: undefined,
                generationGroups: new Map(),
                groupRootMessageIds: new Map(),
                getMessageVersionInfo: () => ({ shouldShow: true }),
            }),
        );

        expect(result.current.allDisplayMessages.map((message) => message.id)).toEqual([
            1, 2, 4, 3, 5, 6,
        ]);
    });

    it("keeps the same order in the grouped display path", () => {
        const generationGroups = makeGenerationGroups(messages);
        const groupRootMessageIds = new Map<string, number>([
            ["g1", 2],
            ["g2", 3],
            ["g3", 5],
            ["g4", 6],
        ]);

        const { result } = renderHook(() =>
            useMessageProcessing({
                messages,
                streamingMessages: new Map(),
                conversation: undefined,
                generationGroups,
                groupRootMessageIds,
                getMessageVersionInfo: () => ({ shouldShow: true }),
            }),
        );

        expect(result.current.allDisplayMessages.map((message) => message.id)).toEqual([
            1, 2, 4, 3, 5, 6,
        ]);
    });

    it("keeps Butler user and error messages after filtering the system prompt", () => {
        const butlerMessages = [
            makeMessage(9990, "system", "2026-07-18T12:26:29.268Z"),
            makeMessage(9991, "user", "2026-07-18T12:26:29.282Z"),
            makeMessage(
                9992,
                "error",
                "2026-07-18T12:26:43.940Z",
                "89703fd7-bac3-4744-804a-8b6d087a56a1",
            ),
        ];
        const groupMergeMap = new Map<string, string>();

        const { result } = renderHook(() => {
            const initial = useMessageProcessing({
                messages: butlerMessages,
                streamingMessages: new Map(),
                conversation: undefined,
                generationGroups: new Map(),
                groupRootMessageIds: new Map(),
                getMessageVersionInfo: () => ({ shouldShow: true }),
            });
            const groups = useMessageGroups({
                allDisplayMessages: initial.combinedMessagesForGrouping,
                groupMergeMap,
            });

            return useMessageProcessing({
                messages: butlerMessages,
                streamingMessages: new Map(),
                conversation: undefined,
                generationGroups: groups.generationGroups,
                groupRootMessageIds: groups.groupRootMessageIds,
                getMessageVersionInfo: groups.getMessageVersionInfo,
            });
        });

        expect(result.current.allDisplayMessages.map((message) => message.id)).toEqual([
            9991, 9992,
        ]);
    });
});
