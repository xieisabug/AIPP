import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ButlerContactList from "./ButlerContactList";

describe("ButlerContactList", () => {
    it("should open settings from the top-left of the contact list", () => {
        const onOpenSettings = vi.fn();
        render(<ButlerContactList
            chief="总管家"
            contacts={[]}
            ready
            previews={[]}
            plugins={[]}
            error=""
            onRefresh={() => {}}
            onSelect={() => {}}
            onSearch={() => {}}
            onOpenSettings={onOpenSettings}
            onAssistantAction={() => {}}
            groups={[]}
            onSelectGroup={() => {}}
            onCreateGroup={() => {}}
            onDeleteGroup={() => {}}
            activity={{ visuals: new Map(), approvals: new Set(), error: "", acknowledge: undefined }}
        />);

        const settings = screen.getByRole("button", { name: "总管家设置" });
        const search = screen.getByRole("button", { name: "搜索管家消息" });
        expect(settings.compareDocumentPosition(search) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        expect(settings).not.toBeDisabled();

        fireEvent.click(settings);
        expect(onOpenSettings).toHaveBeenCalledOnce();
    });
});
