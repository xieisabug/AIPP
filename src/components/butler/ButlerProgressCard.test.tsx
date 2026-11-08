import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import ButlerProgressCard from "./ButlerProgressCard";
import type { ButlerTaskListItem } from "@/data/Butler";
const { invoke } = vi.hoisted(()=>({invoke:vi.fn()}));
vi.mock("@tauri-apps/api/core",()=>({invoke}));
beforeEach(()=>invoke.mockReset().mockResolvedValue({paused:false,delegation_stopped:false,cancelled:false,note:""}));
const task = {task_conversation_id:12,title:"检查资料",executor_assistant_name:"研究助手",is_finalized:false,progress:{waiting_for:["你的操作批准"],blocker:null,next_step:"等待批准",evidence:[{message_id:3,label:"最终答复"}],artifacts:[]}} as unknown as ButlerTaskListItem;
it("显示真实等待与证据，并将暂停发给当前工作",async()=>{
 render(<ButlerProgressCard task={task}/>);
 expect(screen.getByText("在等：你的操作批准")).toBeTruthy();
 expect(screen.getByText("最终答复：本任务消息 #3")).toBeTruthy();
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith("get_butler_task_control",{taskConversationId:12}));
 fireEvent.click(screen.getByText("暂停后续执行"));
 await waitFor(()=>expect(invoke).toHaveBeenCalledWith("control_butler_task",{taskConversationId:12,action:"pause"}));
});
it("完成工作不提供执行按钮",()=>{
 render(<ButlerProgressCard task={{...task,is_finalized:true}}/>);
 expect(screen.queryByText("停止整项工作")).toBeNull();
});
