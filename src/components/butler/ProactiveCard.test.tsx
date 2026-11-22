import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import ProactiveCard from "./ProactiveCard";
import { type Delivery } from "./proactiveApi";

const { invoke } = vi.hoisted(() => ({invoke:vi.fn()}));
vi.mock("@tauri-apps/api/core", () => ({invoke}));
vi.mock("@/components/mcp-tool-components/MessagePreviewToolCall", () => ({default:()=> <p>指定的最终简报</p>}));
const data:Delivery={origin:"butler_proactive",role:"result",kind:"proposal",revision:2,uuid:"abc",delivery:4,item:5,source:10,position:11,stage:"due"};
beforeEach(()=>{invoke.mockReset();});
afterEach(cleanup);

it("should show the confirmed target and instruction before dispatching and opening its receipt",async()=>{
    invoke.mockImplementation(async (cmd, args)=>{
        if(cmd==="get_butler_proactive_card") return {item:5,delivery:4,state:"active",status:"delivered"};
        return args.confirmedAssistant ? {status:"sent",conversation:42,message:90} :
            {status:"confirm",assistant:8,revision:2,name:"健身教练",instruction:"安排周五复查"};
    });
    const open=vi.fn();
    window.addEventListener("butler-proactive-open",open);
    try {
        render(<ProactiveCard data={data} conversation={10} message={12}/>);
        expect(await screen.findByText(/健身教练.*安排周五复查/)).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button",{name:"采纳并执行"}));
        await waitFor(()=>expect(open).toHaveBeenCalledOnce());
        expect((open.mock.calls[0][0] as CustomEvent).detail).toEqual({conversation:42,position:90});
        expect(invoke).toHaveBeenCalledWith("accept_butler_proactive_proposal",{id:5,revision:2,confirmedAssistant:8,confirmedInstruction:"安排周五复查"});
    } finally {window.removeEventListener("butler-proactive-open",open);}
});

it("should open the linked brief and stop the delivered occurrence when dismissed",async()=>{
    invoke.mockResolvedValue({item:5,delivery:4,state:"active",status:"delivered"});
    render(<ProactiveCard data={{...data,kind:"prep_topic",brief:9}} conversation={10} message={12}/>);
    fireEvent.click(await screen.findByRole("button",{name:"查看简报"}));
    expect(screen.getByText("指定的最终简报")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button",{name:"不再跟进"}));
    expect(await screen.findByText("已停止跟进")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("update_butler_proactive_item",expect.objectContaining({id:5,action:"dismiss",deliveryId:4}));
});

it("should hide local controls when a synced message has no matching delivery",async()=>{
    invoke.mockResolvedValue(null);
    render(<ProactiveCard data={data} conversation={33} message={8}/>);
    expect(await screen.findByText("此提醒可在发出它的设备上管理")).toBeInTheDocument();
    expect(screen.queryByRole("button",{name:"采纳并执行"})).not.toBeInTheDocument();
});

it("should display a replaced instruction and wait for a second confirmation", async () => {
    const confirmed: string[] = [];
    invoke.mockImplementation(async (cmd, args) => {
        if (cmd === "get_butler_proactive_card") return {item:5, delivery:4, state:"active", status:"delivered"};
        if (args.confirmedAssistant) confirmed.push(args.confirmedInstruction);
        return {status:"confirm", assistant:8, revision:2, name:"教练", instruction:confirmed.length ? "预约复诊" : "提醒复诊"};
    });
    render(<ProactiveCard data={data} conversation={10} message={12}/>);
    expect(await screen.findByText(/教练.*提醒复诊/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button",{name:"采纳并执行"}));
    expect(await screen.findByText(/教练.*预约复诊/)).toBeInTheDocument();
    expect(confirmed).toEqual(["提醒复诊"]);
    fireEvent.click(screen.getByRole("button",{name:"采纳并执行"}));
    await waitFor(() => expect(confirmed).toEqual(["提醒复诊","预约复诊"]));
});
