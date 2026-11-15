import { useState } from 'react';
import { Blocks, ChevronDown, Plug } from 'lucide-react';

export function CatalogEndpoints({ entries }: { entries: { name: string; description?: string | null }[] }) {
    if (!entries.length) return null;
    return <section className="space-y-3">
        <h4 className="text-sm font-medium">应用 <span className="font-normal text-muted-foreground">{entries.length}</span></h4>
        <div className="divide-y rounded-lg border">{entries.map(entry => <div key={entry.name} className="flex items-start gap-3 px-4 py-3">
            <Plug className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
            <div className="min-w-0"><p className="break-words text-sm font-medium">{entry.name}</p>
                {entry.description && <p className="mt-1 break-all text-xs text-muted-foreground">{entry.description}</p>}
            </div>
        </div>)}</div>
    </section>;
}

interface Entry { id: string; name: string; description: string; content: string; kind?: string }

export function CatalogSkills({ entries }: { entries: Entry[] }) {
    const [expanded, setExpanded] = useState(false);
    if (!entries.length) return null;
    return <section className="space-y-3">
        <h4 className="text-sm font-medium">技能 <span className="font-normal text-muted-foreground">{entries.length}</span></h4>
        <div className="divide-y rounded-lg border">{(expanded ? entries : entries.slice(0, 5)).map(entry =>
            <details key={entry.id} className="group px-4 py-3">
                <summary className="flex cursor-pointer list-none items-start gap-3">
                    <Blocks className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                    <div className="min-w-0 flex-1"><p className="text-sm font-medium">{entry.kind === 'command' && '/'}{entry.name}</p>
                        <p className="mt-0.5 text-xs text-muted-foreground">{entry.description}</p></div>
                    <ChevronDown className="h-4 w-4 text-muted-foreground group-open:rotate-180" />
                </summary>
                <pre className="mt-3 max-h-80 overflow-auto whitespace-pre-wrap rounded-md bg-muted/40 p-3 text-xs">{entry.content}</pre>
            </details>)}
            {entries.length > 5 && <button type="button" className="w-full py-2.5 text-xs text-muted-foreground" onClick={() => setExpanded(!expanded)}>
                {expanded ? '收起' : `查看另外 ${entries.length - 5} 个技能`}
            </button>}
        </div>
    </section>;
}
