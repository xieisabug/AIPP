export type Cell = { row: number; col: number };
export type Bounds = { top: number; bottom: number; left: number; right: number };
export type SheetSlice = { start: number; rows: string[][]; raw: string[] };
export type SheetProgress = { rows: number; columns: number; complete: boolean };
type Read = (start: number, end: number) => Promise<Uint8Array>;
const BLOCK = 512 * 1024;

export function selectionBounds(a: Cell, b: Cell): Bounds {
    return {
        top: Math.min(a.row, b.row),
        bottom: Math.max(a.row, b.row),
        left: Math.min(a.col, b.col),
        right: Math.max(a.col, b.col),
    };
}

export function columnLabel(index: number) {
    let label = "";
    for (let n = index + 1; n > 0; n = Math.floor((n - 1) / 26))
        label = String.fromCharCode(65 + ((n - 1) % 26)) + label;
    return label;
}

function fields(record: string, separator: string) {
    const cells: string[] = [];
    let quoted = false,
        start = 0;
    function push(end: number) {
        const value = record.slice(start, end);
        cells.push(value.startsWith('"') && value.endsWith('"') ? value.slice(1, -1).replace(/""/g, '"') : value);
    }
    for (let i = 0; i < record.length; i++) {
        if (record[i] === '"' && (quoted || i === start)) quoted = !quoted;
        else if (!quoted && record[i] === separator) {
            push(i);
            start = i + 1;
        }
        // Both quotes of an escaped pair belong to the same quoted field.
        if (record[i] === '"' && !quoted && record[i + 1] === '"' && i > start) {
            quoted = true;
            i++;
        }
    }
    push(record.length);
    return cells;
}

export class DelimitedData {
    private offsets = [0];
    private decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
    private width = 1;
    private bom = 0;
    private bigEndian = false;
    columns = 0;
    get length() {
        return this.offsets.length - 1;
    }
    constructor(private size: number, private separator: string, private read: Read) {}

    async index(notify: (progress: SheetProgress) => void) {
        let quoted = false,
            fieldStart = true,
            pendingCR = false,
            columns = 1,
            notified = 0;
        const finish = (end: number) => {
            this.offsets.push(end);
            this.columns = Math.max(this.columns, columns);
            columns = 1;
            fieldStart = true;
        };
        for (let start = 0; start < this.size; start += BLOCK) {
            const bytes = await this.read(start, Math.min(this.size, start + BLOCK));
            if (start === 0) {
                const [a, b, c] = bytes;
                if ((a === 255 && b === 254) || (a === 254 && b === 255)) {
                    this.bigEndian = a === 254;
                    this.width = 2;
                    this.bom = 2;
                    this.decoder = new TextDecoder(this.bigEndian ? "utf-16be" : "utf-16le", {
                        fatal: true,
                        ignoreBOM: true,
                    });
                } else if (a === 239 && b === 187 && c === 191) this.bom = 3;
                this.offsets[0] = this.bom;
            }
            for (let i = start === 0 ? this.bom : 0; i < bytes.length; i += this.width) {
                const ch =
                    this.width === 1
                        ? bytes[i]
                        : this.bigEndian
                        ? (bytes[i] << 8) | bytes[i + 1]
                        : bytes[i] | (bytes[i + 1] << 8);
                const pos = start + i;
                if (pendingCR) {
                    pendingCR = false;
                    if (ch === 10) {
                        finish(pos + this.width);
                        continue;
                    }
                    finish(pos);
                }
                if (ch === 34 && (quoted || fieldStart)) {
                    quoted = !quoted;
                    // After a closing quote the next quote may be its escaped partner.
                    fieldStart = !quoted;
                } else if (!quoted && ch === this.separator.charCodeAt(0)) {
                    columns++;
                    fieldStart = true;
                } else if (!quoted && ch === 13) pendingCR = true;
                else if (!quoted && ch === 10) finish(pos + this.width);
                else fieldStart = false;
            }
            if (this.length && (!notified || performance.now() - notified > 100)) {
                notify({ rows: this.length, columns: this.columns, complete: false });
                notified = performance.now();
            }
        }
        if (quoted) throw new Error("表格末尾的引号未闭合，请检查文件格式");
        if (this.offsets[this.length] < this.size) finish(this.size);
        notify({ rows: this.length, columns: this.columns, complete: true });
    }

