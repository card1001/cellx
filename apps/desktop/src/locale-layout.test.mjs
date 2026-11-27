import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");

describe("localized desktop layout CSS", () => {
  it("provides CJK font fallbacks", () => {
    expect(css).toContain("Noto Sans CJK JP");
    expect(css).toContain("Noto Sans CJK SC");
    expect(css).toContain("Noto Sans CJK TC");
  });

  it("keeps ribbon tabs scrollable rather than clipping translated labels", () => {
    expect(css).toContain(".rtabs { min-width: 0; overflow-x: auto;");
    expect(css).toContain(".rtab { flex: none;");
  });

  it("allows long menu labels to wrap", () => {
    expect(css).toContain(".menu-item { white-space: normal;");
    expect(css).toContain(".menu-label { min-width: 0;");
  });

  it("constrains dialogs to the viewport and permits vertical scrolling", () => {
    expect(css).toContain("max-height: 88vh;");
    expect(css).toContain(".modal-body { min-width: 0; max-height: calc(88vh - 52px); overflow: auto;");
    expect(css).toContain(".fmt-grid { grid-template-columns: minmax(112px, 180px) minmax(0, 1fr);");
  });

  it("allows narrow data bars and action rows to reflow", () => {
    expect(css).toContain(".data-bar { min-height: 30px; height: auto; flex-wrap: wrap;");
    expect(css).toContain(".actions { flex-wrap: wrap;");
  });
});
