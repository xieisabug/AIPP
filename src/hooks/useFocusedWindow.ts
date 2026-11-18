import { useSyncExternalStore } from "react";

function subscribe(change: () => void) {
    window.addEventListener("focus", change);
    window.addEventListener("blur", change);
    document.addEventListener("visibilitychange", change);
    return () => {
        window.removeEventListener("focus", change);
        window.removeEventListener("blur", change);
        document.removeEventListener("visibilitychange", change);
    };
}

export const useFocusedWindow = () => useSyncExternalStore(subscribe, () => !document.hidden && document.hasFocus());
