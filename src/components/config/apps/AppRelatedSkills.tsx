import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Blocks } from 'lucide-react';
import { Button } from '@/components/ui/button';
import SkillInstallGuideDialog from '../SkillInstallGuideDialog';

interface Skill { id: string; name: string; description: string }

export default function AppRelatedSkills({ ids }: { ids: string[] }) {
    const [skills, setSkills] = useState<Skill[]>([]);
    const [error, setError] = useState('');
    const [loading, setLoading] = useState(false);
    const [selected, setSelected] = useState<string>();
    const identity = ids.join('\n');
    useEffect(() => {
        if (!identity) return;
        let alive = true;
        setLoading(true); setError(''); setSkills([]);
        void invoke<Skill[]>('fetch_official_skills', { useProxy: false })
            .then(items => { if (alive) setSkills(items); })
            .catch(e => { if (alive) setError(String(e)); })
            .finally(() => { if (alive) setLoading(false); });
        return () => { alive = false; };
    }, [identity]);
    return <section className="space-y-3">
        <h4 className="text-sm font-medium">配套技能 {ids.length > 0 && <span className="font-normal text-muted-foreground">{ids.length}</span>}</h4>
        {!ids.length && <p className="text-sm text-muted-foreground">此应用未声明配套技能。</p>}
        {loading && <div className="h-14 animate-pulse rounded-lg bg-muted" aria-label="正在查询技能目录" />}
        {error && <p role="alert" className="rounded-lg border border-destructive/30 px-3 py-2 text-sm text-destructive">技能目录读取失败：{error}</p>}
        {!loading && ids.length > 0 && <div className="divide-y rounded-lg border">
            {ids.map(id => {
                const skill = skills.find(item => item.id === id);
                return <div key={id} className="flex items-start gap-3 px-4 py-3">
                    <Blocks className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                    <div className="min-w-0 flex-1">
                        <p className="text-sm font-medium">{skill?.name ?? id}</p>
                        <p className="mt-0.5 text-xs leading-5 text-muted-foreground">{skill?.description ?? '尚未在技能目录中找到此项，当前无法安装。'}</p>
                    </div>
                    {skill && <Button size="sm" variant="outline" onClick={() => setSelected(id)}>查看并安装</Button>}
                </div>;
            })}
        </div>}
        {selected && <SkillInstallGuideDialog isOpen onClose={() => setSelected(undefined)} officialSkillIds={[selected]} />}
    </section>;
}
