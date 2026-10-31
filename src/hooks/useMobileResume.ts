import { useEffect, useRef } from "react";
import { useIsMobilePlatform } from "./use-platform";

/** Refresh observations on resume; this hook never triggers execution or retries. */
export function useMobileResume(refresh: () => void, identity?: string | number) {
    const mobile = useIsMobilePlatform();
    const latest = useRef(refresh);
    latest.current = refresh;
    useEffect(() => {
        if (!mobile) return;
        let timer: ReturnType<typeof setTimeout> | undefined;
        const request = () => {
            if (document.visibilityState === "hidden") return;
            clearTimeout(timer);
            timer = setTimeout(() => latest.current(), 100);
        };
        window.addEventListener("focus", request);
        window.addEventListener("online", request);
        window.addEventListener("aipp:mobile-navigate", request);
        document.addEventListener("visibilitychange", request);
        return () => {
            clearTimeout(timer);
            window.removeEventListener("focus", request);
            window.removeEventListener("online", request);
            window.removeEventListener("aipp:mobile-navigate", request);
            document.removeEventListener("visibilitychange", request);
        };
    }, [mobile, identity]);
}
