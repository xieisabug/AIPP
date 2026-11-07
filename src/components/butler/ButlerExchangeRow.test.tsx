import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ButlerExchangeRow from "./ButlerExchangeRow";
import type { PeerPost } from "./butlerTimeline";

const post: PeerPost = {id:2,anchor:1,occurred_at:"2026-11-05T01:00:00Z",outgoing:false,peer:"审稿",peer_conversation:20,peer_assistant:2,text:"第二轮通过",status:"delivered",error:null,root:1};
describe("Butler exchange context", () => {
    afterEach(clearAllMockHandlers);
    it("should open the full episode in a dialog when clicking a reply receipt", async () => {
        mockInvokeHandler("get_butler_exchange_context", args => {
            expect(args).toEqual({conversationId:10,postIds:[2]});
            return [{prompt:"用户要求迭代两轮",status:"delivered",messages:[
                {sender:"总管家",recipient:"审稿",text:"第一稿"},
                {sender:"审稿",recipient:"总管家",text:"第一轮建议"},
                {sender:"总管家",recipient:"审稿",text:"第二稿"},
                {sender:"审稿",recipient:"总管家",text:post.text},
            ]}];
        });
        const view = render(<ButlerExchangeRow conversationId={10} posts={[post]}/>);
        expect(view.container).not.toHaveTextContent(post.text);
        fireEvent.click(screen.getByRole("button",{name:/消息来自/}));
        const dialog = await screen.findByRole("dialog");
        expect(await within(dialog).findByText(post.text)).toBeVisible();
        expect(within(dialog).getByText("用户要求迭代两轮")).toBeVisible();
        expect(within(dialog).getByText("第一稿")).toBeVisible();
        expect(view.container).not.toHaveTextContent("第一稿");
        fireEvent.click(within(dialog).getByRole("button",{name:"Close"}));
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
        expect(view.container).not.toHaveTextContent(post.text);
    });
    it("should show a direct report in full under the reporting assistant's name", () => {
        const view = render(<ButlerExchangeRow conversationId={10} posts={[{...post,direct:true,text:"核查结论：未发现 6.2"}]}/>);
        expect(view.container).toHaveTextContent("审稿");
        expect(view.container).toHaveTextContent("直接答复");
        expect(view.container).toHaveTextContent("核查结论：未发现 6.2");
        expect(screen.getByRole("button",{name:"查看往来"})).toBeVisible();
    });
    it("should show a task delivered in the executor's own chat under the executor's identity", () => {
        const delivered = {...post,outgoing:true,peer:"总管家",task_id:30,task_reply:true,direct:true,
            speaker:"问候助手",speaker_assistant:2,text:"早安！"};
        const view = render(<ButlerExchangeRow conversationId={20} posts={[delivered]}/>);
        expect(view.container).toHaveTextContent("问候助手");
        expect(view.container).toHaveTextContent("交付任务结果");
        expect(view.container).not.toHaveTextContent("总管家");
    });
    it("should tell handed-over work apart from internal help requests", () => {
        const handed = render(<ButlerExchangeRow conversationId={20} posts={[{...post,peer:"总管家",to_user:true}]}/>);
        expect(handed.container).toHaveTextContent("接手工作，来自");
        handed.unmount();
        const help = render(<ButlerExchangeRow conversationId={20} posts={[{...post,peer:"写作",to_user:false}]}/>);
        expect(help.container).toHaveTextContent("协助请求来自");
    });
    it("should keep technical failure details out of both the receipt and the dialog", async () => {
        mockInvokeHandler("get_butler_exchange_context", () => [{prompt:"问早安",status:"failed",messages:[]}]);
        const view = render(<ButlerExchangeRow conversationId={10} posts={[{...post,status:"failed",error:"401 Unauthorized"}]}/>);
        expect(view.container).not.toHaveTextContent("401");
        fireEvent.click(screen.getByRole("button",{name:/的对话/}));
        expect(await screen.findByText("本次对话未完成")).toBeVisible();
        expect(screen.queryByText(/401/)).not.toBeInTheDocument();
    });
});
