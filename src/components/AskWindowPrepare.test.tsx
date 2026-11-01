import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import AskWindowPrepare from "./AskWindowPrepare";

describe("AskWindowPrepare first-use guidance", () => {
    it("leads with click-to-send copy and folds bang hints behind help", () => {
        render(<AskWindowPrepare selectedText="" isMobile />);
        expect(screen.getByText(/点击发送/)).toBeInTheDocument();
        expect(screen.queryByText("!s")).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: /高级提示/ }));
        expect(screen.getByText("!s")).toBeInTheDocument();
    });

    it("shows setup entry points when unconfigured", () => {
        const onProviders = vi.fn();
        const onAssistant = vi.fn();
        render(
            <AskWindowPrepare
                selectedText=""
                isMobile
                showSetupGuides
                onConfigureProviders={onProviders}
                onChooseAssistant={onAssistant}
            />,
        );
        fireEvent.click(screen.getByRole("button", { name: "配置模型提供商" }));
        fireEvent.click(screen.getByRole("button", { name: "选择助手" }));
        expect(onProviders).toHaveBeenCalledOnce();
        expect(onAssistant).toHaveBeenCalledOnce();
    });
});
