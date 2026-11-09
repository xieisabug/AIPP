import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConversationStatsDialog } from "./ConversationStatsDialog";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

const baseStats = {
    total_tokens: 0, input_tokens: 0, output_tokens: 0, thought_tokens: 0,
    cached_input_tokens: 0, cached_read_tokens: 0, cached_write_tokens: 0,
    estimated_message_count: 0, by_model: [], message_count: 0,
    system_message_count: 0, user_message_count: 0, response_message_count: 0,
    reasoning_message_count: 0, tool_result_message_count: 0, error_message_count: 0,
    other_message_count: 0, avg_ttft_ms: null, avg_tps: null, ttft_sample_count: 0,
    tps_sample_count: 0, start_time: null, finish_time: null, auxiliary: [], context: null,
    delegated: {
        run_count: 0, total_tokens: 0, input_tokens: 0, output_tokens: 0, thought_tokens: 0,
        cached_input_tokens: 0, cached_read_tokens: 0, cached_write_tokens: 0, estimated_message_count: 0,
    },
};

async function openDialog(props: Parameters<typeof ConversationStatsDialog>[0] = { conversationId: "42" }) {
    render(<ConversationStatsDialog {...props} />);
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => {
        expect(screen.queryByText("加载统计中...")).toBeNull();
    });
}

