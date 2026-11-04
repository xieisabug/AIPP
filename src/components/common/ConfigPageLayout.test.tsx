import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ConfigPageLayout, { SelectOption } from "./ConfigPageLayout";
import { backConfigSection, ConfigSectionContext, useConfigSectionDetailTitle } from "@/lib/configSectionNav";

const platform = vi.hoisted(() => ({ mobile: true }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => platform.mobile }));

const options: SelectOption[] = [
    { id: "a", label: "显示", description: "外观主题" },
    { id: "b", label: "网络配置", trailing: <button type="button">开关</button> },
];

function TitleProbe() {
    const title = useConfigSectionDetailTitle("feature");
    return <span data-testid="detail-title">{title ?? "none"}</span>;
}

function Harness({ onSelect = vi.fn(), selected = "a", detailRequest }: { onSelect?: (id: string) => void; selected?: string; detailRequest?: number }) {
    return (
        <ConfigSectionContext.Provider value="feature">
            <TitleProbe />
            <ConfigPageLayout
                sidebar={<div>sidebar</div>}
                content={<div>detail content</div>}
                selectOptions={options}
                selectedOptionId={selected}
                onSelectOption={onSelect}
                listSearchable={false}
                mobileDetailRequest={detailRequest}
            />
        </ConfigSectionContext.Provider>
    );
}

describe("ConfigPageLayout mobile list → detail", () => {
    afterEach(() => { platform.mobile = true; });

    it("should render a list instead of a select when on mobile", () => {
        render(<Harness />);
        expect(screen.getByText("外观主题")).toBeTruthy();
        expect(screen.queryByText("detail content")).toBeNull();
        expect(screen.queryByRole("combobox")).toBeNull();
    });

    it("should open detail and publish its title when a row is tapped", () => {
        const onSelect = vi.fn();
        render(<Harness onSelect={onSelect} />);
        fireEvent.click(screen.getByText("显示"));
        expect(onSelect).toHaveBeenCalledWith("a");
        expect(screen.getByText("detail content")).toBeTruthy();
        expect(screen.getByTestId("detail-title").textContent).toBe("显示");
    });

    it("should return to the list when the section back is consumed", () => {
        render(<Harness />);
        fireEvent.click(screen.getByText("显示"));
        let consumed = false;
        act(() => { consumed = backConfigSection("feature"); });
        expect(consumed).toBe(true);
        expect(screen.queryByText("detail content")).toBeNull();
        expect(screen.getByTestId("detail-title").textContent).toBe("none");
        expect(backConfigSection("feature")).toBe(false);
    });

    it("should not open detail when the trailing control is tapped", () => {
        const onSelect = vi.fn();
        render(<Harness onSelect={onSelect} />);
        fireEvent.click(screen.getByText("开关"));
        expect(onSelect).not.toHaveBeenCalled();
        expect(screen.queryByText("detail content")).toBeNull();
    });

    it("should open detail when a detail request arrives", () => {
        const { rerender } = render(<Harness />);
        rerender(<Harness detailRequest={1} />);
        expect(screen.getByText("detail content")).toBeTruthy();
    });
});
