import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import AskWindowPrepare from "./AskWindowPrepare";

describe("AskWindowPrepare first-use guidance", () => {
    it("should toggle bang hints when on mobile", () => {
        render(<AskWindowPrepare selectedText="" isMobile />);
        expect(screen.getByText(/点击发送/)).toBeInTheDocument();
        expect(screen.queryByText("!s")).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: /高级提示/ }));
        expect(screen.getByText("!s")).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: /高级提示/ }));
        expect(screen.queryByText("!s")).not.toBeInTheDocument();
    });

    it("should show all bang hints without a toggle when on desktop", () => {
        render(<AskWindowPrepare selectedText="选中的文字" />);
        expect(screen.queryByRole("button", { name: /高级提示/ })).not.toBeInTheDocument();
        for (const bang of ["!s", "!cd", "!ct", "!w(url)", "!wm(url)", "!file(path)"]) {
            expect(screen.getByText(bang)).toBeVisible();
        }
        expect(screen.getByText("选中的文字")).toBeVisible();
        expect(screen.getByText("输入文本后回车，与快捷对话助手进行交流")).toBeVisible();
        expect(screen.queryByText(/桌面端也可按回车发送/)).not.toBeInTheDocument();
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
