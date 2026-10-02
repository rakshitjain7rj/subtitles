import { memo, useEffect, useRef } from "react";
import { NUDGE_STEP } from "../lib/captions";
import { clock } from "../lib/format";
import type { Caption } from "../lib/types";

export type CaptionAction =
  | { type: "text"; index: number; english: string }
  | { type: "nudge"; index: number; edge: "start" | "end"; delta: number }
  | { type: "split"; index: number; caret?: number }
  | { type: "merge"; index: number }
  | { type: "remove"; index: number };

interface Props {
  captions: Caption[];
  /** The caption currently on screen, or -1. */
  playing: number;
  selected: number;
  onSelect: (index: number) => void;
  onAction: (action: CaptionAction) => void;
}

export function CaptionList({ captions, playing, selected, onSelect, onAction }: Props) {
  const list = useRef<HTMLOListElement>(null);

  // Keep the caption being spoken in view while the video plays.
  useEffect(() => {
    if (playing < 0) return;
    const row = list.current?.children[playing] as HTMLElement | undefined;
    if (row && !row.contains(document.activeElement)) row.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [playing]);

  return (
    <ol className="caption-list" ref={list}>
      {captions.map((c, i) => (
        <Row
          key={c.id}
          caption={c}
          index={i}
          isLast={i === captions.length - 1}
          playing={i === playing}
          selected={i === selected}
          onSelect={onSelect}
          onAction={onAction}
        />
      ))}
    </ol>
  );
}

interface RowProps {
  caption: Caption;
  index: number;
  isLast: boolean;
  playing: boolean;
  selected: boolean;
  onSelect: (index: number) => void;
  onAction: (action: CaptionAction) => void;
}

const Row = memo(function Row({ caption, index, isLast, playing, selected, onSelect, onAction }: RowProps) {
  const input = useRef<HTMLInputElement>(null);
  const canSplit = caption.english.trim().split(/\s+/).length > 1;
  const nudge = (edge: "start" | "end", delta: number) => onAction({ type: "nudge", index, edge, delta });

  return (
    <li className={`caption${playing ? " playing" : ""}${selected ? " selected" : ""}`} onClick={() => onSelect(index)}>
      <div className="caption-time">
        <span>{clock(caption.start, 2)}</span>
        <span>{clock(caption.end, 2)}</span>
      </div>
      <div className="caption-text">
        <input
          ref={input}
          value={caption.english}
          placeholder="Nothing shown"
          spellCheck
          onFocus={() => onSelect(index)}
          onChange={(e) => onAction({ type: "text", index, english: e.target.value })}
          aria-label={`English caption ${index + 1}`}
        />
        {caption.hindi && (
          <div className="hindi" lang="hi">
            {caption.hindi}
          </div>
        )}
        {selected && (
          <div className="caption-tools" onClick={(e) => e.stopPropagation()}>
            <span className="tool-group" title={`Move the start by ${NUDGE_STEP * 1000} ms`}>
              <span className="tool-label">Start</span>
              <button onClick={() => nudge("start", -NUDGE_STEP)} aria-label="Start earlier">
                −
              </button>
              <button onClick={() => nudge("start", NUDGE_STEP)} aria-label="Start later">
                +
              </button>
            </span>
            <span className="tool-group" title={`Move the end by ${NUDGE_STEP * 1000} ms`}>
              <span className="tool-label">End</span>
              <button onClick={() => nudge("end", -NUDGE_STEP)} aria-label="End earlier">
                −
              </button>
              <button onClick={() => nudge("end", NUDGE_STEP)} aria-label="End later">
                +
              </button>
            </span>
            <button
              disabled={!canSplit}
              title="Split into two captions at the cursor"
              onClick={() => onAction({ type: "split", index, caret: input.current?.selectionStart ?? undefined })}
            >
              Split
            </button>
            <button disabled={isLast} title="Join with the next caption" onClick={() => onAction({ type: "merge", index })}>
              Merge ↓
            </button>
            <button className="danger" onClick={() => onAction({ type: "remove", index })}>
              Delete
            </button>
          </div>
        )}
      </div>
    </li>
  );
});
