import { FilePenLine, FileText, Search, FileMinus } from "lucide-react";
import type { AgentActivityEvent } from "@/data/Conversation";
import { AgentActivityShell } from "./AgentActivityShell";
import { ActivityTextDetails } from "./ActivityDetails";
import { activitySource } from "./activitySource";

export function FileActivityCard({ activity }: { activity: AgentActivityEvent }) {
  const Icon = activity.kind === "file_read" ? FileText : activity.kind === "file_write" ? FilePenLine : activity.kind === "file_delete" ? FileMinus : Search;
  return (
    <AgentActivityShell
      icon={<Icon className="h-4 w-4 flex-shrink-0" />}
      sourceLabel={activitySource(activity)}
      title={activity.title ?? "文件操作"}
      status={activity.status}
      autoExpand={false}
    >
      <ActivityTextDetails activity={activity} />
    </AgentActivityShell>
  );
}
