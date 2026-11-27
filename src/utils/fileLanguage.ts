const languages = new Map<string, string>();
for (const [language, extensions] of Object.entries({
    markdown: "md markdown mdown mkd",
    text: "txt log",
    diff: "diff patch",
    typescript: "ts tsx mts cts",
    javascript: "js jsx mjs cjs",
    rust: "rs",
    python: "py pyi",
    go: "go",
    java: "java",
    kotlin: "kt kts",
    scala: "scala",
    swift: "swift",
    dart: "dart",
    c: "c h",
    cpp: "cc cpp cxx hh hpp hxx",
    csharp: "cs",
    ruby: "rb",
    php: "php",
    bash: "sh bash zsh",
    powershell: "ps1 psm1",
    bat: "bat cmd",
    lua: "lua",
    html: "html htm",
    css: "css",
    scss: "scss",
    less: "less",
    vue: "vue",
    svelte: "svelte",
    json: "json jsonc",
    xml: "xml",
    yaml: "yaml yml",
    toml: "toml",
    ini: "ini",
    sql: "sql",
    gdscript: "gd",
})) {
    for (const extension of extensions.split(" ")) languages.set(extension, language);
}

export function inferLanguageFromPath(path?: string): string | undefined {
    const name = path?.split(/[\\/]/).pop()?.toLowerCase();
    if (name === "dockerfile") return "dockerfile";
    if (name === "makefile" || name === "gnumakefile") return "makefile";
    if (name === "readme" || name === "license") return "text";
    const extension = path?.match(/\.([a-z\d]+)$/i)?.[1];
    return extension ? languages.get(extension.toLowerCase()) : undefined;
}
