import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import ButlerGroups from "./ButlerGroups";

const api = vi.hoisted(() => ({ listButlerGroups:vi.fn(), getButlerGroup:vi.fn(), listButlerGroupMessages:vi.fn(),
    postButlerGroupMessage:vi.fn(), createButlerGroup:vi.fn(), onButlerGroupChanged:vi.fn(),replaceButlerGroupGoal:vi.fn(),getButlerGroupMemberCapabilities:vi.fn() }));
vi.mock("@/services/butlerGroupService", () => api);
vi.mock("@tauri-apps/api/event", () => ({ listen:vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/hooks/useInlineInteraction", () => ({ useAskUserQuestion:() => ({ isDialogOpen:false }) }));
const elicitation = vi.hoisted(() => vi.fn(() => ({ questionRequest:null })));
vi.mock("@/hooks/useAcpElicitation", () => ({ useAcpElicitation:elicitation }));
vi.mock("@/hooks/useOperationPermission", () => ({ useOperationPermission:() => ({ isDialogOpen:false }), useAcpPermission:() => ({ isDialogOpen:false }) }));
vi.mock("@/components/OperationPermissionDialog", () => ({ OperationPermissionDialog:() => null, AcpPermissionDialog:() => null }));
const members = [
    { id:10,group_id:1,assistant_id:1,name:"总管家",conversation_id:100,active:true,joined_after:0,delivered_through:0 },
    { id:20,group_id:1,assistant_id:2,name:"研究员",conversation_id:200,active:true,joined_after:0,delivered_through:0 },
    { id:30,group_id:1,assistant_id:3,name:"审稿人",conversation_id:300,active:true,joined_after:0,delivered_through:0 },
];
beforeEach(() => {
    vi.clearAllMocks();
    api.listButlerGroups.mockResolvedValue([{id:1,name:"设计讨论",state:"active"}]);
    api.getButlerGroup.mockResolvedValue({ group:{id:1,name:"设计讨论",goal:"比较方案",chief_member:10,seq:1,state:"active"},members,inbox:[],rounds:[],stop_reports:[] });
    api.listButlerGroupMessages.mockResolvedValue([{group_id:1,seq:1,sender_member:20,sender_name:"研究员",kind:"result",body:"原始意见",reply_to:null,created_at:"现在"}]);
    api.onButlerGroupChanged.mockResolvedValue(() => {});
    api.postButlerGroupMessage.mockResolvedValue({});
    api.replaceButlerGroupGoal.mockResolvedValue([]);
});

async function open() {
    render(<ButlerGroups assistants={[{id:2,name:"研究员"},{id:3,name:"审稿人"}]} />);
    fireEvent.click(await screen.findByRole("button",{name:"设计讨论"}));
    await screen.findByText("原始意见");
}

it("should preserve recipient and reply identity when replying to a public message", async () => {
    await open();
    fireEvent.click(screen.getByTitle("回复 研究员"));
    fireEvent.change(screen.getByLabelText("群消息"),{target:{value:"请说明依据"}});
    fireEvent.click(screen.getByRole("button",{name:"发送"}));
    await waitFor(() => expect(api.postButlerGroupMessage).toHaveBeenCalledWith(1,expect.objectContaining({body:"请说明依据",reply_to:1,recipients:[20]})));
});

it("should scope native questions to each member and task execution when opening a group", async () => {
    api.getButlerGroup.mockResolvedValue({group:{id:1,name:"设计讨论",goal:"比较方案",chief_member:10,seq:1,state:"active"},members,inbox:[],rounds:[],stop_reports:[],
        tasks:[{conversation_id:201,member_id:20,title:"独立调研",state:"running"}]});
    await open();
    for (const conversationId of [100,200,300,201]) {
        expect(elicitation).toHaveBeenCalledWith({conversationId});
    }
});

it("should submit ordered membership ids atomically when collecting sequential opinions", async () => {
    await open();
    fireEvent.click(screen.getByLabelText("顺序征求意见"));
    fireEvent.click(screen.getByRole("button",{name:"审稿人"}));
    fireEvent.click(screen.getByRole("button",{name:"研究员"}));
    fireEvent.change(screen.getByLabelText("群消息"),{target:{value:"依次评价"}});
    fireEvent.click(screen.getByRole("button",{name:"发送"}));
    await waitFor(() => expect(api.postButlerGroupMessage).toHaveBeenCalledWith(1,expect.objectContaining({ordered_members:[30,20],recipients:[]})));
});

it("should reuse the same request identity when retrying an uncertain send", async () => {
    api.postButlerGroupMessage.mockRejectedValueOnce(new Error("连接中断")).mockResolvedValueOnce({});
    await open();
    fireEvent.change(screen.getByLabelText("群消息"),{target:{value:"只提交一次"}});
    fireEvent.click(screen.getByRole("button",{name:"发送"}));
    await screen.findByRole("alert");
    await waitFor(() => expect(screen.getByRole("button",{name:"发送"})).not.toBeDisabled());
    fireEvent.click(screen.getByRole("button",{name:"发送"}));
    await waitFor(() => expect(api.postButlerGroupMessage).toHaveBeenCalledTimes(2));
    expect(api.postButlerGroupMessage.mock.calls[0]).toEqual(api.postButlerGroupMessage.mock.calls[1]);
});

it("should keep a citation without targeting its author when reference only is selected", async () => {
    await open();
    fireEvent.click(screen.getByTitle("回复 研究员"));
    fireEvent.click(screen.getByLabelText("仅引用资料，不要求原作者回应"));
    fireEvent.change(screen.getByLabelText("群消息"),{target:{value:"请总管家参考"}});
    fireEvent.click(screen.getByRole("button",{name:"发送"}));
    await waitFor(() => expect(api.postButlerGroupMessage).toHaveBeenCalledWith(1,expect.objectContaining({reference_only:true,reply_to:1,recipients:[]})));
});

it("should use independent snapshots instead of ordered delivery when independent mode is selected", async () => {
    await open();
    fireEvent.click(screen.getByLabelText("顺序征求意见"));
    fireEvent.click(screen.getByLabelText("各自独立判断"));
    expect(screen.getByLabelText("顺序征求意见")).not.toBeChecked();
    fireEvent.click(screen.getByRole("button",{name:"研究员"}));
    fireEvent.click(screen.getByRole("button",{name:"审稿人"}));
    fireEvent.change(screen.getByLabelText("群消息"),{target:{value:"不要互相影响"}});
    fireEvent.click(screen.getByRole("button",{name:"发送"}));
    await waitFor(()=>expect(api.postButlerGroupMessage).toHaveBeenCalledWith(1,expect.objectContaining({independent_members:[20,30],ordered_members:[],recipients:[]})));
});

it("should explicitly replace the goal without sending an ordinary message when confirmed", async () => {
    await open();
    fireEvent.click(screen.getByRole("button",{name:"替换目标"}));
    fireEvent.change(screen.getByLabelText("新的群目标"),{target:{value:"改做新目标"}});
    fireEvent.click(screen.getByRole("button",{name:"停止旧工作并替换目标"}));
    await waitFor(()=>expect(api.replaceButlerGroupGoal).toHaveBeenCalledWith(1,"改做新目标",expect.any(String)));
    expect(api.postButlerGroupMessage).not.toHaveBeenCalled();
});
