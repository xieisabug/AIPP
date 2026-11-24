import { useState, useRef, useCallback, useEffect } from "react";

import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

const useFileDropHandler = (onFilesSelect: (files: File[]) => void, onPathsSelect: (paths: string[]) => void) => {
    const [isDragging, setIsDragging] = useState<boolean>(false);
    const dragCounter = useRef<number>(0);

    const handleDragEnter = useCallback((e: React.DragEvent<HTMLDivElement>) => {
        e.preventDefault();
        e.stopPropagation();
        dragCounter.current++;
        if (e.dataTransfer.items && e.dataTransfer.items.length > 0) {
            setIsDragging(true);
        }
    }, []);

    const handleDragLeave = useCallback((e: React.DragEvent<HTMLDivElement>) => {
        e.preventDefault();
        e.stopPropagation();
        dragCounter.current--;
        if (dragCounter.current === 0) {
            setIsDragging(false);
        }
    }, []);

    const handleDragOver = useCallback((e: React.DragEvent<HTMLDivElement>) => {
        e.preventDefault();
        e.stopPropagation();
    }, []);

    const handleDrop = useCallback((e: React.DragEvent<HTMLDivElement>) => {
        e.preventDefault();
        e.stopPropagation();
        setIsDragging(false);
        dragCounter.current = 0;
        if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
            onFilesSelect(Array.from(e.dataTransfer.files));
        }
    }, [onFilesSelect]);

    const dropRef = useRef<HTMLDivElement | null>(null);
    useEffect(() => {
        if (!isTauri()) return;
        let active = true;
        const subscription = getCurrentWebview().onDragDropEvent(({ payload }) => {
            if (!active) return;
            if (payload.type === "leave") { setIsDragging(false); return; }
            const bounds = dropRef.current?.getBoundingClientRect();
            const x = payload.position.x / window.devicePixelRatio;
            const y = payload.position.y / window.devicePixelRatio;
            const inside = !!bounds && x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom;
            setIsDragging(inside && payload.type !== "drop");
            if (inside && payload.type === "drop") onPathsSelect(payload.paths);
        }).catch(error => {
            console.error("无法监听桌面文件拖放", error);
            return () => {};
        });
        return () => { active = false; void subscription.then(unlisten => unlisten()); };
    }, [onPathsSelect]);

    useEffect(() => {
        const div = dropRef.current;
        if (div) {
            div.addEventListener('dragenter', handleDragEnter as any);
            div.addEventListener('dragleave', handleDragLeave as any);
            div.addEventListener('dragover', handleDragOver as any);
            div.addEventListener('drop', handleDrop as any);
        }

        return () => {
            if (div) {
                div.removeEventListener('dragenter', handleDragEnter as any);
                div.removeEventListener('dragleave', handleDragLeave as any);
                div.removeEventListener('dragover', handleDragOver as any);
                div.removeEventListener('drop', handleDrop as any);
            }
        };
    }, [handleDragEnter, handleDragLeave, handleDragOver, handleDrop]);

    return { isDragging, setIsDragging, dropRef, handleDragEnter, handleDragLeave, handleDragOver, handleDrop };
};

export default useFileDropHandler;