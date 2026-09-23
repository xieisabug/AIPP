import { chromium } from 'playwright';
import { readFile, writeFile, mkdir } from 'node:fs/promises';

const base = process.env.CHAT_PERF_URL || 'http://127.0.0.1:4179';
const option = (name, fallback) => process.argv.find(arg => arg.startsWith(`--${name}=`))?.split('=')[1] || fallback;
const channel = option('channel', process.env.CHAT_PERF_CHANNEL || 'msedge');
if (!['chrome', 'msedge'].includes(channel)) throw new Error('Use an installed chrome or msedge channel. No browser download is performed.');
const headed = process.argv.includes('--headed');
const repeats = Number(option('repeats', '1'));
const seconds = Number(option('seconds', '5'));
if (!Number.isInteger(repeats) || repeats < 1 || repeats > 10 || !Number.isFinite(seconds) || seconds < 5 || seconds > 120) throw new Error('Invalid repeats or seconds');
const requestedCases = option('cases', '').split(',').filter(Boolean).map(Number);
const manifest = JSON.parse(await readFile('tmp/chat-perf/fixtures/manifest.json', 'utf8'));
const runDirectory = `tmp/chat-perf/results/${process.platform}-${channel}-${headed ? 'headed' : 'headless'}-${Date.now()}`;
await mkdir(runDirectory, { recursive: true });
const browser = await chromium.launch({ headless: !headed, channel });
console.log(`Installed ${channel} ${browser.version()}, headed=${headed}, output=${runDirectory}`);
const results = [];
try {
    for (const fixture of manifest.cases.filter(c => !requestedCases.length || requestedCases.includes(c.id))) {
        const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
        const errors = [];
        page.on('pageerror', e => errors.push(e.message));
        await page.goto(base);
        await page.bringToFront();
        await page.getByLabel('Conversation', { exact: true }).selectOption(String(fixture.id));
        await page.waitForFunction(id => document.querySelector('.stats')?.textContent.includes(`${id} messages`), fixture.messages);
        await page.locator('[data-message-item]').first().waitFor({ timeout: 60000 });
        await page.getByRole('spinbutton', { name: 'Seconds', exact: true }).fill(String(seconds));
        for (let repeat = 0; repeat < repeats; repeat++) {
            await page.evaluate(() => { delete window.__CHAT_BROWSER_PERF_RESULT__; });
            await page.getByTitle('Run scroll benchmark').click();
            await page.waitForFunction(() => window.__CHAT_BROWSER_PERF_RESULT__, undefined, { timeout: seconds * 2000 + 60000 });
            const result = await page.evaluate(() => window.__CHAT_BROWSER_PERF_RESULT__);
            result.automationPageErrors = [...errors];
            result.automation = { channel, browserVersion: browser.version(), headed, repeat: repeat + 1, cache: repeat ? 'warm' : 'first-scroll' };
            results.push(result);
            await page.screenshot({ path: `${runDirectory}/case-${fixture.id}-${repeat + 1}.png` });
            console.log(JSON.stringify({ id: fixture.id, valid: result.valid, p95: result.metrics.p95FrameMs,
                previews: result.previewCoverage, diagnostics: result.diagnostics, errors: result.errors }));
            await writeFile(`${runDirectory}/results.json`, JSON.stringify(results, null, 2));
        }
        await page.close();
    }
    const mobile = await browser.newPage({ viewport: { width: 390, height: 844 } });
    await mobile.goto(base);
    await mobile.locator('[data-message-item]').first().waitFor({ timeout: 60000 });
    await mobile.screenshot({ path: `${runDirectory}/mobile.png` });
    console.log('Mobile horizontal overflow:', await mobile.evaluate(() => document.documentElement.scrollWidth > innerWidth));
    await mobile.close();
    const unexpectedFailures = results.filter(r => !r.valid && !r.metrics.warning);
    console.log(JSON.stringify({ total: results.length, valid: results.filter(r => r.valid).length,
        noScroll: results.filter(r => r.metrics.warning).length, unexpectedFailures: unexpectedFailures.length, runDirectory }));
    if (unexpectedFailures.length || results.some(r => r.automationPageErrors.length)) process.exitCode = 1;
} finally {
    await browser.close();
}
