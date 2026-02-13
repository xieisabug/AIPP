import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { useArtifactEvents } from "@/hooks/useArtifactEvents";

type CallbackProps = {
    onArtifactData?: (data: any) => void;
    onRedirect?: (url: string) => void;
    onEnvironmentCheck?: (data: any) => void;
    onReset?: () => void;
};

function Harness({ onArtifactData, onRedirect, onEnvironmentCheck, onReset }: CallbackProps) {
    const { logs, hasReceivedData } = useArtifactEvents({
        windowType: "preview",
        onArtifactData,
        onRedirect,
        onEnvironmentCheck,
        onReset,
    });

    return (
        <div>
            <div data-testid="has-data">{hasReceivedData ? "yes" : "no"}</div>
            <div data-testid="logs">
                {logs.map((log) => `${log.type}:${log.message}`).join("|")}
            </div>
        </div>
    );
}

const flushEffects = async () => {
    await act(async () => {
        await Promise.resolve();
    });
};

describe("useArtifactEvents request_id filtering", () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });

    afterEach(() => {
        vi.clearAllTimers();
        vi.useRealTimers();
    });

    it("filters events by request_id after reset", async () => {
        const onArtifactData = vi.fn();
        const onRedirect = vi.fn();
        const onEnvironmentCheck = vi.fn();

        render(
            <Harness
                onArtifactData={onArtifactData}
                onRedirect={onRedirect}
                onEnvironmentCheck={onEnvironmentCheck}
            />
        );

        await flushEffects();

        await act(async () => {
            await emit("artifact-preview-reset", { request_id: "req-1" });
        });

        await act(async () => {
            await emit("artifact-preview-data", {
                type: "markdown",
                original_code: "alpha",
                request_id: "req-1",
            });
        });

        expect(onArtifactData).toHaveBeenCalledTimes(1);
        expect(onArtifactData.mock.calls[0][0].original_code).toBe("alpha");

        await act(async () => {
            await emit("artifact-preview-data", {
                type: "markdown",
                original_code: "stale",
                request_id: "req-2",
            });
        });

        await act(async () => {
            await emit("artifact-preview-data", {
                type: "markdown",
                original_code: "legacy",
            });
        });

        expect(onArtifactData).toHaveBeenCalledTimes(1);

        await act(async () => {
            await emit("artifact-preview-log", { message: "ok", request_id: "req-1" });
        });

        expect(screen.getByTestId("logs")).toHaveTextContent("log:ok");

        await act(async () => {
            await emit("artifact-preview-log", { message: "ignored", request_id: "req-2" });
        });

        expect(screen.getByTestId("logs")).not.toHaveTextContent("ignored");

        await act(async () => {
            await emit("artifact-preview-redirect", { url: "http://a", request_id: "req-1" });
        });

        await act(async () => {
            await emit("artifact-preview-redirect", { url: "http://b", request_id: "req-2" });
        });

        expect(onRedirect).toHaveBeenCalledTimes(1);

        await act(async () => {
            await emit("environment-check", {
                tool: "bun",
                message: "install",
                lang: "react",
                input_str: "code",
                request_id: "req-2",
            });
        });

        expect(onEnvironmentCheck).not.toHaveBeenCalled();
    });

    it("accepts legacy events before request_id is known", async () => {
        const onArtifactData = vi.fn();

        render(<Harness onArtifactData={onArtifactData} />);

        await flushEffects();

        await act(async () => {
            await emit("artifact-preview-log", "legacy log");
        });

        expect(screen.getByTestId("logs")).toHaveTextContent("log:legacy log");

        await act(async () => {
            await emit("artifact-preview-data", {
                type: "markdown",
                original_code: "legacy",
            });
        });

        await act(async () => {
            await emit("artifact-preview-data", {
                type: "markdown",
                original_code: "current",
                request_id: "req-9",
            });
        });

        await act(async () => {
            await emit("artifact-preview-data", {
                type: "markdown",
                original_code: "late legacy",
            });
        });

        expect(onArtifactData).toHaveBeenCalledTimes(2);
        expect(onArtifactData.mock.calls[0][0].original_code).toBe("legacy");
        expect(onArtifactData.mock.calls[1][0].original_code).toBe("current");
    });
});
