import React from "react";
import { Sparkles } from "lucide-react";

interface MessageSkillAttachmentProps {
    skill_name?: string;
    invocation?: string;
    identifier?: string;
}

const MessageSkillAttachment: React.FC<MessageSkillAttachmentProps> = ({
    skill_name,
    invocation,
    identifier,
}) => {
    const displayName = skill_name || invocation || identifier || "未命名技能";

    return (
        <span className="mt-2 inline-flex min-w-[220px] max-w-full items-start gap-3 rounded-lg border border-border bg-muted/40 px-4 py-3 text-left text-foreground shadow-sm">
            <span className="mt-0.5 rounded-full bg-background p-1 text-muted-foreground">
                <Sparkles size={14} />
            </span>
            <span className="min-w-0">
                <span className="block truncate text-sm font-medium text-foreground">
                    {displayName}
                </span>
                {identifier ? (
                    <span className="mt-1 block truncate text-[11px] text-muted-foreground">
                        {identifier}
                    </span>
                ) : null}
            </span>
        </span>
    );
};

export default MessageSkillAttachment;
