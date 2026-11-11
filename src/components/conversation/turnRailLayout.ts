/**
 * 轮次刻度的纵向布局（桌面导航条与移动端滑轨共用）。
 *
 * 模型：N 轮提问均匀分布在一条有界高度的轨道上，第 i 轮位于 i/(N-1) 处。
 * 轮次很多时轨道不再变长，只把刻度抽样绘制，交互仍按指针位置命中最近的那一轮。
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
 * 需要绘制的刻度下标。
 *
 * 相邻刻度间距不小于 minGapPx 时全部绘制；否则按固定步长抽样，避免刻度挤成一整条。
 * 首尾两轮始终绘制，标出轨道两端；末尾与上一个抽样点过近时用最后一轮替换它。
 */
export function sampleTickIndices(count: number, trackHeight: number, minGapPx: number): number[] {
    if (count <= 0) {
        return [];
    }
    if (count === 1) {
        return [0];
    }
    const spacing = trackHeight / (count - 1);
    const stride = spacing >= minGapPx ? 1 : Math.ceil(minGapPx / Math.max(spacing, Number.EPSILON));
    const indices: number[] = [];
    for (let index = 0; index < count; index += stride) {
        indices.push(index);
    }
    const last = count - 1;
    const tail = indices[indices.length - 1];
    if (tail !== last) {
        if (indices.length > 1 && (last - tail) * spacing < minGapPx) {
            indices[indices.length - 1] = last;
        } else {
            indices.push(last);
        }
    }
    return indices;
}