describe("ConversationStatsDialog", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("shows estimated badge and ACP session usage", async () => {
        mockInvokeHandler("get_conversation_token_stats", () => ({
            ...baseStats,
            total_tokens: 500,
            input_tokens: 180,
            output_tokens: 220,
            thought_tokens: 80,
            cached_input_tokens: 15,
            cached_read_tokens: 15,
            cached_write_tokens: 5,
            estimated_message_count: 2,
            opencode_go_allowance: 0.00000927,
            by_model: [
                {
                    model_id: 0,
                    model_name: "ACP",
                    total_tokens: 500,
                    input_tokens: 180,
                    output_tokens: 220,
                    thought_tokens: 80,
                    cached_input_tokens: 15,
                    cached_read_tokens: 15,
                    cached_write_tokens: 5,
                    request_count: 3,
                },
            ],
            message_count: 3,
            user_message_count: 1,
            response_message_count: 1,
            reasoning_message_count: 1,
        }));

        mockInvokeHandler("get_acp_session_state", () => ({
            conversation_id: 42,
            session_id: "session-1",
            title: null,
            updated_at: null,
            load_session_supported: true,
            session_resume_supported: true,
            restored_session_method: "resume",
            prompt_capabilities: { image: true, audio: false, embedded_context: true },
            current_mode_id: null,
            modes: [],
            config_options: [],
            plan: [],
            available_commands: [],
            has_active_prompt: false,
            context_tokens_used: 2048,
            context_window_size: 8192,
            session_cost_amount: 0.1234,
            session_cost_currency: "USD",
        }));

        await openDialog();

        expect(screen.getByText("Token 用量")).toBeInTheDocument();
        expect(screen.getByText("OpenCode Go 订阅额度累计扣减")).toBeVisible();
        expect(screen.getByText("$0.00000927")).toBeVisible();
        expect(screen.getByText("含估算 2 条")).toBeInTheDocument();
        expect(screen.getAllByText("缓存命中输入").length).toBeGreaterThan(0);
        expect(screen.getByText("其中思考（含在输出里）")).toBeInTheDocument();
        expect(screen.getByText("3 次调用")).toBeInTheDocument();
        expect(screen.getByText("ACP 会话 Usage")).toBeInTheDocument();
        expect(screen.getByText("2,048")).toBeInTheDocument();
        expect(screen.getByText("8,192")).toBeInTheDocument();
        // 没有委派与辅助调用时不显示构成明细
        expect(screen.queryByText(/其中助手协作/)).toBeNull();
    });

    it("shows how the total is composed of own replies, collaboration and compaction", async () => {
        mockInvokeHandler("get_conversation_token_stats", () => ({
            ...baseStats,
            total_tokens: 8500, input_tokens: 8000, output_tokens: 500,
            message_count: 4, user_message_count: 2, response_message_count: 2,
            delegated: { ...baseStats.delegated, run_count: 7, total_tokens: 5000, input_tokens: 4900, output_tokens: 100 },
            auxiliary: [{ purpose: "context_compaction", call_count: 2, total_tokens: 1200, input_tokens: 1100, output_tokens: 100 }],
        }));
        mockInvokeHandler("get_acp_session_state", () => null);

        await openDialog();

        expect(screen.getByText("其中助手协作（7 次委派，含多层转派）")).toBeInTheDocument();
        expect(screen.getByText("5,000 tokens")).toBeInTheDocument();
        expect(screen.getByText("其中上下文压缩（2 次调用）")).toBeInTheDocument();
        expect(screen.getByText("1,200 tokens")).toBeInTheDocument();
        expect(screen.getByText("其中本会话回复")).toBeInTheDocument();
        expect(screen.getByText("2,300 tokens")).toBeInTheDocument();
    });

    it("shows a dash instead of zero when there is no performance sample", async () => {
        mockInvokeHandler("get_conversation_token_stats", () => ({ ...baseStats }));
        mockInvokeHandler("get_acp_session_state", () => null);

        await openDialog();

        expect(screen.queryByText("0ms")).toBeNull();
        expect(screen.queryByText("0.0 tok/s")).toBeNull();
        expect(screen.getAllByText("暂无数据")).toHaveLength(2);
    });

    it("shows sample counts and every message type including errors", async () => {
        mockInvokeHandler("get_conversation_token_stats", () => ({
            ...baseStats,
            avg_ttft_ms: 820, avg_tps: 42.5, ttft_sample_count: 3, tps_sample_count: 2,
            message_count: 6, user_message_count: 2, response_message_count: 2, error_message_count: 1,
            other_message_count: 1,
        }));
        mockInvokeHandler("get_acp_session_state", () => null);

        await openDialog();

        expect(screen.getByText("820ms")).toBeInTheDocument();
        expect(screen.getByText("42.5 tok/s")).toBeInTheDocument();
        expect(screen.getByText("3 次请求")).toBeInTheDocument();
        expect(screen.getByText("错误")).toBeInTheDocument();
        expect(screen.getByText("其他")).toBeInTheDocument();
    });

    it("shows the current context usage for model conversations", async () => {
        mockInvokeHandler("get_conversation_token_stats", () => ({
            ...baseStats,
            context: {
                latest_input_tokens: 64000, latest_message_id: 9, context_window_size: 128000,
                effective_input_limit: 115712, compaction_trigger: 92569, compaction_enabled: true,
                checkpoint_count: 2,
            },
        }));
        mockInvokeHandler("get_acp_session_state", () => null);

        await openDialog();

        expect(screen.getByText("当前上下文")).toBeInTheDocument();
        expect(screen.getByText("64,000 / 128,000")).toBeInTheDocument();
        expect(screen.getByText("(50.0%)")).toBeInTheDocument();
        expect(screen.getByText("超过 92,569 触发")).toBeInTheDocument();
        expect(screen.getByText("2 次")).toBeInTheDocument();
    });

    it("shows the concrete backend error when loading fails", async () => {
        mockInvokeHandler("get_conversation_token_stats", () =>
            Promise.reject("读取对话 42 的用量统计失败：database is locked"),
        );
        mockInvokeHandler("get_acp_session_state", () => null);

        await openDialog();

        expect(screen.getByText("读取对话 42 的用量统计失败：database is locked")).toBeInTheDocument();
    });

    it("loads group usage without ACP session or cache activity", async () => {
        mockInvokeHandler("get_butler_group_token_stats", () => ({
            ...baseStats,
            total_tokens: 340, input_tokens: 300, output_tokens: 40, message_count: 3,
            auxiliary: [{ purpose: "group_dispatch", call_count: 1, total_tokens: 40, input_tokens: 38, output_tokens: 2 }],
        }));

        await openDialog({ groupId: 100, showCacheActivity: true });

        expect(screen.getByText("群聊用量")).toBeInTheDocument();
        expect(screen.getByText("其中群聊调度（1 次调用）")).toBeInTheDocument();
        expect(screen.getByText("成员执行记录消息")).toBeInTheDocument();
        const commands = vi.mocked(invoke).mock.calls.map(([command]) => command);
        expect(commands).toEqual(["get_butler_group_token_stats"]);
    });
});
