import { describe, expect, it } from "vitest";
import { activeIndex, insertAt, mergeWithNext, nudge, remove, split } from "./captions";
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
