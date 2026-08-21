import type { AgentActivityEvent } from "@/data/Conversation";

export interface ActivitySegment {
    /** 该分段内的正文文本（可能为空字符串） */
    text: string;
    /** 位于该分段之后的活动卡片 */
    activities: AgentActivityEvent[];
}

/**
 * 按 content_offset 把正文切成若干段，活动卡片穿插在对应分段之后。
 *
 * offset 语义：item 开始时已输出的正文字符数（Unicode 字符计，与 Rust chars().count() 一致）。
 * 没有有效 offset 的活动（旧数据）统一挂到正文末尾。
 * 全部活动都没有有效 offset 时返回 null，调用方回退为「正文 + 列表」的旧布局。
 */
export function buildActivitySegments(
    content: string,
    activities: AgentActivityEvent[],
): ActivitySegment[] | null {
    if (activities.length === 0) return null;

    const codePoints = Array.from(content);
    const withOffset: AgentActivityEvent[] = [];
    const tailActivities: AgentActivityEvent[] = [];
    for (const activity of activities) {
        if (
            typeof activity.content_offset === "number"
            && activity.content_offset >= 0
            && activity.content_offset <= codePoints.length
        ) {
            withOffset.push(activity);
        } else {
            tailActivities.push(activity);
        }
    }
    if (withOffset.length === 0) return null;

    withOffset.sort(
        (a, b) => (a.content_offset as number) - (b.content_offset as number)
            || a.sequence - b.sequence,
    );

    const segments: ActivitySegment[] = [];
    let prevOffset = 0;
    let index = 0;
    while (index < withOffset.length) {
        const offset = withOffset[index].content_offset as number;
        const group: AgentActivityEvent[] = [];
        while (index < withOffset.length && withOffset[index].content_offset === offset) {
            group.push(withOffset[index]);
            index += 1;
        }
        segments.push({
            text: codePoints.slice(prevOffset, offset).join(""),
            activities: group,
        });
        prevOffset = offset;
    }
    segments.push({
        text: codePoints.slice(prevOffset).join(""),
        activities: tailActivities,
    });
    return segments;
}
