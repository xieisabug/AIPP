import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentPlanCard } from "./AgentPlanCard";

describe("AgentPlanCard", () => {
    it("renders the full Markdown plan body", () => {
        render(
            <AgentPlanCard
                content={"# 实施方案\n\n1. 修改后端\n2. 更新界面"}
                isStreaming={false}
                canAct={false}
                modeSwitching={false}
            />
        );

        expect(screen.getByRole("heading", { name: "实施方案" })).toBeInTheDocument();
        expect(screen.getByText("修改后端")).toBeInTheDocument();
    });

    it("offers actions only when the completed plan can act", () => {
        const onContinuePlanning = vi.fn();
        const onApprove = vi.fn();
        render(
            <AgentPlanCard
                content="Plan body"
                isStreaming={false}
                canAct
                modeSwitching={false}
                onContinuePlanning={onContinuePlanning}
                onApprove={onApprove}
            />
        );

        fireEvent.click(screen.getByRole("button", { name: "继续完善" }));
        fireEvent.click(screen.getByRole("button", { name: "通过" }));
        expect(onContinuePlanning).toHaveBeenCalledOnce();
        expect(onApprove).toHaveBeenCalledOnce();
    });

    it("disables approval while switching modes", () => {
        render(
            <AgentPlanCard
                content="Plan body"
                isStreaming={false}
                canAct
                modeSwitching
                onApprove={vi.fn()}
            />
        );

        expect(screen.getByRole("button", { name: "切换中" })).toBeDisabled();
    });
});
