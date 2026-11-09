import ButlerAvatar from "./ButlerAvatar";
import type { ButlerGroupMember } from "@/data/ButlerGroup";

/** 群头像：最多拼四位成员的头像，和聊天软件的群头像一样一眼能认出是群。 */
export default function ButlerGroupAvatar({ members, size = 36 }: { members: ButlerGroupMember[]; size?: number }) {
    const shown = members.slice(0, 4);
    const cell = Math.floor((size - 6) / 2);
    return <span aria-hidden="true" data-aipp-slot="butler-group-avatar"
        className="grid shrink-0 grid-cols-2 place-content-center place-items-center gap-0.5 rounded-xl bg-muted p-[2px]"
        style={{ width: size, height: size }}>
        {shown.map(member => <ButlerAvatar key={member.id} id={member.assistant_id} chief={member.is_chief} size={cell} figure />)}
    </span>;
}
