import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import ButlerCheckPolicy from "./ButlerCheckPolicy";
const {invoke}=vi.hoisted(()=>({invoke:vi.fn()}));
vi.mock("@tauri-apps/api/core",()=>({invoke}));
beforeEach(()=>invoke.mockReset().mockResolvedValue(null));
it("未配置时不能启用，必须明确来源和截止时间",async()=>{
 render(<ButlerCheckPolicy taskId={4}/>);
 expect((screen.getByText("确认范围并启用") as HTMLButtonElement).disabled).toBe(true);
 fireEvent.change(screen.getByLabelText("检查来源"),{target:{value:"https://example.com/status"}});
 fireEvent.change(screen.getByLabelText("检查截止时间"),{target:{value:"2027-01-01T00:00:00+08:00"}});
 fireEvent.click(screen.getByText("确认范围并启用"));
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith("configure_butler_check",{taskId:4,policy:{mode:"read_only",sources:["https://example.com/status"],expires_at:"2027-01-01T00:00:00+08:00",paused:false,stopped:false,stop_current_on_pause:false}}));
});
it("关闭后不展示自动恢复入口",async()=>{
 invoke.mockResolvedValue({mode:"draft",sources:["/tmp/a"],expires_at:"2027-01-01T00:00:00Z",paused:true,stopped:true,stop_current_on_pause:false});
 render(<ButlerCheckPolicy taskId={5}/>);
 await screen.findByText(/已关闭/);
 expect(screen.queryByText("恢复检查")).toBeNull();
});
