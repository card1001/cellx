import { describe, expect, it } from "vitest";
import { formatBytes, formatElapsed } from "./DataJobProgress";

describe("data job progress text", () => {
  it("formats byte counts", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(250 * 1024 * 1024)).toBe("250 MB");
    expect(formatBytes(3.2 * 1024 ** 3)).toBe("3.2 GB");
  });

  it("formats elapsed time", () => {
    expect(formatElapsed(0)).toBe("0:00");
    expect(formatElapsed(65_400)).toBe("1:05");
    expect(formatElapsed(3_725_000)).toBe("1:02:05");
  });
});
