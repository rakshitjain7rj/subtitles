import { useEffect, useRef, useState } from "react";
import { snapToWords } from "../lib/captions";
import { clock } from "../lib/format";
import type { Caption, Word } from "../lib/types";

interface Props {
  duration: number;
  captions: Caption[];
  /** Spoken words, drawn as ticks so captions can be lined up with speech. */
  words: Word[] | null;
  selected: number;
  getTime: () => number;
  onSeek: (t: number) => void;
  onSelect: (index: number) => void;
  /** A drag is starting: one undo step covers the whole drag. */
  onDragStart: () => void;
  onRetime: (index: number, edge: "start" | "end", t: number) => void;
}

const ZOOMS = [20, 40, 80, 160, 320];
const DEFAULT_ZOOM = 2;
/** How close, in pixels, a dragged edge must come to a word to snap to it. */
const SNAP_PX = 8;

/** Seconds between labelled ticks at `pps` pixels per second. */
function labelEvery(pps: number): number {
  if (pps >= 160) return 1;
  if (pps >= 40) return 5;
  return 10;
}

/** Caption blocks on a time strip: click to jump, drag an edge to retime. */
export function Timeline({ duration, captions, words, selected, getTime, onSeek, onSelect, onDragStart, onRetime }: Props) {
  const [zoom, setZoom] = useState(DEFAULT_ZOOM);
  const pps = ZOOMS[zoom];
  const width = Math.max(1, Math.ceil(duration * pps));
  const scroll = useRef<HTMLDivElement>(null);
  const track = useRef<HTMLDivElement>(null);
  const playhead = useRef<HTMLDivElement>(null);
  const drag = useRef<{ index: number; edge: "start" | "end"; pointer: number } | null>(null);

  // The playhead follows the video every frame without re-rendering React,
  // and the strip scrolls along while it plays.
  useEffect(() => {
    let last = -1;
    let frame = requestAnimationFrame(function tick() {
      const t = getTime();
      const x = t * pps;
      if (playhead.current) playhead.current.style.transform = `translateX(${x}px)`;
      const el = scroll.current;
      if (el && t !== last && !drag.current) {
        const moving = Math.abs(t - last) < 0.5;
        if (moving && (x > el.scrollLeft + el.clientWidth - 40 || x < el.scrollLeft)) el.scrollLeft = x - 40;
      }
      last = t;
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [getTime, pps]);

  // Bring a caption picked in the list into view.
  useEffect(() => {
    const c = captions[selected];
    const el = scroll.current;
    if (!c || !el) return;
    const left = c.start * pps;
    const right = c.end * pps;
    if (left < el.scrollLeft || right > el.scrollLeft + el.clientWidth) el.scrollLeft = Math.max(0, left - 60);
    // Only when the selection or zoom changes, not on every edit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, pps]);

  function timeAt(clientX: number): number {
    const rect = track.current?.getBoundingClientRect();
    if (!rect) return 0;
    return Math.min(Math.max((clientX - rect.left) / pps, 0), duration);
  }

  function startDrag(e: React.PointerEvent, index: number, edge: "start" | "end") {
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { index, edge, pointer: e.pointerId };
    onDragStart();
    onSelect(index);
  }

  function moveDrag(e: React.PointerEvent) {
    const d = drag.current;
    if (!d || d.pointer !== e.pointerId) return;
    onRetime(d.index, d.edge, snapToWords(timeAt(e.clientX), words, SNAP_PX / pps));
  }

  function endDrag(e: React.PointerEvent) {
    if (drag.current?.pointer === e.pointerId) drag.current = null;
  }

  const every = labelEvery(pps);
  const labels: number[] = [];
  for (let t = 0; t <= duration; t += every) labels.push(t);

  return (
    <section className="timeline" aria-label="Timeline">
      <div className="timeline-tools">
        <span className="muted small">Drag a caption's edge to change when it shows. Edges snap to spoken words.</span>
        <span className="spacer" />
        <button
          className="ghost small"
          onClick={() => setZoom((z) => Math.max(z - 1, 0))}
          disabled={zoom === 0}
          aria-label="Zoom out"
        >
          −
        </button>
        <button
          className="ghost small"
          onClick={() => setZoom((z) => Math.min(z + 1, ZOOMS.length - 1))}
          disabled={zoom === ZOOMS.length - 1}
          aria-label="Zoom in"
        >
          +
        </button>
      </div>
      <div className="timeline-scroll" ref={scroll}>
        <div
          className="timeline-track"
          ref={track}
          style={{ width }}
          onPointerDown={(e) => {
            if (e.button === 0) onSeek(timeAt(e.clientX));
          }}
        >
          {labels.map((t) => (
            <span key={t} className="timeline-label" style={{ left: t * pps }}>
              {clock(t, 0)}
            </span>
          ))}
          <div className="timeline-words" aria-hidden>
            {words?.map((w, i) => (
              <span key={i} style={{ left: w.start * pps, width: Math.max(1, (w.end - w.start) * pps) }} />
            ))}
          </div>
          {captions.map((c, i) => (
            <div
              key={c.id}
              className={`timeline-caption${i === selected ? " selected" : ""}${c.english.trim() ? "" : " empty"}`}
              style={{ left: c.start * pps, width: Math.max(2, (c.end - c.start) * pps) }}
              title={c.english}
              onPointerDown={(e) => {
                e.stopPropagation();
                if (e.button === 0) onSelect(i);
              }}
            >
              <span
                className="timeline-handle start"
                onPointerDown={(e) => startDrag(e, i, "start")}
                onPointerMove={moveDrag}
                onPointerUp={endDrag}
                onPointerCancel={endDrag}
                aria-label="Drag to change the start"
              />
              <span className="timeline-text">{c.english || "(empty)"}</span>
              <span
                className="timeline-handle end"
                onPointerDown={(e) => startDrag(e, i, "end")}
                onPointerMove={moveDrag}
                onPointerUp={endDrag}
                onPointerCancel={endDrag}
                aria-label="Drag to change the end"
              />
            </div>
          ))}
          <div className="timeline-playhead" ref={playhead} />
        </div>
      </div>
    </section>
  );
}
