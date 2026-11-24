import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { SquareArrowOutUpRight } from "lucide-react";
import { toast } from "sonner";
import { usePlatform } from "@/hooks/use-platform";
import IconButton from "./IconButton";

export default function InteractivePowerShellButton({ code }: { code: string }) {
    const platform = usePlatform();
    if (platform !== "windows") return null;

    const openTerminal = async () => {
        try {
            await writeText(code);
            await invoke("open_interactive_powershell");
        } catch (error) {
            toast.error(`复制脚本并打开 PowerShell 失败：${error}`);
        }
    };

    return <IconButton title="复制脚本并打开 PowerShell" icon={<SquareArrowOutUpRight size={16} />} onClick={openTerminal} />;
}
