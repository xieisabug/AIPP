export interface SlashNamespaceCompletionContext {
    kind: "namespace";
    triggerStart: number;
    replaceStart: number;
    replaceEnd: number;
    query: string;
}

export interface SlashSkillCompletionContext {
    kind: "skill";
    namespace: "skills";
    triggerStart: number;
    replaceStart: number;
    replaceEnd: number;
    query: string;
}

export type SlashCompletionContext =
    | SlashNamespaceCompletionContext
    | SlashSkillCompletionContext;

function isNamespaceChar(char: string): boolean {
    return /^[a-zA-Z0-9_-]$/.test(char);
}

function isSlashBoundary(value: string, index: number): boolean {
    return index === 0 || /\s/.test(value[index - 1]);
}

function findInvocationEnd(
    value: string,
    openParenIndex: number,
    fallbackEnd: number,
): number {
    let depth = 1;
    let escaped = false;

    for (let index = openParenIndex + 1; index < value.length; index += 1) {
        const char = value[index];

        if (escaped) {
            escaped = false;
            continue;
        }

        if (char === "\\") {
            escaped = true;
            continue;
        }

        if (char === "(") {
            depth += 1;
            continue;
        }

        if (char === ")") {
            depth -= 1;
            if (depth === 0) {
                return index + 1;
            }
        }
    }

    return fallbackEnd;
}

export function escapeSlashArgument(argument: string): string {
    return argument
        .replace(/\\/g, "\\\\")
        .replace(/\(/g, "\\(")
        .replace(/\)/g, "\\)");
}

export function buildSkillInvocation(invokeName: string): string {
    return `/skills(${escapeSlashArgument(invokeName)})`;
}

export function getSlashCompletionContext(
    value: string,
    cursorPosition: number,
): SlashCompletionContext | null {
    let latestContext: SlashCompletionContext | null = null;

    for (let index = 0; index < cursorPosition; index += 1) {
        if (value[index] !== "/" || !isSlashBoundary(value, index)) {
            continue;
        }

        let namespaceEnd = index + 1;
        while (
            namespaceEnd < value.length &&
            isNamespaceChar(value[namespaceEnd])
        ) {
            namespaceEnd += 1;
        }

        if (cursorPosition <= namespaceEnd) {
            latestContext = {
                kind: "namespace",
                triggerStart: index,
                replaceStart: index,
                replaceEnd: namespaceEnd,
                query: value.slice(index + 1, cursorPosition),
            };
            continue;
        }

        if (namespaceEnd >= value.length || value[namespaceEnd] !== "(") {
            continue;
        }

        const namespace = value.slice(index + 1, namespaceEnd).toLowerCase();
        const invocationEnd = findInvocationEnd(value, namespaceEnd, cursorPosition);
        if (cursorPosition <= namespaceEnd || cursorPosition > invocationEnd) {
            continue;
        }

        if (namespace === "skills") {
            latestContext = {
                kind: "skill",
                namespace: "skills",
                triggerStart: index,
                replaceStart: index,
                replaceEnd: invocationEnd,
                query: value.slice(namespaceEnd + 1, cursorPosition),
            };
        }
    }

    return latestContext;
}
