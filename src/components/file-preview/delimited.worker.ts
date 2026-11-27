import { DelimitedData, type Bounds, type Cell } from "./delimitedData";

export type SheetTask =
    | { op: "open"; url: string; size: number; delimiter: string }
    | { op: "slice"; start: number; count: number }
    | { op: "find"; query: string; from: Cell; direction: 1 | -1 }
    | { op: "copy"; bounds: Bounds };

let data: DelimitedData;
let currentSearch = 0;
self.onmessage = async ({ data: msg }: { data: SheetTask & { id: number } }) => {
    try {
        let result;
        switch (msg.op) {
            case "open":
                data = new DelimitedData(msg.size, msg.delimiter, async (start, end) => {
                    const res = await fetch(msg.url, { headers: { Range: `bytes=${start}-${end - 1}` } });
                    if (res.status !== 206)
                        throw new Error(
                            res.ok
                                ? "预览服务未返回请求的数据范围"
                                : `读取表格失败（${res.status}）：${await res.text()}`
                        );
                    const bytes = new Uint8Array(await res.arrayBuffer());
                    if (bytes.length !== end - start) throw new Error("表格数据长度发生变化，请重新打开文件");
                    return bytes;
                });
                await data.index((progress) => self.postMessage({ progress }));
                break;
            case "slice":
                result = await data.slice(msg.start, msg.count);
                break;
            case "find":
                currentSearch = msg.id;
                result = await data.find(msg.query, msg.from, msg.direction, () => currentSearch !== msg.id);
                break;
            case "copy":
                result = await data.copy(msg.bounds);
                break;
        }
        self.postMessage({ id: msg.id, result });
    } catch (e) {
        self.postMessage({ id: msg.id, error: e instanceof Error ? e.message : String(e) });
    }
};
