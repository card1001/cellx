import { describe, expect, it } from "vitest";
import { formatLocaleDate, formatLocaleNumber, formatLocaleTime, normalizeLocale, translate, translateText } from "./i18n";

describe("locale selection", () => {
  it("maps common browser locales to supported locales", () => {
    expect(normalizeLocale("ko")).toBe("ko-KR");
    expect(normalizeLocale("ja-JP")).toBe("ja-JP");
    expect(normalizeLocale("zh-Hant-TW")).toBe("zh-TW");
    expect(normalizeLocale("zh-SG")).toBe("zh-CN");
    expect(normalizeLocale("fr-FR")).toBe("en-US");
    expect(normalizeLocale(null)).toBe("en-US");
  });

  it("provides translated common application strings", () => {
    expect(translate("ko-KR", "language")).toBe("언어");
    expect(translate("en-US", "cancel")).toBe("Cancel");
    expect(translate("ja-JP", "settings")).toBe("設定");
    expect(translate("zh-CN", "enter")).toBe("输入");
    expect(translate("zh-TW", "insertFunction")).toBe("插入函數");
  });

  it("translates common spreadsheet UI labels in each locale", () => {
    expect(translateText("ko-KR", "파일")).toBe("파일");
    expect(translateText("en-US", "파일")).toBe("File");
    expect(translateText("ja-JP", "파일")).toBe("ファイル");
    expect(translateText("zh-CN", "파일")).toBe("文件");
    expect(translateText("zh-TW", "파일")).toBe("檔案");
  });

  it("translates dynamic messages by their stable prefix", () => {
    expect(translateText("en-US", "저장했습니다: report.xlsx")).toBe("Saved: report.xlsx");
    expect(translateText("ja-JP", "저장했습니다: report.xlsx")).toBe("保存しました: report.xlsx");
    expect(translateText("zh-CN", "내보냈습니다: data.csv")).toBe("已导出： data.csv");
  });

  it("falls back to the source text for untranslated labels", () => {
    expect(translateText("en-US", "A user-defined label")).toBe("A user-defined label");
    expect(translateText("ja-JP", "A user-defined label")).toBe("A user-defined label");
  });
});

describe("locale formatting", () => {
  it("formats numbers using the selected locale", () => {
    expect(formatLocaleNumber(1234.5, "en-US")).toBe("1,234.5");
    expect(formatLocaleNumber(1234.5, "zh-CN")).toBe("1,234.5");
  });

  it("formats date and time using the selected locale", () => {
    const date = new Date("2026-11-27T13:05:00Z");
    expect(formatLocaleDate(date, "en-US", { timeZone: "UTC", year: "numeric", month: "2-digit", day: "2-digit" })).toBe("11/27/2026");
    expect(formatLocaleTime(date, "en-US", { timeZone: "UTC", hour: "2-digit", minute: "2-digit", hour12: false })).toBe("13:05");
  });
});
