import { useEffect, useRef, useState, type RefObject } from "react";
import { useReducedMotion } from "motion/react";
import { actionDurations, type AvatarAction, type AvatarState } from "./avatarDefinition";

export interface AvatarActionRequest { action: AvatarAction; sequence: number }

export function useAvatarMotion(ref: RefObject<HTMLElement | null>, enabled: boolean, state: AvatarState, request?: AvatarActionRequest, equipment = "") {
    const reduced = useReducedMotion();
    const [visible, setVisible] = useState(!document.hidden);
    const [action, setAction] = useState<AvatarAction | null>(null);
    const [sequence, setSequence] = useState(0);
    const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
    const active = enabled && !reduced && visible;

    useEffect(() => {
        let inView = true;
        const update = () => setVisible(inView && !document.hidden);
        const observer = typeof IntersectionObserver !== "undefined" ? new IntersectionObserver(entries => {
            inView = entries[0]?.isIntersecting ?? false;
            update();
        }) : undefined;
        if (ref.current) observer?.observe(ref.current);
        document.addEventListener("visibilitychange", update);
        return () => { observer?.disconnect(); document.removeEventListener("visibilitychange", update); };
    }, [ref]);

    // State changes interrupt prop performances and settle in the equipped pose.
    useEffect(() => {
        clearTimeout(timer.current);
        setAction(null);
        return () => clearTimeout(timer.current);
    }, [state, active, equipment]);

    useEffect(() => {
        if (!active || !request) return;
        clearTimeout(timer.current);
        setAction(request.action);
        setSequence(previous => previous + 1);
        timer.current = setTimeout(() => setAction(null), actionDurations[request.action]);
        return () => clearTimeout(timer.current);
        // A completed/interrupted request must never replay when visibility returns.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [request?.sequence, request?.action]);

    return { active, action: active ? action : null, sequence };
}
