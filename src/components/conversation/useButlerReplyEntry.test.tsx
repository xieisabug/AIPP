import { act, render } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { useRef } from 'react';
import type { Message } from '@/data/Conversation';
import { useButlerReplyEntry } from './useButlerReplyEntry';
import { playButlerBubbleAnimation } from './butlerBubbleAnimation';
vi.mock('./butlerBubbleAnimation', () => ({playButlerBubbleAnimation:vi.fn(() => vi.fn())}));
const msg = (id:number, content='reply') => ({id,content,message_type:'response'} as Message);
function Fixture({messages,loaded=true,enabled=true,conversation='1',merged=false}:{messages:Message[],loaded?:boolean,enabled?:boolean,conversation?:string,merged?:boolean}) {
    const ref=useRef<HTMLDivElement>(null);
    useButlerReplyEntry(ref,conversation,loaded,enabled,messages);
    if (merged) return <div ref={ref}>{messages.length > 0 && <div data-bubble-entry-id={messages[0].id} data-bubble-entry-members={messages.map(m=>m.id).join(',')}>{messages.map(m=>m.content).join('')}</div>}</div>;
    return <div ref={ref}>{messages.map(m=><div key={m.id} data-bubble-entry-id={m.id}>{m.content}</div>)}</div>;
}
afterEach(() => vi.clearAllMocks());
it('should animate a reply once, including an initially empty streaming placeholder',async()=>{
    const view=render(<Fixture messages={[msg(1)]}/>);
    view.rerender(<Fixture messages={[msg(1),msg(2,'')]}/>);
    expect(playButlerBubbleAnimation).not.toHaveBeenCalled();
    view.rerender(<Fixture messages={[msg(1),msg(2,'first')]}/>);
    expect(playButlerBubbleAnimation).toHaveBeenCalledTimes(1);
    view.rerender(<Fixture messages={[msg(1),msg(2,'more tokens')]}/>);
    await act(async()=>{});
    expect(playButlerBubbleAnimation).toHaveBeenCalledTimes(1);
});
it('should baseline async history and switched conversations without animating',()=>{
    const view=render(<Fixture messages={[]} loaded={false}/>);
    view.rerender(<Fixture messages={[msg(1)]}/>);
    view.rerender(<Fixture conversation="2" messages={[]} loaded={false}/>);
    view.rerender(<Fixture conversation="2" messages={[msg(3)]}/>);
    expect(playButlerBubbleAnimation).not.toHaveBeenCalled();
});
it('should leave ordinary Chat unchanged and not replay deleted then restored messages',()=>{
    const view=render(<Fixture messages={[]} enabled={false}/>);
    view.rerender(<Fixture messages={[msg(1)]} enabled={false}/>);
    expect(playButlerBubbleAnimation).not.toHaveBeenCalled();
    view.rerender(<Fixture messages={[msg(1)]}/>);
    view.rerender(<Fixture messages={[msg(1),msg(2)]}/>);
    view.rerender(<Fixture messages={[msg(1)]}/>);
    view.rerender(<Fixture messages={[msg(1),msg(2)]}/>);
    expect(playButlerBubbleAnimation).toHaveBeenCalledTimes(1);
});

it('should animate a merged reply only once and not replay its later segments when unmerged',()=>{
    const view=render(<Fixture messages={[]} merged/>);
    view.rerender(<Fixture messages={[msg(1)]} merged/>);
    expect(playButlerBubbleAnimation).toHaveBeenCalledTimes(1);
    view.rerender(<Fixture messages={[msg(1),msg(2)]} merged/>);
    view.rerender(<Fixture messages={[msg(1),msg(2)]}/>);
    expect(playButlerBubbleAnimation).toHaveBeenCalledTimes(1);
});
