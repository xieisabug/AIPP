import { Loader2 } from "lucide-react";
import { useDisplayConfig } from "@/hooks/useDisplayConfig";
import ReasoningMessage, { type ReasoningMessageProps } from "./ReasoningMessage";

/** Shared display policy for API reasoning messages and native Agent thinking. */
export function ReasoningDisplay(props: ReasoningMessageProps) {
  const { isShowThinking } = useDisplayConfig();
  if (isShowThinking) return <ReasoningMessage {...props} />;
  if (props.message.finish_time !== null || props.streamEvent?.is_done) return null;
  return (
    <div
      {...(props.embedded ? { "data-agent-reasoning": true } : { "data-message-item": true, "data-message-id": props.message.id })}
      data-message-type="reasoning"
      className="my-2 flex items-center gap-2 px-3 py-1.5"
    >
      <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
      <span className="text-xs text-muted-foreground">思考中...</span>
    </div>
  );
}
