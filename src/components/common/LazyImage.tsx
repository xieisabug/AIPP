import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { getInlineImage } from '@/lib/inlineImageStore';

interface LazyImageProps {
    src?: string;
    alt?: string;
    className?: string;
    [key: string]: any;
}

/**
 * 懒加载图片组件
 * - 使用 Intersection Observer 实现视口内加载
 * - 对于 base64 图片进行异步解码
 * - 提供加载状态和平滑过渡
 */
// 在内存中缓存 data URI 转换生成的 objectURL，避免重复解码
const base64UrlCache = new Map<string, { objectUrl: string; refCount: number }>();

const LazyImage: React.FC<LazyImageProps> = ({ src, alt = '', className = '', ...props }) => {
    const inlineId = (props['data-inline-id'] ?? props['dataInlineId'] ?? props['dataInlineid']) as string | undefined;
    const inlineAlt = ((props['data-alt'] ?? props['dataAlt']) as string | undefined) ?? alt;
    const inlineRecord = useMemo(() => getInlineImage(inlineId), [inlineId]);
    const resolvedSrc = inlineRecord?.data ?? src;
    const approximateSizeKB = inlineRecord ? inlineRecord.byteLength / 1024 : undefined;
    const cacheKey = resolvedSrc ? (inlineId ? `inline:${inlineId}` : resolvedSrc) : undefined;
    const [isLoaded, setIsLoaded] = useState(false);
    const [isInView, setIsInView] = useState(false);
    const [imageSrc, setImageSrc] = useState<string | undefined>(undefined);
    const imgRef = useRef<HTMLImageElement>(null);
    const containerRef = useRef<HTMLDivElement>(null);
    const cleanupRef = useRef<(() => void) | null>(null);

    // 移除不需要传递到 DOM 的属性
    const forwardedProps = useMemo(() => {
        const cloned = { ...props };
        delete cloned['data-inline-id'];
        delete cloned['data-inlineid'];
        delete cloned['dataInlineId'];
        delete cloned['dataInlineid'];
        delete cloned['data-alt'];
        delete cloned['dataAlt'];
        return cloned;
    }, [props]);

    // 检测是否是 base64 图片
    const isBase64 = resolvedSrc?.startsWith('data:');

    // Intersection Observer 懒加载
    useEffect(() => {
        const container = containerRef.current;
        if (!container) return;

        const observer = new IntersectionObserver(
            (entries) => {
                entries.forEach((entry) => {
                    if (entry.isIntersecting) {
                        setIsInView(true);
                        observer.disconnect();
                    }
                });
            },
            {
                rootMargin: '100px', // 提前 100px 开始加载
                threshold: 0.01,
            }
        );

        observer.observe(container);

        return () => observer.disconnect();
    }, []);

    // 当图片进入视口时，开始加载
    useEffect(() => {
        if (!isInView || !resolvedSrc) return;

        let canceled = false;
        cleanupRef.current?.();
        cleanupRef.current = null;
        setIsLoaded(false);
        setImageSrc(undefined);

        type ScheduleToken = {
            handle: number | ReturnType<typeof setTimeout>;
            isIdle: boolean;
        };

        const schedule = (cb: () => void): ScheduleToken => {
            if (typeof window !== 'undefined') {
                const win = window as typeof window & {
                    requestIdleCallback?: (callback: () => void) => number;
                };
                if (typeof win.requestIdleCallback === 'function') {
                        const idleHandle = win.requestIdleCallback(() => cb());
                        return { handle: idleHandle, isIdle: true };
                }
            }
            return { handle: setTimeout(cb, 16), isIdle: false };
        };

        const cancelSchedule = (token: ScheduleToken) => {
            if (token.isIdle && typeof window !== 'undefined') {
                const win = window as typeof window & {
                    cancelIdleCallback?: (id: number) => void;
                };
                if (typeof win.cancelIdleCallback === 'function') {
                    win.cancelIdleCallback(token.handle as number);
                    return;
                }
            }
            clearTimeout(token.handle as ReturnType<typeof setTimeout>);
        };

        const loadImage = () => {
            if (canceled) return;

            if (isBase64) {
                let released = false;
                const retainCache = (objectUrl: string) => {
                    setImageSrc(objectUrl);
                    return () => {
                        if (released) return;
                        if (cacheKey && base64UrlCache.has(cacheKey)) {
                            const cached = base64UrlCache.get(cacheKey)!;
                            cached.refCount -= 1;
                            if (cached.refCount <= 0) {
                                URL.revokeObjectURL(cached.objectUrl);
                                base64UrlCache.delete(cacheKey);
                            }
                        } else {
                            URL.revokeObjectURL(objectUrl);
                        }
                        released = true;
                    };
                };

                const assignObjectUrl = async () => {
                    try {
                        if (cacheKey && base64UrlCache.has(cacheKey)) {
                            const cached = base64UrlCache.get(cacheKey)!;
                            cached.refCount += 1;
                            return retainCache(cached.objectUrl);
                        }

                        const response = await fetch(resolvedSrc);
                        const blob = await response.blob();
                        const objectUrl = URL.createObjectURL(blob);
                        if (cacheKey) {
                            base64UrlCache.set(cacheKey, { objectUrl, refCount: 1 });
                        }
                        return retainCache(objectUrl);
                    } catch (error) {
                        console.error('Failed to prepare base64 image', error);
                        setImageSrc(resolvedSrc);
                        return null;
                    }
                };

                assignObjectUrl().then((cleanup) => {
                    if (cleanup && !canceled) {
                        cleanupRef.current = cleanup;
                    }
                });
            } else {
                setImageSrc(resolvedSrc);
            }
        };

        const token = schedule(loadImage);

        return () => {
            canceled = true;
            cancelSchedule(token);
            cleanupRef.current?.();
            cleanupRef.current = null;
        };
    }, [isInView, resolvedSrc, isBase64, cacheKey, inlineId]);

    // 图片加载完成回调
    const handleLoad = useCallback(() => {
        setIsLoaded(true);
    }, []);

    // 如果没有 src，不渲染
    if (!resolvedSrc) return null;

    return (
        <span
            ref={containerRef}
            className="inline-block relative"
            style={{ minHeight: isLoaded ? 'auto' : '1.5em', minWidth: isLoaded ? 'auto' : '2em' }}
        >
            {/* 占位符/加载状态 */}
            {!isLoaded && (
                <span
                    className="inline-flex items-center justify-center bg-muted/50 rounded text-muted-foreground text-xs"
                    style={{ 
                        minHeight: '1.5em', 
                        minWidth: '2em',
                        padding: '0.25em 0.5em',
                    }}
                >
                    <svg
                        className="animate-spin mr-1"
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                    >
                        <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
                    </svg>
                    {approximateSizeKB ? `${Math.ceil(approximateSizeKB)} KB` : '加载图片'}
                </span>
            )}
            
            {/* 实际图片 */}
            {imageSrc && (
                <img
                    ref={imgRef}
                    src={imageSrc}
                    alt={inlineAlt}
                    className={`${className} transition-opacity duration-300`}
                    style={{
                        opacity: isLoaded ? 1 : 0,
                        maxWidth: '100%',
                        height: 'auto',
                        // 加载完成前隐藏
                        position: isLoaded ? 'relative' : 'absolute',
                    }}
                    onLoad={handleLoad}
                    loading="lazy"
                    decoding="async"
                    {...forwardedProps}
                />
            )}
        </span>
    );
};

export default React.memo(LazyImage);
