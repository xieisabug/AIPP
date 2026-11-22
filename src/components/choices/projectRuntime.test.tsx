import { expect, it } from "vitest";
import { mountProject } from "./projectRuntime";

it("should render a template-only Vue child imported by another snapshot file", () => {
    document.body.innerHTML = '<div id="root"></div>';
    mountProject({
        "App.vue": btoa('<script setup>import Child from "./Child.vue"</script><template><Child/></template>'),
        "Child.vue": btoa("<template><h1>Saved child</h1></template>"),
    }, "App.vue");
    expect(document.querySelector("h1")?.textContent).toBe("Saved child");
});
