export type ProviderEndpointStatus =
    | { kind: "unset"; message: string }
    | { kind: "invalid"; message: string }
    | { kind: "custom"; message: string };

export function getProviderEndpointStatus(raw: string): ProviderEndpointStatus {
    const endpoint = raw.trim();
    if (!endpoint) {
        return { kind: "unset", message: "接口地址未填写，将使用该提供商的默认端点。" };
    }
    const invalid: ProviderEndpointStatus = {
        kind: "invalid",
        message: "接口地址无效：请填写完整的 HTTP(S) 基址，不含空白、反斜杠、账号密码、查询参数或片段。",
    };
    if (!/^https?:\/\/[^/]/i.test(endpoint) || /[\s\\\u0000-\u001f\u007f]/u.test(endpoint)) {
        return invalid;
    }
    try {
        const url = new URL(endpoint);
        if (!url.hostname || url.username || url.password || endpoint.includes("?") || endpoint.includes("#")) {
            return invalid;
        }
        if (!url.pathname.endsWith("/")) url.pathname += "/";
        return { kind: "custom", message: `请求基址：${url.href}` };
    } catch {
        return invalid;
    }
}
