import { expect, it } from "vitest";
import { projectHtml, projectImage } from "./projectHtml";

it("should embed snapshot files and preserve external URLs for host approval when preparing HTML", () => {
    const files = Object.fromEntries(
        Object.entries({
            "src/index.html":
                '<link rel="stylesheet" href="./theme.css"><img src="../logo.svg"><img src="https://example.org/photo.png"><script src="./app.js"></script>',
            "src/theme.css": "body { background-image: url(../logo.svg) }",
            "src/app.js": "document.title='chosen';",
            "logo.svg": "<svg/>",
        }).map(([name, data]) => [name, btoa(data)])
    );
    const html = projectHtml(files, "src/index.html")!;
    expect(html).toContain("data:image/svg+xml;base64,");
    expect(html).toContain("background-image: url(data:image/svg+xml;base64,");
    expect(html).toContain("document.title='chosen'");
    expect(html).toContain("https://example.org/photo.png");
    expect(html).not.toContain('src="./app.js"');
});
it("should delegate module entry points to the project compiler when loading an application", () => {
    expect(
        projectHtml({ "index.html": btoa('<script type="module" src="main.tsx"></script>') }, "index.html")
    ).toBeUndefined();
});

it("should resolve Markdown image paths within the same snapshot", () => {
    const files = { "img/front cover.png": "AA==", "public/logo.svg": "AQ==" };
    expect(projectImage(files, "docs/index.md", "../img/front%20cover.png?v=2#cover")).toBe("data:image/png;base64,AA==");
    expect(projectImage(files, "docs/index.md", "/logo.svg")).toBe("data:image/svg+xml;base64,AQ==");
    expect(projectImage(files, "docs/index.md", "../../logo.svg")).toBeUndefined();
    expect(projectImage(files, "docs/index.md", "%zz")).toBeUndefined();
    expect(projectImage(files, "docs/index.md", "absent.jpg")).toBeUndefined();
});
