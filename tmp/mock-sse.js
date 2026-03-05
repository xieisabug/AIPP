import http from "http";

const PORT = Number(process.env.MOCK_SSE_PORT || 8787);
const DEFAULT_SEED = 20260305;

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function mulberry32(seed) {
    let t = seed >>> 0;
    return () => {
        t += 0x6D2B79F5;
        let x = Math.imul(t ^ (t >>> 15), 1 | t);
        x ^= x + Math.imul(x ^ (x >>> 7), 61 | x);
        return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
    };
}

function clampNumber(value, min, max, fallback) {
    if (!Number.isFinite(value)) return fallback;
    return Math.max(min, Math.min(max, Math.floor(value)));
}

function randomInt(rng, min, maxInclusive) {
    return min + Math.floor(rng() * (maxInclusive - min + 1));
}

function splitByCodePoints(text, rng, minChars = 1, maxChars = 4) {
    const cps = Array.from(text);
    const parts = [];
    let i = 0;
    while (i < cps.length) {
        const size = randomInt(rng, minChars, maxChars);
        parts.push(cps.slice(i, i + size).join(""));
        i += size;
    }
    return parts;
}

function buildStressText() {
    return [
        "复杂压测开始🚀\n",
        "中文 English 123 mixed。\n",
        "特殊字符: \\\"quote\\\" \\\\backslash\\\\ /slash/ <xml>&amp;\n",
        "Emoji: 😀🧪👩‍💻🔥✅\n",
        "多行内容A\n多行内容B\n多行内容C\n",
        "Markdown片段:\n```json\n{\"k\":\"值\",\"arr\":[1,2,3],\"emoji\":\"🧠\"}\n```\n",
        "再来一段：春眠不觉晓，处处闻啼鸟。\n",
        "最后收尾：THE-END-终。\n",
    ].join("");
}

function buildSseChunks({ model, seed }) {
    const rng = mulberry32(seed ^ 0x9e3779b9);
    const created = Math.floor(Date.now() / 1000);
    const id = `mock-${seed}`;
    const textParts = splitByCodePoints(buildStressText(), rng, 1, 3);

    const events = [];
    events.push({
        id,
        object: "chat.completion.chunk",
        created,
        model,
        choices: [{ index: 0, delta: { role: "assistant" }, finish_reason: null }],
    });

    for (const part of textParts) {
        events.push({
            id,
            object: "chat.completion.chunk",
            created,
            model,
            choices: [{ index: 0, delta: { content: part }, finish_reason: null }],
        });
    }

    events.push({
        id,
        object: "chat.completion.chunk",
        created,
        model,
        choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
        usage: {
            prompt_tokens: 111,
            completion_tokens: textParts.length,
            total_tokens: 111 + textParts.length,
        },
    });

    const dataBuffer = Buffer.from(events.map((e) => `data: ${JSON.stringify(e)}\n\n`).join(""), "utf8");
    const doneBuffer = Buffer.from("data: [DONE]\n\n", "utf8");
    return { dataBuffer, doneBuffer };
}

async function writeBufferByRandomChunks(res, bytes, opts) {
    const rng = mulberry32(opts.seed);
    let offset = 0;
    let chunkCount = 0;
    while (offset < bytes.length) {
        const size = randomInt(rng, opts.minChunk, opts.maxChunk);
        const end = Math.min(offset + size, bytes.length);
        res.write(bytes.subarray(offset, end));
        offset = end;
        chunkCount += 1;
        const delay = randomInt(rng, opts.minDelay, opts.maxDelay);
        if (delay > 0) await wait(delay);
    }
    return chunkCount;
}

async function readJsonBody(req) {
    const chunks = [];
    for await (const chunk of req) {
        chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    }
    if (chunks.length === 0) return {};
    try {
        return JSON.parse(Buffer.concat(chunks).toString("utf8"));
    } catch {
        return {};
    }
}

http.createServer(async (req, res) => {
    const url = new URL(req.url || "/", "http://127.0.0.1");

    if (req.method === "GET" && url.pathname === "/health") {
        res.writeHead(200, { "content-type": "application/json; charset=utf-8" });
        res.end(JSON.stringify({ ok: true, service: "mock-sse", port: PORT }));
        return;
    }

    if (!(req.method === "POST" && url.pathname === "/v1/chat/completions")) {
        res.writeHead(404).end();
        return;
    }

    req.socket.setNoDelay(true);
    const body = await readJsonBody(req);
    const model = typeof body.model === "string" && body.model ? body.model : "mock/stress-model";

    const querySeed = Number(url.searchParams.get("seed"));
    const seed = clampNumber(querySeed, 1, 0x7fffffff, DEFAULT_SEED);
    const scenario = (url.searchParams.get("scenario") || "stress").toLowerCase();
    const minChunk = clampNumber(Number(url.searchParams.get("min_chunk")), 1, 32, 1);
    const maxChunk = clampNumber(Number(url.searchParams.get("max_chunk")), minChunk, 64, 3);
    const minDelay = clampNumber(Number(url.searchParams.get("min_delay")), 0, 100, 0);
    const maxDelay = clampNumber(Number(url.searchParams.get("max_delay")), minDelay, 200, 4);

    res.writeHead(200, {
        "Content-Type": "text/event-stream; charset=utf-8",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
        "X-Mock-Scenario": scenario,
        "X-Mock-Seed": String(seed),
    });

    const { dataBuffer, doneBuffer } = buildSseChunks({ model, seed });
    const chunkOpts = { seed: seed ^ 0xa5a5a5a5, minChunk, maxChunk, minDelay, maxDelay };

    if (scenario === "invalid_utf8") {
        const cut = Math.floor(dataBuffer.length * 0.6);
        const head = dataBuffer.subarray(0, cut);
        const tail = dataBuffer.subarray(cut);
        await writeBufferByRandomChunks(res, head, chunkOpts);
        await writeBufferByRandomChunks(res, Buffer.from([0xff, 0xfe]), chunkOpts);
        await writeBufferByRandomChunks(res, tail, chunkOpts);
        await writeBufferByRandomChunks(res, doneBuffer, chunkOpts);
        res.end();
        return;
    }

    if (scenario === "truncated_utf8") {
        await writeBufferByRandomChunks(res, dataBuffer, chunkOpts);
        const prefix = Buffer.from(
            'data: {"id":"truncated","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"',
            "utf8"
        );
        const zhong = Buffer.from("终", "utf8"); // e7 bb 88
        await writeBufferByRandomChunks(res, prefix, chunkOpts);
        await writeBufferByRandomChunks(res, zhong.subarray(0, 2), chunkOpts); // 故意缺 1 字节
        res.end(); // 不发送 [DONE]
        return;
    }

    // 默认 stress 场景：合法 UTF-8，但任意字节切分，覆盖 data:/\n\n/[DONE] 边界。
    await writeBufferByRandomChunks(res, dataBuffer, chunkOpts);
    await writeBufferByRandomChunks(res, doneBuffer, chunkOpts);
    res.end();
}).listen(PORT, () => {
    console.log(
        `[mock-sse] listening on :${PORT} | scenarios: stress (default), invalid_utf8, truncated_utf8`
    );
    console.log(
        `[mock-sse] example: http://127.0.0.1:${PORT}/v1/chat/completions?scenario=stress&seed=20260305&min_chunk=1&max_chunk=3`
    );
});
