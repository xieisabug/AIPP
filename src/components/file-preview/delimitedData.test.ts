import { describe, expect, it } from "vitest";
import { columnLabel, DelimitedData, selectionBounds } from "./delimitedData";

async function indexed(bytes: Uint8Array, separator = ",") {
    const data = new DelimitedData(bytes.length, separator, async (start, end) => bytes.slice(start, end));
    await data.index(() => {});
    return data;
}
const encode = (text: string) => new TextEncoder().encode(text);
describe("indexed delimited files", () => {
    it("keeps quoted line breaks, literal quotes, empty cells, ragged rows and original values", async () => {
        const source = '\ufeff0001,=1+1,"x,""y""\r\nz",\r\n\r\nonly\n';
        const data = await indexed(encode(source));
        expect(data.length).toBe(3);
        expect(data.columns).toBe(4);
        const slice = await data.slice(0, 10);
        expect(slice.rows).toEqual([["0001", "=1+1", 'x,"y"\r\nz', ""], [""], ["only"]]);
        expect(slice.raw[0]).toBe('0001,=1+1,"x,""y""\r\nz",');
        expect(await data.copy(selectionBounds({ row: 2, col: 2 }, { row: 0, col: 1 }))).toBe(
            '=1+1\t"x,""y""\r\nz"\n\t\n\t'
        );
    });
    it.each([false, true])("indexes UTF-16 TSV in either byte order (big endian %s)", async (big) => {
        const text = '\ufeff编号\t备注\r\n01\t"中\t文"\r\n',
            bytes = new Uint8Array(text.length * 2);
        const view = new DataView(bytes.buffer);
        for (let i = 0; i < text.length; i++) view.setUint16(i * 2, text.charCodeAt(i), !big);
        const data = await indexed(bytes, "\t");
        expect((await data.slice(1, 1)).rows).toEqual([["01", "中\t文"]]);
    });
    it("keeps quote and CRLF state across read boundaries", async () => {
        const unit = 512 * 1024;
        const record = '"' + "a".repeat(unit - 2) + '""\nend"';
        const tail = "x".repeat(unit - ((record.length + 2) % unit) - 1);
        const data = await indexed(encode(record + "\r\n" + tail + "\r\nlast"));
        expect((await data.slice(0, 1)).rows[0][0]).toBe("a".repeat(unit - 2) + '"\nend');
        expect((await data.slice(1, 2)).rows).toEqual([[tail], ["last"]]);
    });
    it("searches both directions and wraps within the starting row", async () => {
        const data = await indexed(encode("match,y,Match\nskip\nz,MATCH"));
        const find = (row: number, col: number, direction: 1 | -1) =>
            data.find("match", { row, col }, direction, () => false);
        expect(await find(0, 0, 1)).toEqual({ row: 0, col: 2 });
        expect(await find(0, 0, -1)).toEqual({ row: 2, col: 1 });
        expect(await find(2, 1, 1)).toEqual({ row: 0, col: 0 });
        expect(await data.find("absent", { row: 0, col: -1 }, 1, () => false)).toBeNull();
        expect(await data.find("match", { row: 0, col: -1 }, 1, () => true)).toBeNull();
        expect(columnLabel(702)).toBe("AAA");
    });
    it("handles empty files and reports unterminated quoted records", async () => {
        expect((await indexed(encode(""))).length).toBe(0);
        await expect(indexed(encode('a,"b\nc'))).rejects.toThrow("引号未闭合");
    });
});
