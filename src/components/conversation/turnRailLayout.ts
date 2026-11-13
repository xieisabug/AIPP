/**
 * 轮次刻度的纵向布局（桌面导航条与移动端滑轨共用）。
 *
 * 模型：N 轮提问均匀分布在一条有界高度的轨道上，第 i 轮位于 i/(N-1) 处。
 * 每一轮都必须有自己的刻度：轮次很多时轨道不再变长，只把刻度画细，
 * 交互仍按指针位置命中最近的那一轮。
 */

/** 将指针在轨道内的纵向位置映射为最近的轮次下标。 */
export function turnIndexFromOffset(offsetY: number, trackHeight: number, count: number): number {
    if (count <= 1 || trackHeight <= 0) {
        return 0;
    }
    const clamped = Math.min(Math.max(offsetY, 0), trackHeight);
    return Math.round((clamped / trackHeight) * (count - 1));
}

/** 第 index 轮刻度在轨道内的纵向位置。 */
export function tickOffset(index: number, trackHeight: number, count: number): number {
    return count <= 1 ? trackHeight / 2 : (index / (count - 1)) * trackHeight;
}

/**
 * 滑轨在消息区内的顶部位置（垂直居中）。
 *
 * `padding` 是轨道上下的额外热区，滑轨总高 = trackHeight + 2 * padding。
 * 量不到消息区高度（0）时贴顶，调用方不应在此时渲染（会全是零位刻度）。
 */
export function railOffsetTop(trackHeight: number, containerHeight: number, padding: number): number {
    return Math.max(0, (containerHeight - (trackHeight + padding * 2)) / 2);
}

/**
 * 每根刻度的高度（像素）。
 *
 * 每一轮都画自己的刻度，不做抽样；轮次挤在一起时只把刻度变薄，
 * 尺寸始终小于间距，保证每一轮都能分辨、又不会糊成一整条。
 */
export function tickThickness(count: number, trackHeight: number): number {
    if (count <= 1 || trackHeight <= 0) {
        return 3;
    }
    const spacing = trackHeight / (count - 1);
    if (spacing >= 6) {
        return 3;
    }
    return spacing >= 2.5 ? 2 : 1;
}
