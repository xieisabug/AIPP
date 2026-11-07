import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { registerMobileBackHandler } from "@/mobile/navigation";

type DetailsPreview = "artifact" | "context";

interface SuspendedDetails {
    conversationId: string;
    preview: DetailsPreview;
    scrollTop: number;
}

interface MobileConversationDetailsOptions {
    conversationId: string;
    enabled: boolean;
    previewFileOpen?: boolean;
}

export function useMobileConversationDetails({
    conversationId,
    enabled,
    previewFileOpen = false,
}: MobileConversationDetailsOptions) {
    const [detailsOpen, setDetailsOpen] = useState(false);
    const scrollerRef = useRef<HTMLDivElement | null>(null);
    const pendingScrollRef = useRef<number | null>(null);
    const conversationIdRef = useRef(conversationId);
    const suspendedRef = useRef<SuspendedDetails | null>(null);
    const contextPreviewSeenOpenRef = useRef(false);
    const previewFileOpenRef = useRef(previewFileOpen);
    conversationIdRef.current = conversationId;
    previewFileOpenRef.current = previewFileOpen;

    const writePendingScroll = useCallback((node: HTMLDivElement | null) => {
        if (!node || pendingScrollRef.current == null) return;
        node.scrollTop = pendingScrollRef.current;
        pendingScrollRef.current = null;
    }, []);

    const detailsScrollRef = useCallback((node: HTMLDivElement | null) => {
        scrollerRef.current = node;
        writePendingScroll(node);
    }, [writePendingScroll]);

    const openDetails = useCallback(() => {
        if (!conversationIdRef.current) return;
        suspendedRef.current = null;
        contextPreviewSeenOpenRef.current = false;
        pendingScrollRef.current = null;
        setDetailsOpen(true);
    }, []);

    const suspendDetails = useCallback((preview: DetailsPreview) => {
        const currentConversationId = conversationIdRef.current;
        if (!currentConversationId) return;
        suspendedRef.current = {
            conversationId: currentConversationId,
            preview,
            scrollTop: scrollerRef.current?.scrollTop ?? 0,
        };
        if (preview === "context") contextPreviewSeenOpenRef.current = previewFileOpenRef.current;
        setDetailsOpen(false);
    }, []);

    const restoreDetails = useCallback((preview: DetailsPreview) => {
        const suspended = suspendedRef.current;
        if (!suspended || suspended.preview !== preview) return;
        suspendedRef.current = null;
        if (suspended.conversationId !== conversationIdRef.current) return;
        pendingScrollRef.current = suspended.scrollTop;
        setDetailsOpen(true);
    }, []);

    useLayoutEffect(() => {
        if (!detailsOpen) return;
        writePendingScroll(scrollerRef.current);
    }, [detailsOpen, writePendingScroll]);

    useEffect(() => {
        if (!enabled) return;
        return registerMobileBackHandler("preview", () => {
            restoreDetails("artifact");
            return false;
        });
    }, [enabled, restoreDetails]);

    useEffect(() => {
        if (!enabled || suspendedRef.current?.preview !== "context") return;
        if (previewFileOpen) {
            contextPreviewSeenOpenRef.current = true;
            return;
        }
        if (!contextPreviewSeenOpenRef.current) return;
        contextPreviewSeenOpenRef.current = false;
        restoreDetails("context");
    }, [enabled, previewFileOpen, restoreDetails]);

    useEffect(() => {
        suspendedRef.current = null;
        contextPreviewSeenOpenRef.current = false;
        pendingScrollRef.current = null;
        setDetailsOpen(false);
    }, [conversationId, enabled]);

    return {
        detailsOpen,
        setDetailsOpen,
        detailsScrollRef,
        openDetails,
        suspendDetails,
    };
}
