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
        expect(new Set(ranked.map(row => row.quality)).size).toBe(1);
        expect(ranked[0].score).toBeGreaterThan(ranked[1].score);
        expect(ranked[2].cost).toBe(Infinity);
    });
    it("uses unambiguous underlying benchmarks on private relays without copying public prices", () => {
        const picks = recommendModels(["a","b","c"], "openai_api", priced, "https://private.test");
        expect(picks.main[0].code).toBe("a");
        expect(picks.review[0].code).toBe("c");
        expect(picks.main.every(row => row.cost === Infinity)).toBe(true);
    });
    it("gives no ability credit for isolated tests and recognizes free and unknown prices", () => {
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
        expect(picks.main.every(row=>row.quality === 0)).toBe(true);
        expect(picks.light.some(row=>row.code==="text")).toBe(true);
    });
    it("retains the ten-model cap for expert imports", () => {
        const picks=recommendModels(Array.from({length:20},(_,i)=>String(i)),"ollama",preset);
        expect(recommendedCodes(picks,["manual"])).toHaveLength(10);
        expect(recommendedCodes(picks,["manual"])[0]).toBe("manual");
    });

    it("balances low prices and measured capability for light tasks", () => {
        const models = {tiny:{input:0.1,output:0.1}, better:{input:0.12,output:0.12}, costly:{input:4,output:4}};
        const data = {...preset, recommendation_catalog:{providers:{direct:{endpoints:["https://direct.test"],models}},
            benchmarks:{better:[{group:"instruction",test:"i",score:55}],costly:[{group:"instruction",test:"i",score:98}]}}};
        const picks = recommendModels(Object.keys(models),"openai_api",data,"https://direct.test");
        expect(picks.light.map(row => row.code)).toEqual(["better","tiny","costly"]);
        expect(picks.main[0].code).toBe("costly");
        models.tiny.input = models.tiny.output = 0;
        expect(recommendModels(Object.keys(models),"openai_api",data,"https://direct.test").light[0].code).toBe("tiny");
    });

    it("keeps existing scores stable when unrelated selectable candidates change", () => {
        const source = (codes: string[]) => recommendModels(codes, "openai_api", priced, "https://direct.test");
        const small = source(["a","b"]);
        const large = source(["c","b","a"]);
        for (const role of ["main","light","review"] as const) {
            expect(large[role].filter(row => row.code !== "c")).toEqual(small[role]);
        }
    });

    it("uses only the deployed effort and the newest composite version", () => {
        const aa = (effort: string, score: number, version = "5.0") => ({group:"reasoning",test:"aa:intelligence:"+version,effort,score});
        const data = {...preset, recommendation_catalog: {
            providers:catalog.providers, benchmarks:{}, identities:{"lab/a":{effort:"medium"},"lab/b":{effort:"high"}},
            independent_benchmarks:{
                "lab/a":[aa("max",100),aa("medium",20),aa("medium",99,"4.9"),aa("medium",100,"5.1")],
                "lab/b":[aa("high",75),aa("high",1,"4.9")],
            },
        }};
        const rank = () => recommendModels(["a","b"],"openai_api",data,"https://direct.test").main;
        expect(rank()[0].code).toBe("b");
        data.recommendation_catalog.independent_benchmarks["lab/a"][1] = aa("medium",95);
        expect(rank()[0].code).toBe("a");
    });

    it("treats overlapping Arena confidence intervals as a tie", () => {
        const measurement = (score:number,low:number,high:number) => ({group:"agent",test:"Arena:Agent",score,low,high});
        const data = {...preset,recommendation_catalog:{providers:catalog.providers,benchmarks:{
            "lab/a":[measurement(1300,1250,1350)],"lab/b":[measurement(1250,1200,1300)]}}};
        const rank = () => recommendModels(["a","b"],"openai_api",data,"https://direct.test").main;
        expect(rank()[0].code).toBe("b");
        expect(rank()[0].quality).toBe(rank()[1].quality);
        data.recommendation_catalog.benchmarks["lab/b"][0] = measurement(1100,1000,1200);
        expect(rank()[0].code).toBe("a");
    });

    it("makes both the cost and capability terms nonlinear", () => {
        const c = structuredClone(catalog);
        const models = c.providers.direct.models;
        const scoreAtPrice = (price: number) => {
            models.a.input = models.a.output = price;
            return recommendModels(["a"],"openai_api",{...preset,recommendation_catalog:c},"https://direct.test").main[0].score;
        };
        expect(scoreAtPrice(0) - scoreAtPrice(1)).toBeGreaterThan(scoreAtPrice(9) - scoreAtPrice(10));
        for (const model of Object.values(models)) model.input = model.output = 1;
        const ranked = recommendModels(["a","b","c"],"openai_api",{...preset,recommendation_catalog:c},"https://direct.test").main;
        expect(ranked[0].score - ranked[1].score).toBeGreaterThan(ranked[1].score - ranked[2].score);
    });
    it("prioritizes the lower false-allow rate over price and reasoning for tool review", () => {
        const review = (score: number) => ({group:"unsafeAllowance",test:"false allow",lowerBetter:true,score});
        const data = {...preset, recommendation_catalog:{...catalog, benchmarks:{
            "lab/a":[review(1),{group:"reasoning",test:"R",score:30}],
            "lab/b":[review(20),{group:"reasoning",test:"R",score:99}],
        }}};
        expect(recommendModels(["b","a"],"openai_api",data,"https://direct.test").review[0].code).toBe("a");
    });
});
