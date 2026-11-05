import { Star } from "lucide-react";

const colors = ["#246bd5", "#bb3156", "#9353ba", "#147764", "#9a612e", "#5365a9", "#c14a28"];

export default function ButlerAvatar({ id, chief = false, size = 32 }: { id: number; chief?: boolean; size?: number }) {
    return <span aria-hidden="true" className="butler-avatar relative inline-flex shrink-0 items-center justify-center"
        style={{ width: size, height: size, backgroundColor: chief ? "#202127" : colors[Math.abs(id) % colors.length] }}>
        <span className="butler-eyes" />
        {chief && size >= 30 && <Star className="absolute -bottom-0.5 -right-1 size-4 fill-amber-400 text-background" />}
    </span>;
}
