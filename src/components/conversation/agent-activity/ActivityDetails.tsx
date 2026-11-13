import { JsonDisplay } from "@/components/McpToolCall";
import type { AgentActivityEvent } from "@/data/Conversation";

/** Native data is available on demand without filling the transcript with JSON. */
export function ActivityRawDetails({ activity }: { activity: AgentActivityEvent }) {
  const metadata = activity.metadata as { rawResult?: unknown } | null;
  if (activity.input == null && metadata?.rawResult === undefined) return null;
  return (
    <details className="text-xs text-muted-foreground">
      <summary className="cursor-pointer">参数与原始结果</summary>
      <JsonDisplay
        content={JSON.stringify({ input: activity.input, result: metadata?.rawResult }, null, 2)}
        maxHeight="288px"
        className="mt-1"
      />
    </details>
  );
}

export function ActivityTextDetails({ activity }: { activity: AgentActivityEvent }) {
  return (
    <>
      {activity.output && <pre className="text-xs whitespace-pre-wrap break-words max-h-80 overflow-auto">{activity.output}</pre>}
      {activity.error && activity.error !== activity.output && (
        <div className="text-xs text-destructive whitespace-pre-wrap break-words">{activity.error}</div>
      )}
      <ActivityRawDetails activity={activity} />
    </>
  );
}
