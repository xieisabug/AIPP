/**
 * 前端测试基础设施验证测试
 *
 * 这个文件用于验证测试环境配置是否正确
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import React from "react";
import {
    invoke,
    mockInvokeHandler,
    clearAllMockHandlers,
} from "./mocks/tauri";

// 简单的测试组件
function TestButton({
    onClick,
    children,
}: {
    onClick: () => void;
    children: React.ReactNode;
}) {
    return <button onClick={onClick}>{children}</button>;
}

function TestCounter() {
    const [count, setCount] = React.useState(0);
    return (
        <div>
            <span data-testid="count">{count}</span>
            <button onClick={() => setCount((c) => c + 1)}>增加</button>
        </div>
    );
}

describe("测试基础设施验证", () => {
    describe("Vitest 基本功能", () => {
        it("应该能够运行基本断言", () => {
            expect(1 + 1).toBe(2);
            expect("hello").toContain("ell");
            expect([1, 2, 3]).toHaveLength(3);
        });

        it("应该能够使用 async/await", async () => {
            const result = await Promise.resolve("async works");
            expect(result).toBe("async works");
        });

        it("应该能够使用 vi.fn() mock 函数", () => {
            const mockFn = vi.fn();
            mockFn("arg1", "arg2");

            expect(mockFn).toHaveBeenCalled();
            expect(mockFn).toHaveBeenCalledWith("arg1", "arg2");
        });
    });

    describe("Testing Library 功能", () => {
        it("应该能够渲染 React 组件", () => {
            render(<TestButton onClick={() => {}}>Click me</TestButton>);

            expect(
                screen.getByRole("button", { name: /click me/i })
            ).toBeInTheDocument();
        });

        it("应该能够处理用户事件", async () => {
            const user = userEvent.setup();
            const handleClick = vi.fn();

            render(<TestButton onClick={handleClick}>点击</TestButton>);

            await user.click(screen.getByRole("button", { name: /点击/i }));

            expect(handleClick).toHaveBeenCalledTimes(1);
        });

        it("应该能够测试状态变化", async () => {
            const user = userEvent.setup();

            render(<TestCounter />);

            expect(screen.getByTestId("count")).toHaveTextContent("0");

            await user.click(screen.getByRole("button", { name: /增加/i }));

            expect(screen.getByTestId("count")).toHaveTextContent("1");
        });
    });

    describe("Tauri Mock 功能", () => {
        afterEach(() => {
            clearAllMockHandlers();
            vi.clearAllMocks();
        });

        it("应该能够 mock invoke 调用", async () => {
            mockInvokeHandler("test_command", () => ({
                success: true,
                data: "test data",
            }));

            const result = await invoke("test_command");

            expect(result).toEqual({ success: true, data: "test data" });
        });

        it("应该能够 mock 带参数的 invoke 调用", async () => {
            mockInvokeHandler("get_item", (args) => ({
                id: args?.id,
                name: `Item ${args?.id}`,
            }));

            const result = await invoke("get_item", { id: 42 });

            expect(result).toEqual({ id: 42, name: "Item 42" });
        });

        it("应该在没有 handler 时返回空对象", async () => {
            const result = await invoke("unknown_command");

            expect(result).toEqual({});
        });

        it("invoke 应该被记录调用次数", async () => {
            mockInvokeHandler("tracked_command", () => "tracked");

            await invoke("tracked_command");
            await invoke("tracked_command");

            expect(invoke).toHaveBeenCalledTimes(2);
        });
    });

    describe("jest-dom 扩展匹配器", () => {
        it("应该支持 toBeInTheDocument", () => {
            render(<div data-testid="element">Hello</div>);

            expect(screen.getByTestId("element")).toBeInTheDocument();
        });

        it("应该支持 toHaveTextContent", () => {
            render(<div data-testid="element">Hello World</div>);

            expect(screen.getByTestId("element")).toHaveTextContent(
                "Hello World"
            );
        });

        it("应该支持 toBeVisible", () => {
            render(<div data-testid="element">Visible</div>);

            expect(screen.getByTestId("element")).toBeVisible();
        });

        it("应该支持 toBeDisabled", () => {
            render(<button disabled>Disabled Button</button>);

            expect(
                screen.getByRole("button", { name: /disabled/i })
            ).toBeDisabled();
        });
    });
});
