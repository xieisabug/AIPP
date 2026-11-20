/**
 * 内置搜索工具的设置项按「搜索服务」联动显示：
 * - 搜索服务（HTTP_SEARCH_PROVIDER）是唯一的选择：本地搜索（浏览器，仅桌面）或某个 HTTP 搜索 API；
 *   search_web 与 search_http 都按它执行（后端 search/handler.rs）。
 * - 搜索引擎只属于本地搜索，Kagi 会话链接只在引擎为 Kagi 时出现；
 * - API 地址 / Key / 超时只属于 HTTP 搜索服务；DeepSeek Fetch 只在服务为 DeepSeek 时可选；
 * - 浏览器设置（数据目录、代理、等待等）由本地搜索与本机网页抓取共用：
 *   只要搜索或抓取还用浏览器就显示，二者都改用 DeepSeek 时隐藏。
 * 隐藏的值保留在配置里（切回时恢复），后端同样只按当前服务读取，不会让隐藏项生效。
 */

export const SEARCH_PROVIDER_KEY = "HTTP_SEARCH_PROVIDER";
const LOCAL_SEARCH = "local";

const LOCAL_SEARCH_KEYS: ReadonlySet<string> = new Set(["SEARCH_ENGINE", "KAGI_SESSION_URL"]);
const HTTP_SEARCH_KEYS: ReadonlySet<string> = new Set(["HTTP_SEARCH_ENDPOINT", "HTTP_SEARCH_API_KEY", "HTTP_SEARCH_TIMEOUT_MS"]);

/** 本地搜索与本机网页抓取共用的浏览器设置。 */
export const BROWSER_SETTING_KEYS: ReadonlySet<string> = new Set([
    "USER_DATA_DIR",
    "PROXY_SERVER",
    "HEADLESS",
    "WAIT_SELECTORS",
    "WAIT_TIMEOUT_MS",
    "WAIT_POLL_MS",
    "MAX_CONCURRENT_PAGES",
    "POOL_ACQUIRE_TIMEOUT_MS",
    "DEBUG_CAPTURE_EMPTY_ARTIFACTS",
    "DEBUG_ARTIFACT_DIR",
]);

/** 未设置与旧版的 disabled 都表示本地搜索（与后端 uses_local_search 一致）。 */
export function isLocalSearch(provider: string | undefined): boolean {
    const value = (provider ?? "").trim();
    return value === "" || value === "disabled" || value === LOCAL_SEARCH;
}

/** 下拉框显示的值：旧值 disabled 显示为本地搜索；本机不能用浏览器时本地搜索显示为未选择。 */
export function displayedSearchProvider(provider: string | undefined, browserAvailable: boolean): string {
    if (!isLocalSearch(provider)) return (provider ?? "").trim();
    return browserAvailable ? LOCAL_SEARCH : "";
}

export function searchProviderOptions<T extends { value: string }>(options: readonly T[], browserAvailable: boolean): T[] {
    return options.filter((option) => browserAvailable || option.value !== LOCAL_SEARCH);
}

function usesBrowser(values: Readonly<Record<string, string>>): boolean {
    const provider = values[SEARCH_PROVIDER_KEY];
    if (isLocalSearch(provider)) return true;
    // 网页抓取：只有服务与抓取方式都是 DeepSeek 时才不经过本机浏览器
    return !(provider?.trim() === "deepseek" && values.FETCH_PROVIDER?.trim() === "deepseek");
}

export type SearchFieldPlacement = "main" | "browser" | "hidden";

/** 某个设置项在当前取值下放在哪里。 */
export function searchFieldPlacement(key: string, values: Readonly<Record<string, string>>, browserAvailable: boolean): SearchFieldPlacement {
    const provider = values[SEARCH_PROVIDER_KEY];
    const local = isLocalSearch(provider);
    if (key === SEARCH_PROVIDER_KEY) return "main";
    if (key === "SEARCH_ENGINE") return local && browserAvailable ? "main" : "hidden";
    if (key === "KAGI_SESSION_URL") return local && browserAvailable && values.SEARCH_ENGINE?.trim() === "kagi" ? "main" : "hidden";
    if (HTTP_SEARCH_KEYS.has(key)) return local ? "hidden" : "main";
    if (key === "FETCH_PROVIDER") return provider?.trim() === "deepseek" ? "main" : "hidden";
    if (BROWSER_SETTING_KEYS.has(key)) return browserAvailable && usesBrowser(values) ? "browser" : "hidden";
    return LOCAL_SEARCH_KEYS.has(key) ? "hidden" : "main";
}

/** 浏览器设置默认是否展开：本地搜索时展开，只剩本机抓取在用时收起。 */
export function browserSettingsOpenByDefault(values: Readonly<Record<string, string>>): boolean {
    return isLocalSearch(values[SEARCH_PROVIDER_KEY]);
}