    private async bytes(start: number, end: number) {
        if (end - start <= BLOCK) return this.read(start, end);
        const result = new Uint8Array(end - start);
        for (let p = start; p < end; p += BLOCK) result.set(await this.read(p, Math.min(end, p + BLOCK)), p - start);
        return result;
    }

    async slice(start: number, count: number, query?: string): Promise<SheetSlice> {
        const end = Math.min(this.length, start + count);
        const rows: string[][] = [],
            raw: string[] = [];
        if (start >= end) return { start, rows, raw };
        const base = this.offsets[start];
        const data = await this.bytes(base, this.offsets[end]);
        if (query && !query.includes('"') && !this.decoder.decode(data).toLowerCase().includes(query))
            return { start, rows, raw };
        for (let i = start; i < end; i++) {
            const record = this.decoder
                .decode(data.subarray(this.offsets[i] - base, this.offsets[i + 1] - base))
                .replace(/(?:\r\n|\n|\r)$/, "");
            raw.push(record);
            rows.push(fields(record, this.separator));
        }
        return { start, rows, raw };
    }

    async find(text: string, from: Cell, direction: 1 | -1, cancelled: () => boolean): Promise<Cell | null> {
        const query = text.toLowerCase();
        if (!query || !this.length) return null;
        const origin = Math.min(from.row, this.length - 1);
        let remaining = this.length + 1,
            row = origin,
            first = true;
        while (remaining > 0 && !cancelled()) {
            // Fill a byte range with complete records to avoid one IPC round trip per small row batch.
            let count = 1;
            let upper = Math.min(remaining, direction === 1 ? this.length - row : row + 1);
            while (count < upper) {
                const middle = Math.ceil((count + upper) / 2);
                const bytes =
                    direction === 1
                        ? this.offsets[row + middle] - this.offsets[row]
                        : this.offsets[row + 1] - this.offsets[row + 1 - middle];
                if (bytes <= BLOCK) count = middle;
                else upper = middle - 1;
            }
            const start = direction === 1 ? row : row - count + 1;
            const block = await this.slice(start, count, query);
            for (let j = 0; j < block.rows.length; j++) {
                const r = row + j * direction,
                    cells = block.rows[r - start];
                const col = first && j === 0 ? from.col + direction : direction === 1 ? 0 : cells.length - 1;
                const begin = direction === 1 ? Math.max(0, col) : Math.min(cells.length - 1, col);
                for (let c = begin; c >= 0 && c < cells.length; c += direction) {
                    if (cells[c].toLowerCase().includes(query)) return { row: r, col: c };
                }
            }
            first = false;
            remaining -= count;
            row = (row + direction * count + this.length) % this.length;
        }
        return null;
    }

    async copy(bounds: Bounds) {
        if ((bounds.bottom - bounds.top + 1) * (bounds.right - bounds.left + 1) > 2_000_000) {
            throw new Error("单次复制最多支持 200 万个单元格，请缩小选区");
        }
        const records: string[] = [];
        for (let row = bounds.top; row <= bounds.bottom; row += 128) {
            const batch = await this.slice(row, Math.min(128, bounds.bottom - row + 1));
            for (const cells of batch.rows) {
                const values = [];
                for (let c = bounds.left; c <= bounds.right; c++) {
                    const v = cells[c] ?? "";
                    values.push(/[\t\r\n"]/.test(v) ? '"' + v.replace(/"/g, '""') + '"' : v);
                }
                records.push(values.join("\t"));
            }
        }
        return records.join("\n");
    }
}
