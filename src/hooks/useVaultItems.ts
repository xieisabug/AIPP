import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { VaultCapability, VaultItem } from "@/data/Vault";

/** 保险箱条目（仅元数据）与本机能力，保险箱变化时自动刷新。供配置里的「从保险箱选择」使用。 */
export function useVaultItems() {
    const [capability, setCapability] = useState<VaultCapability | null>(null);
    const [items, setItems] = useState<VaultItem[]>([]);
    const [loaded, setLoaded] = useState(false);

    const reload = useCallback(async () => {
        try {
            const cap = await invoke<VaultCapability>("vault_capability");
            setCapability(cap);
            setItems(cap.available ? await invoke<VaultItem[]>("vault_list") : []);
        } catch {
            setCapability(null);
            setItems([]);
        } finally {
            setLoaded(true);
        }
    }, []);

    useEffect(() => {
        void reload();
        let disposed = false;
        let unlisten: (() => void) | undefined;
        void listen("vault_changed", () => void reload()).then((fn) => {
            if (disposed) fn();
            else unlisten = fn;
        });
        return () => {
            disposed = true;
            unlisten?.();
        };
    }, [reload]);

    return useMemo(
        () => ({ capability, items, loaded, available: !!capability?.available, reload }),
        [capability, items, loaded, reload],
    );
}
