import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Window } from 'happy-dom';

const script = readFileSync(new URL('./verification_state.js', import.meta.url), 'utf8');
const evaluate = new Function('document', 'location', 'getComputedStyle', 'innerHeight', 'innerWidth', `return (${script})(['._0_SRI','.search-result']);`);

async function fixture(html, check) {
    const window = new Window({ url: 'https://kagi.com/' });
    Object.defineProperty(window.document, 'readyState', { value: 'complete' });
    window.document.body.innerHTML = html;
    try { check(evaluate(window.document, window.location, window.getComputedStyle.bind(window), 800, 1000)); }
    finally { await window.happyDOM.close(); }
}

test('should detect the observed Kagi attention notice without a light-DOM iframe', async () => {
    await fixture('<form><p class="sr-only" data-fsp-status aria-live="polite">Human verification requires your attention.</p></form>', state => {
        assert.equal(state.challengeText, true);
        assert.equal(state.hasResults, false);
    });
});

test('should request a closed-shadow probe when Kagi is waiting for Turnstile', async () => {
    await fixture('<button data-fsp-submit disabled title="Please wait while Turnstile verifies your request">Search</button>', state => {
        assert.equal(state.challengePending, true);
    });
});

test('should leave a normal homepage alone and recognize Kagi results', async () => {
    await fixture('<form><input name="q"><button data-fsp-submit>Search</button></form>', state => {
        assert.equal(state.challengeText, false);
        assert.equal(state.visibleChallenge, false);
    });
    await fixture('<div class="_0_SRI search-result">A result</div>', state => assert.equal(state.hasResults, true));
});
