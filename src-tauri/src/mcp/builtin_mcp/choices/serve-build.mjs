// Only serves entries in the immutable snapshot. No directory access or build code runs here.
const [snapshotPath, token] = process.argv.slice(2);
const files = await Bun.file(snapshotPath).json();
const mime = {
    html: "text/html",
    js: "text/javascript",
    mjs: "text/javascript",
    css: "text/css",
    json: "application/json",
    svg: "image/svg+xml",
    png: "image/png",
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    webp: "image/webp",
    gif: "image/gif",
    ico: "image/x-icon",
    woff2: "font/woff2",
    woff: "font/woff",
    wasm: "application/wasm",
    txt: "text/plain",
};
const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch(request) {
        const url = new URL(request.url);
        const host = token + ".aipp-choice.localhost:" + server.port;
        if (request.headers.get("host") !== host || !["GET", "HEAD"].includes(request.method))
            return new Response(null, { status: 403 });
        let path;
        try {
            path = decodeURIComponent(url.pathname).slice(1) || "index.html";
        } catch {
            return new Response(null, { status: 400 });
        }
        if (!Object.hasOwn(files, path)) return new Response(null, { status: 404 });
        const origin = "http://" + host;
        const headers = {
            "Content-Type": mime[path.split(".").pop()] || "application/octet-stream",
            "X-Content-Type-Options": "nosniff",
            "Cache-Control": "no-store",
            "Access-Control-Allow-Origin": "null",
            "Referrer-Policy": "no-referrer",
            "Content-Security-Policy":
                "sandbox allow-scripts; default-src 'none'; script-src " +
                origin +
                " 'unsafe-inline'; style-src " +
                origin +
                " 'unsafe-inline'; img-src " +
                origin +
                " data: blob:; font-src " +
                origin +
                " data:; media-src " +
                origin +
                " blob:; connect-src " +
                origin +
                "; base-uri " +
                origin +
                "; form-action 'none'",
        };
        return new Response(request.method === "HEAD" ? null : Buffer.from(files[path], "base64"), { headers });
    },
});
console.log(server.port);
for await (const _ of Bun.stdin.stream()) {
}
server.stop(true);
