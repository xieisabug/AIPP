// Python webbrowser runs this instead of opening a browser itself. The UI
// presents the validated link; this process never receives OAuth credentials.
process.stderr.on('error', () => process.exit(0));
process.stderr.write(`__AIPP_ANTIGRAVITY_AUTH_URL__${JSON.stringify(process.argv[2])}\n`, () => process.exit(0));
