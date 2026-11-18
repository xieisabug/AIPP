import React from 'react';
import { render } from '@testing-library/react';
import { expect, it } from 'vitest';
import { SendLayoutSnapshot, type SendLayout } from './SendLayoutSnapshot';

it('should snapshot the outer merged row when nested messages share its ID', () => {
    const container = document.createElement('div');
    document.body.append(container);
    container.getBoundingClientRect = () => ({top:0,bottom:400} as DOMRect);
    const wrapperRef = React.createRef<HTMLDivElement>();
    const snapshotRef = {current:null as SendLayout | null};
    const tree = (key:string) => <SendLayoutSnapshot snapshotKey={key} wrapperRef={wrapperRef}
        container={container} snapshotRef={snapshotRef}>
        <div data-message-item data-message-id="1" data-testid="outer">
            <div data-message-item data-message-id="1" data-testid="inner">reply</div>
        </div>
    </SendLayoutSnapshot>;
    const view = render(tree('1'), {container});
    view.getByTestId('outer').getBoundingClientRect = () => ({top:100,bottom:150} as DOMRect);
    view.getByTestId('inner').getBoundingClientRect = () => ({top:117,bottom:140} as DOMRect);
    view.rerender(tree('2'));
    expect(snapshotRef.current?.anchorTop).toBe(100);
    expect(snapshotRef.current?.anchorId).toBe('1');
    view.unmount();
    container.remove();
});
