import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
interface Policy {mode:string;sources:string[];expires_at:string;paused:boolean;stopped:boolean;stop_current_on_pause:boolean}
export default function ButlerCheckPolicy({ taskId }: {taskId:number}) {
    const [policy,setPolicy]=useState<Policy>();
    const [mode,setMode]=useState("read_only");
    const [sources,setSources]=useState("");
    const [expires,setExpires]=useState("");
    const [stopCurrent,setStopCurrent]=useState(false);
    const [busy,setBusy]=useState(false);
    const [error,setError]=useState("");
    const id=useRef(taskId); id.current=taskId;
    useEffect(()=>{let active=true;setPolicy(undefined);setError("");setSources("");setExpires("");setMode("read_only");setStopCurrent(false);setBusy(false);
        void invoke<Policy|null>("get_butler_check_policy",{taskId}).then(p=>{if(active&&p){setPolicy(p);setMode(p.mode);setSources(p.sources.join("\n"));setExpires(p.expires_at);setStopCurrent(p.stop_current_on_pause);}}).catch(e=>{if(active)setError(String(e));});
        return()=>{active=false;};},[taskId]);
    const act=async(action:string)=>{if(busy)return;setBusy(true);setError("");
        try {
            if(action==="save") {const p:Policy={mode,sources:sources.split("\n").map(s=>s.trim()).filter(Boolean),expires_at:expires,paused:false,stopped:false,stop_current_on_pause:stopCurrent};await invoke("configure_butler_check",{taskId,policy:p});}
            else await invoke("control_butler_check",{taskId,action,stopCurrent});
            const p=await invoke<Policy>("get_butler_check_policy",{taskId});if(id.current===taskId)setPolicy(p);
        }catch(e){if(id.current===taskId)setError(String(e));}finally{if(id.current===taskId)setBusy(false);}
    };
    return <section className="space-y-2 border rounded p-3" aria-label="管家后台检查范围">
        <h3 className="text-sm font-medium">管家后台检查范围</h3>
        <p className="text-xs">{!policy?"未配置，不会自动执行":policy.stopped?"已关闭":policy.paused?"已暂停":"已授权检查"}。仅处理你明确交代的检查，不自动寻找新事项。</p>
        <label className="block text-xs">模式<select aria-label="检查模式" value={mode} onChange={e=>setMode(e.target.value)} className="block min-h-11"><option value="read_only">只读检查</option><option value="draft">读取后准备草稿</option></select></label>
        <label className="block text-xs">检查来源（每行一个完整网址或文件绝对路径）<Textarea aria-label="检查来源" value={sources} onChange={e=>setSources(e.target.value)}/></label>
        <p className="text-xs text-muted-foreground">目前只适配内置 fetch_url/read_file；未适配工具会明确拒绝。草稿保留在结果里，不向外发送或写入文件。</p>
        <label className="block text-xs">截止时间（含时区，例如 2026-11-08T18:00:00+08:00）<Input aria-label="检查截止时间" value={expires} onChange={e=>setExpires(e.target.value)}/></label>
        <label className="text-xs flex items-center gap-2"><input type="checkbox" checked={stopCurrent} onChange={e=>setStopCurrent(e.target.checked)}/>暂停或关闭时，也请求停止当前执行（不代表撤销已完成操作）</label>
        <div className="flex flex-wrap gap-2">
            <Button disabled={busy||!sources.trim()||!expires.trim()} onClick={()=>void act("save")}>确认范围并启用</Button>
            {policy && !policy.stopped && <><Button variant="outline" disabled={busy} onClick={()=>void act(policy.paused?"resume":"pause")}>{policy.paused?"恢复检查":"暂停检查"}</Button><Button variant="outline" disabled={busy} onClick={()=>void act("stop")}>关闭检查</Button></>}
        </div>
        {error&&<p role="alert" className="text-destructive">{error}</p>}
    </section>;
}
