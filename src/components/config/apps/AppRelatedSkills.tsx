import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
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
    return <section className="space-y-2">
        <h4 className="text-sm font-medium">配套技能</h4>
        {!ids.length && <p className="text-sm text-muted-foreground">此应用未声明配套技能。</p>}
        {loading && <p className="text-sm text-muted-foreground">正在查询技能目录…</p>}
        {error && <p role="alert" className="text-sm text-destructive">技能目录读取失败：{error}</p>}
        {!loading && ids.map(id => {
            const skill = skills.find(item => item.id === id);
            return <div key={id} className="space-y-2 rounded-lg border p-3">
                <p className="text-sm font-medium">{skill?.name ?? id}</p>
                <p className="text-xs text-muted-foreground">{skill?.description ?? '尚未在技能目录中找到此项，当前无法安装。'}</p>
                {skill && <Button size="sm" variant="outline" onClick={() => setSelected(id)}>查看并安装</Button>}
            </div>;
        })}
        {selected && <SkillInstallGuideDialog isOpen onClose={() => setSelected(undefined)} officialSkillIds={[selected]} />}
    </section>;
}
