import { describe, expect, it } from "vitest";
import { maxLineEm, wrapAll, wrapLines } from "./wrap";

// One unit per character: easy to reason about.
const byChars = (text: string) => text.length;

describe("wrapLines", () => {
  it("keeps text that fits on one line", () => {
    expect(wrapLines("  to save   money ", 20, byChars)).toEqual(["to save money"]);
    expect(wrapLines("   ", 20, byChars)).toEqual([]);
  });

  it("uses the fewest lines and balances them", () => {
    // Greedy would give "to save money that really" / "works"; balanced is more even.
    expect(wrapLines("to save money that really works", 25, byChars)).toEqual(["to save money", "that really works"]);
  });

  it("breaks into three lines when two can't hold the text", () => {
    const lines = wrapLines("one two three four five six seven eight nine", 16, byChars);
    expect(lines).toHaveLength(3);
    expect(lines.join(" ")).toBe("one two three four five six seven eight nine");
    expect(Math.max(...lines.map(byChars))).toBeLessThanOrEqual(16);
  });

  it("gives an over-long word its own line instead of failing", () => {
    expect(wrapLines("a supercalifragilistic b", 10, byChars)).toEqual(["a", "supercalifragilistic", "b"]);
  });
});

describe("wrapAll", () => {
  it("wraps against the width between the side margins", () => {
    const style = { y_pct: 72, size_pct: 10 };
    // 1080 wide with 6% margins leaves 950.4 px; at a 108 px em that is 8.8 em.
    expect(maxLineEm(1080, 1920, style)).toBeCloseTo(8.8);
    const captions = [
      { id: "a", start: 0, end: 1, english: "short", hindi: "" },
      { id: "b", start: 1, end: 2, english: "a b c d e f g", hindi: "" },
    ];
    expect(wrapAll(captions, 1080, 1920, style, byChars)).toEqual(["short", "a b c\nd e f g"]);
  });
});
