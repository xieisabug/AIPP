import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(process.argv[2] || '.');
const port = Number(process.argv[3] || 4179);
const mime = { '.html':'text/html; charset=utf-8', '.js':'text/javascript', '.css':'text/css',
    '.json':'application/json', '.png':'image/png', '.svg':'image/svg+xml', '.woff':'font/woff',
    '.woff2':'font/woff2', '.ttf':'font/ttf', '.jpg':'image/jpeg', '.webp':'image/webp' };
export const server = createServer(async (req,res) => {
    try {
        if (req.method !== 'GET' && req.method !== 'HEAD') { res.writeHead(405).end(); return; }
        const url = new URL(req.url, `http://127.0.0.1:${port}`);
        const file = path.resolve(root, '.' + decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname));
        if (!file.startsWith(root + path.sep)) { res.writeHead(403).end(); return; }
        const info = await stat(file);
        if (!info.isFile()) { res.writeHead(404).end(); return; }
        res.writeHead(200, { 'Content-Type': mime[path.extname(file)] || 'application/octet-stream',
            'Content-Length': info.size, 'Cache-Control':'no-store', 'X-Content-Type-Options':'nosniff' });
        res.end(req.method === 'HEAD' ? undefined : await readFile(file));
    } catch { res.writeHead(404).end('Not found'); }
});
server.on('error', error => { console.error(error.message); process.exitCode = 1; });
server.listen(port, '127.0.0.1', () => console.log(`Chat benchmark: http://127.0.0.1:${port} (${root})`));
