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
    expect(css).toMatch(/\.rtabs\s*\{[^}]*overflow-x:\s*auto/s);
    expect(css).toMatch(/\.rtab\s*\{[^}]*flex:\s*none/s);
  });

  it("allows long menu labels to wrap", () => {
    expect(css).toMatch(/\.menu-item\s*\{[^}]*white-space:\s*normal/s);
    expect(css).toMatch(/\.menu-label\s*\{[^}]*min-width:\s*0/s);
  });

  it("constrains dialogs to the viewport and permits vertical scrolling", () => {
    expect(css).toMatch(/\.modal\s*\{[^}]*max-height:\s*88vh/s);
    expect(css).toMatch(/\.modal-body\s*\{[^}]*overflow:\s*auto/s);
    expect(css).toMatch(/\.fmt-grid\s*\{[^}]*minmax\(0,\s*1fr\)/s);
  });

  it("allows narrow data bars and action rows to reflow", () => {
    expect(css).toMatch(/\.data-bar\s*\{[^}]*flex-wrap:\s*wrap/s);
    expect(css).toMatch(/\.actions\s*\{[^}]*flex-wrap:\s*wrap/s);
  });
});
