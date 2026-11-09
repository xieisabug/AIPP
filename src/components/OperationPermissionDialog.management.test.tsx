import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { OperationPermissionDialog } from "./OperationPermissionDialog";
describe("management approval",()=>{
 it("should offer only one-time approval when action parameters require consent",()=>{
  const decide=vi.fn();
  render(<OperationPermissionDialog request={{request_id:"one",operation:"superadmin_execute",path:'{"recipient":"example","resource":"file"}',conversation_id:7}} isOpen onDecision={decide}/>);
  expect(screen.getByText(/5分钟后失效/)).toBeTruthy();
  expect(screen.queryByText("对话期间信任")).toBeNull();
  expect(screen.queryByText("添加到助手工作区")).toBeNull();
  expect(screen.queryByText("允许并加入全局白名单")).toBeNull();
  fireEvent.click(screen.getByText("仅本次允许"));expect(decide).toHaveBeenCalledWith("one","allow");
 });
});
