export interface VectorTransform {
    x?: number;
    y?: number;
    rotate?: number;
    scale?: number;
    scaleX?: number;
    scaleY?: number;
}

export interface VectorLayer {
    type: "path" | "rect" | "circle" | "ellipse";
    d?: string;
    x?: number;
    y?: number;
    width?: number;
    height?: number;
    rx?: number;
    ry?: number;
    cx?: number;
    cy?: number;
    r?: number;
    fill?: string;
    fillOpacity?: number;
    stroke?: string;
    strokeWidth?: number;
    strokeOpacity?: number;
    strokeLinecap?: "butt" | "round" | "square";
    strokeLinejoin?: "miter" | "round" | "bevel";
    opacity?: number;
    whiteEdge?: boolean;
    transform?: VectorTransform;
}

export interface VectorColorPalette {
    primary: string;
    secondary: string;
    core: string;
}

export function resolveVectorColor(token: string | undefined, palette: VectorColorPalette): string | undefined {
    if (!token) return undefined;
    switch (token.toLowerCase()) {
        case "primary":
            return palette.primary;
        case "secondary":
            return palette.secondary;
        case "white":
            return "#FFFFFF";
        case "core":
            return palette.core;
        case "none":
            return "none";
        case "white-edge":
            return "#E4E4E4";
        default:
            return token;
    }
}

export function formatVectorTransform(transform?: VectorTransform): string | undefined {
    if (!transform) return undefined;
    const parts: string[] = [];
    if (transform.x !== undefined || transform.y !== undefined) {
        parts.push(`translate(${transform.x ?? 0} ${transform.y ?? 0})`);
    }
    if (transform.rotate !== undefined) {
        parts.push(`rotate(${transform.rotate})`);
    }
    if (transform.scaleX !== undefined || transform.scaleY !== undefined) {
        const sx = transform.scaleX ?? transform.scale ?? 1;
        const sy = transform.scaleY ?? transform.scale ?? 1;
        parts.push(`scale(${sx} ${sy})`);
    } else if (transform.scale !== undefined) {
        parts.push(`scale(${transform.scale})`);
    }
    return parts.length > 0 ? parts.join(" ") : undefined;
}

export function DeclarativeArtwork({
    layers,
    palette,
}: {
    layers: VectorLayer[];
    palette: VectorColorPalette;
}) {
    return <>
        {layers.map((layer, index) => {
            const fill = resolveVectorColor(layer.fill, palette);
            const isWhiteEdge = !!layer.whiteEdge && fill?.toUpperCase() === "#FFFFFF";
            const stroke = isWhiteEdge ? "#E4E4E4" : resolveVectorColor(layer.stroke, palette);
            const strokeWidth = isWhiteEdge ? 1 : layer.strokeWidth;
            const common = {
                fill: fill ?? "none",
                fillOpacity: layer.fillOpacity,
                stroke,
                strokeWidth,
                strokeOpacity: layer.strokeOpacity,
                strokeLinecap: layer.strokeLinecap,
                strokeLinejoin: layer.strokeLinejoin ?? (stroke ? "round" : undefined),
                opacity: layer.opacity,
                transform: formatVectorTransform(layer.transform),
                vectorEffect: isWhiteEdge ? ("non-scaling-stroke" as const) : undefined,
            };

            switch (layer.type) {
                case "path":
                    return layer.d ? <path key={index} d={layer.d} {...common} /> : null;
                case "rect":
                    return <rect key={index} x={layer.x ?? 0} y={layer.y ?? 0} width={layer.width ?? 0} height={layer.height ?? 0} rx={layer.rx} ry={layer.ry} {...common} />;
                case "circle":
                    return <circle key={index} cx={layer.cx ?? 0} cy={layer.cy ?? 0} r={layer.r ?? 0} {...common} />;
                case "ellipse":
                    return <ellipse key={index} cx={layer.cx ?? 0} cy={layer.cy ?? 0} rx={layer.rx ?? 0} ry={layer.ry ?? 0} {...common} />;
                default:
                    return null;
            }
        })}
    </>;
}
