import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ButlerContactList from "./ButlerContactList";

import { mockInvokeHandler } from "@/__tests__/mocks/tauri";

describe("ButlerContactList", () => {
    beforeEach(() => { mockInvokeHandler("get_butler_contact_order", () => []); });
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

    it("should render chief, group, and contact avatars 50% larger in compact mode", () => {
        const { container } = render(<ButlerContactList
            chief="总管家"
            contacts={[{ id: 8, name: "写作助手", assistant_type: 0 }]}
            ready
            previews={[]}
            plugins={[]}
            error=""
            onRefresh={() => {}}
            onSelect={() => {}}
            onSearch={() => {}}
            onOpenSettings={() => {}}
            onAssistantAction={() => {}}
            groups={[{
                id: 3,
                name: "协作群",
                members: [
                    { id: 1, assistant_id: 0, name: "总管家", conversation_id: 10, is_chief: true },
                    { id: 2, assistant_id: 8, name: "写作助手", conversation_id: 11, is_chief: false },
                ],
                last_message: null,
                updated_at: "2026-11-16T00:00:00Z",
                typing: [],
            }]}
            onSelectGroup={() => {}}
            onCreateGroup={() => {}}
            onDeleteGroup={() => {}}
            activity={{ visuals: new Map([[0, { state: "alerting", label: "有未读消息" }], [8, { state: "alerting", label: "有未读消息" }]]), approvals: new Set(), error: "", acknowledge: undefined }}
        />);

        const chiefAvatar = container.querySelector('[data-avatar-shape="aipp"]') as HTMLElement;
        expect(chiefAvatar).toHaveAttribute("data-avatar-state", "alerting");
        expect(chiefAvatar).toHaveStyle({ width: "66px", height: "66px" });
        expect(chiefAvatar.querySelector("svg")).toHaveAttribute("viewBox", "-12 -12 534 534");

        const groupAvatar = container.querySelector('[data-aipp-slot="butler-group-avatar"]') as HTMLElement;
        expect(groupAvatar).toHaveStyle({ width: "48px", height: "48px" });

        const contactButton = screen.getByRole("button", { name: /^写作助手/ });
        const contactAvatar = contactButton.querySelector('[data-avatar-shape="pebble"]') as HTMLElement;
        expect(contactButton.querySelectorAll('[aria-label="未读"]')).toHaveLength(1);
        expect(contactAvatar).toHaveAttribute("data-avatar-state", "alerting");
        expect(contactAvatar).toHaveStyle({ width: "48px", height: "48px" });
        expect(contactAvatar.querySelector("svg")).toHaveAttribute("viewBox", "-12 -12 534 534");
    });
});
