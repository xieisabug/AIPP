import { describe, it, expect } from "vitest";
import { rankModels, recommendModels, recommendedCodes, type OnboardingPreset } from "./modelRecommend";
export const preset: OnboardingPreset = {
    recommended_provider: {
        api_type: "deepseek",
        name: "DeepSeek",
        endpoint: "https://example.test",
        price_hint: "",
        key_guide: [],
        models: { main: "main", light: "small" },
    },
    model_ranking: {
        main: [{ base: "vendor/model", aliases: ["alternative"] }],
        light: [{ base: "vendor/small" }],
    },
    strip_suffixes: [":free", ":beta"],
    light_keywords: ["mini", "flash"],
};
const catalog = {
    benchmarks: {
        "lab/a": [{group:"reasoning",test:"reason",score:95},{group:"context",test:"summary",score:70},{group:"safety",test:"guard",score:50}],
        "lab/b": [{group:"reasoning",test:"reason",score:40},{group:"context",test:"summary",score:80},{group:"safety",test:"guard",score:70}],
        "lab/c": [{group:"reasoning",test:"reason",score:60},{group:"context",test:"summary",score:60},{group:"safety",test:"guard",score:98}],
    },
    providers: { direct: {endpoints:["https://direct.test"],models:{
        a:{model:"lab/a",input:1,output:1},b:{model:"lab/b",input:0.2,output:0.3},c:{model:"lab/c",input:1,output:2},
    }}, relay: {endpoints:["https://relay.test/v1"],models:{a:{model:"lab/a",input:8,output:17}}} },
};
const priced = {...preset, recommendation_catalog:catalog};
describe("catalog recommendations", () => {
    it("uses role-specific comparable benchmarks and prices", () => {
        const picks=recommendModels(["a","b","c"], "openai_api", priced, "https://direct.test/");
        expect([picks.main,picks.light,picks.review].map(rows=>rows[0].code)).toEqual(["a","b","c"]);
    });
    it("uses exact endpoint prices and never borrows a price from a similar model or host", () => {
        const ranked=rankModels(["https://relay.test/v1","https://direct.test","https://relay.test/other"].map((endpoint,i)=>({
            id:i+1,api_type:"openai_api",endpoint,codes:["a"],
        })),priced).main;
        expect(ranked.map(row=>row.providerId)).toEqual([2,1,3]);
        expect(ranked.map(row=>row.value)).toEqual([0,0,0]);
        expect(ranked[2].cost).toBe(Infinity);
    });
    it("uses unambiguous underlying benchmarks on private relays without copying public prices", () => {
        const picks = recommendModels(["a","b","c"], "openai_api", priced, "https://private.test");
        expect(picks.main[0].code).toBe("a");
        expect(picks.review[0].code).toBe("c");
        expect(picks.main.every(row => row.cost === Infinity)).toBe(true);
    });
    it("assigns zero for missing and incomparable evidence; a known zero price is valid", () => {
        const data = {...preset, recommendation_catalog:{
            benchmarks:{"lab/a":[{group:"agent",test:"Terminal:2.1:A",score:99}],
                "lab/b":[{group:"agent",test:"Terminal:4.0:B",score:5}]},
            providers:{fixture:{endpoints:["https://fixture.test"],models:{
                a:{model:"lab/a",input:7,output:7},b:{model:"lab/b",input:1,output:1},
                free:{input:0,output:0}, unknown:{}, image:{chat:false}, text:{tools:false,input:0,output:0},
            }}},
        }};
        const picks=recommendModels(["a","b","free","unknown","image","text"],"openai_api",data,"https://fixture.test");
        expect(picks.main.map(row=>row.code)).toEqual(["free","b","a","unknown"]);
        expect(picks.main.every(row=>row.value===0)).toBe(true);
        expect(picks.light.some(row=>row.code==="text")).toBe(true);
    });
    it("retains the ten-model cap for expert imports", () => {
        const picks=recommendModels(Array.from({length:20},(_,i)=>String(i)),"ollama",preset);
        expect(recommendedCodes(picks,["manual"])).toHaveLength(10);
        expect(recommendedCodes(picks,["manual"])[0]).toBe("manual");
    });
});
