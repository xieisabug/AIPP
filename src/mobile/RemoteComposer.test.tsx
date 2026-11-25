import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import RemoteComposer from "./RemoteComposer";

vi.mock("@tauri-apps/api/core", () => ({invoke:vi.fn()}));
vi.mock("@/components/conversation/InputArea", () => ({default:(p:{inputText:string;setInputText:(s:string)=>void;handleSend:()=>void}) =>
    <><textarea aria-label="消息" value={p.inputText} onChange={e=>p.setInputText(e.target.value)}/><button onClick={()=>p.handleSend()}>发送或停止</button></>}));
const accepted = vi.fn();
function composer(id=7, running=false) {
    return <RemoteComposer hostId="office" target={{id,assistantId:7}} running={running} onAccepted={accepted}/>;
}
describe("paired desktop composer",() => {
    beforeEach(() => {
        const data = new Map<string,string>();
        vi.stubGlobal("localStorage",{getItem:(k:string)=>data.get(k)??null,setItem:(k:string,v:string)=>data.set(k,v),removeItem:(k:string)=>data.delete(k)});
        vi.mocked(invoke).mockReset(); accepted.mockReset();
    });
    it("should reuse the request ID after a lost reply and recover the desktop conversation",async () => {
        let key = "";
        let delivered = false;
        vi.mocked(invoke).mockImplementation(async (_,args:any) => {
            const a = args.action;
            if (a.action==="send") {
                expect(a.destination).toEqual({kind:"new",assistant:7});
                if (!key) { key=a.request; throw Error("连接中断"); }
                expect(a.request).toBe(key); delivered=true; return {};
            }
            return delivered ? {state:"accepted",conversation:20,message:30} : {state:"pending"};
        });
        render(composer(-7));
        fireEvent.change(screen.getByRole("textbox"),{target:{value:"问电脑"}});
        fireEvent.click(screen.getByText("发送或停止"));
        await screen.findByText("Error: 连接中断");
        fireEvent.click(screen.getByText("重试原消息"));
        await waitFor(()=>expect(accepted).toHaveBeenCalledWith(20),{timeout:3500});
        expect(localStorage.getItem("aipp.mobile.host.office.chat.20.last")).toBe(key);
        expect(localStorage.getItem("aipp.mobile.host.office.chat.-7.pending")).toBeNull();
    });
    it("should only check the existing receipt after remount and preserve the draft",async () => {
        localStorage.setItem("aipp.mobile.host.office.chat.7.pending",JSON.stringify({request:"previous",text:"原消息"}));
        localStorage.setItem("aipp.mobile.host.office.chat.7.draft","第二条草稿");
        vi.mocked(invoke).mockResolvedValue({state:"accepted",conversation:7,message:12});
        render(composer());
        await waitFor(()=>expect(accepted).toHaveBeenCalledWith(7));
        expect(invoke).toHaveBeenCalledTimes(1);
        expect(invoke).toHaveBeenCalledWith("bridge_connect",{id:"office",action:{action:"receipt",request:"previous"}});
        expect(screen.getByRole("textbox")).toHaveValue("第二条草稿");
    });
    it("should send a stop only for this device's accepted request",async () => {
        localStorage.setItem("aipp.mobile.host.office.chat.7.last","own-turn");
        vi.mocked(invoke).mockResolvedValue({stopped:true});
        render(composer(7,true)); fireEvent.click(screen.getByText("发送或停止"));
        expect(invoke).toHaveBeenCalledWith("bridge_connect",{id:"office",action:{action:"stop",request:"own-turn"}});
    });
});
