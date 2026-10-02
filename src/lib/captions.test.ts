import { describe, expect, it } from "vitest";
import { activeIndex, insertAt, mergeWithNext, nudge, remove, retime, snapToWords, split } from "./captions";
import type { Caption } from "./types";

const cap = (id: string, start: number, end: number, english: string, hindi = ""): Caption => ({ id, start, end, english, hindi });

const three = () => [cap("a", 0, 1, "Hello friends"), cap("b", 1, 2, "today we talk"), cap("c", 3, 4, "about money")];

describe("activeIndex", () => {
  it("finds the caption on screen and treats the end as exclusive", () => {
    const caps = three();
    expect(activeIndex(caps, 0.5)).toBe(0);
    expect(activeIndex(caps, 1)).toBe(1);
    expect(activeIndex(caps, 2.5)).toBe(-1);
  });
});

describe("nudge", () => {
  it("moves an edge without touching the input", () => {
    const caps = three();
    const out = nudge(caps, 2, "start", -0.05);
    expect(out[2].start).toBe(2.95);
    expect(caps[2].start).toBe(3);
  });

  it("pushes the neighbour's edge instead of overlapping", () => {
    const later = nudge(three(), 0, "end", 0.2);
    expect(later[0].end).toBe(1.2);
    expect(later[1].start).toBe(1.2);
    const earlier = nudge(three(), 1, "start", -0.3);
    expect(earlier[1].start).toBe(0.7);
    expect(earlier[0].end).toBe(0.7);
  });

  it("never makes a caption shorter than the minimum or starts before zero", () => {
    expect(nudge(three(), 0, "start", -5)[0].start).toBe(0);
    expect(nudge(three(), 0, "end", -5)[0].end).toBe(0.1);
    const squeezed = nudge(three(), 0, "end", 5);
    expect(squeezed[0].end).toBe(1.9);
    expect(squeezed[1]).toMatchObject({ start: 1.9, end: 2 });
  });
});

describe("split", () => {
  it("splits at the middle word boundary and divides the time by text length", () => {
    const out = split([cap("a", 0, 2, "one two three four", "एक दो तीन चार")], 0);
    expect(out).toHaveLength(2);
    expect(out[0]).toMatchObject({ id: "a", english: "one two", hindi: "एक दो", start: 0 });
    expect(out[1]).toMatchObject({ english: "three four", hindi: "तीन चार", end: 2 });
    expect(out[0].end).toBe(out[1].start);
    expect(out[1].id).not.toBe("a");
  });

  it("splits at the caret when one is given", () => {
    const out = split([cap("a", 0, 2, "one two three four")], 0, 3);
    expect(out[0].english).toBe("one");
    expect(out[1].english).toBe("two three four");
  });

  it("leaves a one-word caption alone", () => {
    const caps = [cap("a", 0, 2, "Hello")];
    expect(split(caps, 0)).toBe(caps);
  });
});

describe("merge, remove, insert", () => {
  it("merges with the next caption", () => {
    const out = mergeWithNext(three(), 0);
    expect(out).toHaveLength(2);
    expect(out[0]).toMatchObject({ id: "a", start: 0, end: 2, english: "Hello friends today we talk" });
    expect(mergeWithNext(three(), 2)).toHaveLength(3);
  });

  it("removes by index", () => {
    expect(remove(three(), 1).map((c) => c.id)).toEqual(["a", "c"]);
  });

  it("inserts only into a gap, stopping at the next caption", () => {
    const added = insertAt(three(), 2.4, 10);
    expect(added?.index).toBe(2);
    expect(added?.captions[2]).toMatchObject({ start: 2.4, end: 3, english: "" });
    expect(insertAt(three(), 0.5, 10)).toBeNull();
    expect(insertAt(three(), 9.95, 10)).toBeNull();
    expect(insertAt(three(), 5, 10)?.captions[3]).toMatchObject({ start: 5, end: 6 });
  });
});

describe("retime", () => {
  const caps = [cap("a", 0, 1, "a"), cap("b", 2, 3, "b"), cap("c", 4, 5, "c")];

  it("moves an edge freely within the gap", () => {
    const out = retime(caps, 1, "start", 1.5, 10);
    expect(out[1].start).toBe(1.5);
    expect(out[0]).toEqual(caps[0]);
  });

  it("pushes a touching neighbour's edge, keeping it at least the minimum length", () => {
    const touching = [cap("a", 0, 1, "a"), cap("b", 1, 2, "b"), cap("c", 2, 3, "c")];
    const longer = retime(touching, 1, "end", 2.5, 10);
    expect(longer[1].end).toBe(2.5);
    expect(longer[2].start).toBe(2.5);
    const capped = retime(touching, 1, "end", 9, 10);
    expect(capped[1].end).toBeCloseTo(2.9);
    expect(capped[2].start).toBeCloseTo(2.9);
    expect(retime(caps, 2, "end", 99, 6)[2].end).toBe(6);
  });

  it("keeps a minimum length", () => {
    expect(retime(caps, 1, "start", 3.5, 10)[1].start).toBeCloseTo(2.9);
    expect(retime(caps, 1, "end", 0, 10)[1].end).toBeCloseTo(2.1);
  });

  it("returns the same list when nothing changes", () => {
    expect(retime(caps, 1, "start", 2, 10)).toBe(caps);
  });
});

describe("snapToWords", () => {
  const words = [
    { text: "आज", start: 1.0, end: 1.4 },
    { text: "हम", start: 1.6, end: 2.0 },
  ];

  it("snaps to the nearest word edge within the tolerance", () => {
    expect(snapToWords(1.45, words, 0.1)).toBe(1.4);
    expect(snapToWords(1.58, words, 0.1)).toBe(1.6);
  });

  it("leaves times away from any word alone", () => {
    expect(snapToWords(3, words, 0.1)).toBe(3);
    expect(snapToWords(1.2, null, 0.5)).toBe(1.2);
  });
});
