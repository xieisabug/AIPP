import { useRef } from "react";
import { motion, useMotionValueEvent, type MotionValue } from "motion/react";
import type { AvatarEffect } from "./avatarRecipes";

export default function AvatarEffects({ kind, time, opacity, color }: { kind: AvatarEffect; time: MotionValue<number>; opacity: MotionValue<number>; color: string }) {
    const root = useRef<SVGGElement>(null);
    useMotionValueEvent(time, "change", t => {
        const nodes = root.current?.querySelectorAll<SVGElement>("[data-piece]");
        nodes?.forEach((node, i) => {
            const phase = t * 3.5 - i * 0.75;
            let transform = "", alpha = 1;
            if (kind === "dots") { transform = `translate(${(i-1)*64} ${-Math.max(0, Math.sin(phase))*35}) scale(${0.8+0.2*Math.sin(phase)})`; alpha = 0.5+0.5*(Math.sin(phase)+1)/2; }
            else if (kind === "orbit") transform = `rotate(${t*90+i*120}) translate(85 0) scale(${0.65+0.35*Math.sin(phase)})`;
            else if (kind === "radar") { transform = i === 2 ? `rotate(${t*125})` : `scale(${0.65+((t*0.5+i*0.5)%1)*0.5})`; alpha = i === 2 ? 1 : 0.25+0.4*(1-((t*0.5+i*0.5)%1)); }
            else if (kind === "progress") { transform = `rotate(${i*30}) translate(0 -78)`; alpha = 0.15+0.85*((Math.sin(t*6-i*0.52)+1)/2)**3; }
            else if (kind === "bars") transform = `translate(${(i-3)*28} 0) scale(1 ${0.25+0.75*Math.abs(Math.sin(phase)*Math.cos(t*1.5+i))})`;
            else if (["send", "receive", "upload"].includes(kind)) { const f = (t*0.7+i*0.32)%1; transform = `translate(0 ${(kind === "receive" ? 1 : -1)*(f*100-50)})`; alpha = Math.sin(f*Math.PI); }
            else if (kind === "pencil") transform = i === 0 ? `translate(${Math.sin(t*2)*65} ${Math.cos(t*4)*10}) rotate(${12+Math.sin(t*2)*9})` : `scale(${0.5+0.5*Math.sin(t*2)} 1)`;
            else if (kind === "alert") transform = `rotate(${Math.sin(t*14)*6*Math.max(0,Math.sin(t*2))}) scale(${1+0.07*Math.sin(t*4)})`;
            else if (kind === "ball") transform = `translate(0 ${30-Math.abs(Math.sin(t*3))*110}) scale(${1+0.18*(1-Math.abs(Math.sin(t*3)))} ${0.8+0.2*Math.abs(Math.sin(t*3))})`;
            else if (kind === "power") { transform = `scale(${0.85+0.15*Math.cos(t*2)})`; alpha = 0.45+0.55*(Math.cos(t*2)+1)/2; }
            else if (kind === "confetti") { const f = (t*0.45+i/10)%1; const angle = i*2.399; transform = `translate(${Math.cos(angle)*(90+f*160)} ${Math.sin(angle)*(90+f*140)+f*f*100}) rotate(${f*300})`; alpha = Math.sin(f*Math.PI); }
            else if (kind === "spark") transform = `rotate(${t*40+i*90}) translate(0 ${35+25*Math.sin(t*2)}) scale(${0.7+0.3*Math.sin(phase)})`;
            node.setAttribute("transform", transform); node.setAttribute("opacity", String(alpha));
        });
    });
    return <motion.g ref={root} data-avatar-effect={kind} style={{ opacity }} fill={color} stroke={color} strokeWidth="9" strokeLinecap="round" strokeLinejoin="round">
        {kind === "dots" && [0,1,2].map(i => <circle key={i} data-piece="" r="22" />)}
        {kind === "orbit" && <><ellipse rx="100" ry="100" fill="none" opacity="0.12" strokeWidth="5" />{[0,1,2].map(i => <circle key={i} data-piece="" r="18" />)}</>}
        {kind === "radar" && <><circle data-piece="" r="90" fill="none" /><circle data-piece="" r="55" fill="none" /><path data-piece="" d="M0 0L0-90A90 90 0 0 1 78-45Z" strokeWidth="3" fillOpacity="0.25" /></>}
        {kind === "progress" && Array.from({length:12},(_,i) => <rect key={i} data-piece="" x="-6" y="-15" width="12" height="30" rx="6" stroke="none" />)}
        {kind === "bars" && Array.from({length:7},(_,i) => <rect key={i} data-piece="" x="-7" y="-65" width="14" height="130" rx="7" stroke="none" />)}
        {["send","receive","upload"].includes(kind) && [0,1,2].map(i => <path key={i} data-piece="" d={kind === "receive" ? "M-35-20L0 15L35-20" : "M-35 20L0-15L35 20"} fill="none" />)}
        {kind === "pencil" && <><g data-piece=""><path d="M-9 30L12-65L36-60L15 36L-2 53Z" /><path d="M-2 53L15 36" stroke="white" strokeWidth="4" /></g><path data-piece="" d="M-90 65H85" fill="none" opacity="0.45" /></>}
        {kind === "alert" && <g data-piece=""><path d="M0-85L94 70H-94Z" fill="none" /><path d="M0-25V20" /><circle cy="43" r="6" /></g>}
        {kind === "ball" && <><ellipse cx="0" cy="65" rx="65" ry="9" stroke="none" opacity="0.12" /><circle data-piece="" r="35" /></>}
        {kind === "power" && <g data-piece="" fill="none"><path d="M0-90V-5M-49-60A78 78 0 1 0 49-60" /></g>}
        {kind === "confetti" && Array.from({length:10},(_,i) => <rect key={i} data-piece="" width="9" height="23" rx="3" fill={["#D8A242","#698CC4","#B77BA3"][i%3]} stroke="none" />)}
        {kind === "spark" && [0,1,2,3].map(i => <path key={i} data-piece="" d="M0-27Q5-5 27 0Q5 5 0 27Q-5 5-27 0Q-5-5 0-27Z" stroke="none" />)}
    </motion.g>;
}
