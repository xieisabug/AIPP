import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import type { AssistantListItem } from "@/data/Assistant";
import type { ButlerMainLoadResponse } from "@/data/Butler";

export default function MobileButlerContacts({ onSelect }: { onSelect: (id: string) => void }) {
    const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
    const [error, setError] = useState("");
    const [opening, setOpening] = useState(false);
    useEffect(() => {
        let live = true;
        invoke<AssistantListItem[]>("get_assistants").then(items => { if (live) setAssistants(items); })
            .catch(e => { if (live) setError(String(e)); });
        return () => { live = false; };
    }, []);
    const mounted = useRef(false);
    useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
    async function select(assistant?: AssistantListItem) {
        setOpening(true); setError("");
        try {
            const id = assistant ? await invoke<number>("open_butler_contact", { assistantId: assistant.id })
                : (await invoke<ButlerMainLoadResponse>("load_butler_main_conversation", { reconcile: false })).conversation.id;
            if (mounted.current) onSelect(String(id));
        } catch (e) { if (mounted.current) setError(String(e)); }
        finally { if (mounted.current) setOpening(false); }
    }
    return <div className="min-h-0 overflow-y-auto px-2">
        <Button variant="ghost" disabled={opening} className="min-h-11 w-full justify-start" onClick={() => void select()}>总管家</Button>
        {assistants.map(assistant => <Button key={assistant.id} variant="ghost" disabled={opening}
            className="min-h-11 w-full justify-start" onClick={() => void select(assistant)}>{assistant.name}</Button>)}
        {error && <p role="alert" className="px-2 text-sm text-destructive">{error}</p>}
    </div>;
}
