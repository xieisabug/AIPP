import { describe, expect, it } from "vitest";
import {
    browserSettingsOpenByDefault,
    displayedSearchProvider,
    isLocalSearch,
    searchFieldPlacement,
    searchProviderOptions,
} from "./searchToolFields";

const place = (key: string, values: Record<string, string>, browser = true) => searchFieldPlacement(key, values, browser);

describe("searchToolFields", () => {
    it("treats unset and the legacy disabled value as local search", () => {
        for (const value of [undefined, "", "disabled", "local", " local "]) expect(isLocalSearch(value)).toBe(true);
        for (const value of ["deepseek", "tavily", "brave", "searxng"]) expect(isLocalSearch(value)).toBe(false);
        expect(displayedSearchProvider("disabled", true)).toBe("local");
        expect(displayedSearchProvider(undefined, false)).toBe("");
        expect(displayedSearchProvider("tavily", false)).toBe("tavily");
    });

    it("offers local search only where a browser is available", () => {
        const options = [{ value: "local" }, { value: "deepseek" }, { value: "tavily" }];
        expect(searchProviderOptions(options, true).map((o) => o.value)).toEqual(["local", "deepseek", "tavily"]);
        expect(searchProviderOptions(options, false).map((o) => o.value)).toEqual(["deepseek", "tavily"]);
    });

    it("shows the engine for local search and the Kagi link only for Kagi", () => {
        const local = { HTTP_SEARCH_PROVIDER: "local", SEARCH_ENGINE: "google" };
        expect(place("HTTP_SEARCH_PROVIDER", local)).toBe("main");
        expect(place("SEARCH_ENGINE", local)).toBe("main");
        expect(place("KAGI_SESSION_URL", local)).toBe("hidden");
        expect(place("KAGI_SESSION_URL", { ...local, SEARCH_ENGINE: "kagi" })).toBe("main");
        for (const key of ["HTTP_SEARCH_ENDPOINT", "HTTP_SEARCH_API_KEY", "HTTP_SEARCH_TIMEOUT_MS", "FETCH_PROVIDER"]) {
            expect(place(key, local)).toBe("hidden");
        }
        expect(place("USER_DATA_DIR", local)).toBe("browser");
        expect(place("SEARCH_ENGINE", local, false)).toBe("hidden");
    });

    it("hides engine settings for an HTTP service and shows its API settings", () => {
        const tavily = { HTTP_SEARCH_PROVIDER: "tavily", SEARCH_ENGINE: "kagi" };
        expect(place("SEARCH_ENGINE", tavily)).toBe("hidden");
        expect(place("KAGI_SESSION_URL", tavily)).toBe("hidden");
        expect(place("HTTP_SEARCH_API_KEY", tavily)).toBe("main");
        expect(place("HTTP_SEARCH_ENDPOINT", tavily)).toBe("main");
        expect(place("FETCH_PROVIDER", tavily)).toBe("hidden");
        // fetch_url still fetches locally, so browser settings stay reachable
        expect(place("PROXY_SERVER", tavily)).toBe("browser");
    });

    it("offers DeepSeek Fetch only for DeepSeek and drops browser settings when nothing uses the browser", () => {
        const deepseek = { HTTP_SEARCH_PROVIDER: "deepseek", FETCH_PROVIDER: "local" };
        expect(place("FETCH_PROVIDER", deepseek)).toBe("main");
        expect(place("HEADLESS", deepseek)).toBe("browser");
        expect(place("HEADLESS", { ...deepseek, FETCH_PROVIDER: "deepseek" })).toBe("hidden");
        expect(place("HEADLESS", deepseek, false)).toBe("hidden");
    });

    it("opens browser settings by default only for local search", () => {
        expect(browserSettingsOpenByDefault({ HTTP_SEARCH_PROVIDER: "local" })).toBe(true);
        expect(browserSettingsOpenByDefault({})).toBe(true);
        expect(browserSettingsOpenByDefault({ HTTP_SEARCH_PROVIDER: "brave" })).toBe(false);
    });
});
