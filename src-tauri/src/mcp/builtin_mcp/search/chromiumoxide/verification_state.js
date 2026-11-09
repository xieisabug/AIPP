(sels) => {
        const visible = el => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return r.width > 20 && r.height > 20 && r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth && s.visibility !== 'hidden' && s.display !== 'none'; };
        const text = (document.body?.innerText || '').slice(0, 4000).toLowerCase();
        // Kagi exposes the closed-shadow Turnstile state in a screen-reader status node.
        const notices = [...document.querySelectorAll('[data-fsp-status]')].map(el => el.textContent || '').join(' ').toLowerCase();
        const challengePending = [...document.querySelectorAll('button[data-fsp-submit][disabled]')].some(el => /turnstile/i.test(el.title));
        const answered = [...document.querySelectorAll('[name="cf-turnstile-response"],[name="g-recaptcha-response"],[name="h-captcha-response"]')].some(el => !!el.value?.trim());
        return { url: location.origin + location.pathname, title: document.title,
            ready: document.readyState, hasBody: !!document.body,
            hasResults: sels.length ? sels.some(s => !!document.querySelector(s)) : !!document.body && document.readyState === 'complete',
            visibleChallenge: !answered && [...document.querySelectorAll('iframe')].some(f => /challenges\.cloudflare\.com|recaptcha.*(?:anchor|bframe)|hcaptcha\.com/.test(f.src) && visible(f)),
            challengePending,
            challengeText: /verif(?:y|ying) (?:you(?:’|')re|you are) human|checking your browser|请完成.{0,6}(?:验证|验证码)/.test(text)
                || /human verification requires your attention/.test(notices)
        };
    }
