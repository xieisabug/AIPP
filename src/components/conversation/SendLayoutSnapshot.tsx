import React from 'react';

export interface SendLayout { height: number; scrollTop: number; scrollHeight: number; viewportHeight: number; anchorId?: string; anchorTop?: number }
/** React's pre-mutation lifecycle preserves the actual old layout, including resizes. */
export class SendLayoutSnapshot extends React.Component<React.HTMLAttributes<HTMLDivElement> & {
    snapshotKey: string;
    wrapperRef: React.RefObject<HTMLDivElement | null>;
    container: HTMLDivElement | null;
    snapshotRef: React.RefObject<SendLayout | null>;
}> {
    getSnapshotBeforeUpdate(previous: Readonly<typeof this.props>) {
        if (previous.snapshotKey === this.props.snapshotKey) return null;
        const {wrapperRef, container, snapshotRef} = this.props;
        if (!wrapperRef.current || !container) return null;
        const viewport = container.getBoundingClientRect();
        const anchor = [...container.querySelectorAll<HTMLElement>('[data-message-item]')].filter(row => {
            if (row.parentElement?.closest('[data-message-item]')) return false;
            const rect = row.getBoundingClientRect();
            return rect.bottom > viewport.top && rect.top < viewport.bottom;
        }).at(-1);
        snapshotRef.current = {
            anchorId: anchor?.dataset.messageId,
            anchorTop: anchor ? anchor.getBoundingClientRect().top - viewport.top : undefined,
            height: wrapperRef.current.getBoundingClientRect().height,
            scrollTop: container.scrollTop,
            scrollHeight: container.scrollHeight,
            viewportHeight: container.clientHeight - parseFloat(getComputedStyle(container).paddingTop || "0") - parseFloat(getComputedStyle(container).paddingBottom || "0"),
        };
        return null;
    }
    componentDidUpdate() {}
    render() {
        const { wrapperRef, container: _container, snapshotRef: _snapshot, snapshotKey: _key, ...props } = this.props;
        return <div {...props} ref={wrapperRef} />;
    }
}
