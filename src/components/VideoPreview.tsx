import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { activeIndex } from "../lib/captions";
import { clock } from "../lib/format";
import { overlayCss } from "../lib/style";
import { captionFontReady, maxLineEm, measureEm, wrapLines } from "../lib/wrap";
import type { Caption, MediaInfo, Style } from "../lib/types";

export interface PreviewHandle {
  seek: (t: number) => void;
  time: () => number;
  toggle: () => void;
}

interface Props {
  url: string;
  info: MediaInfo;
  captions: Caption[];
  style: Style;
  /** Called when a different caption (or none, -1) comes on screen. */
  onActiveChange: (index: number) => void;
}

/** The video with the captions drawn over it the way the export will burn them in. */
export const VideoPreview = forwardRef<PreviewHandle, Props>(function VideoPreview(
  { url, info, captions, style, onActiveChange },
  ref,
) {
  const stage = useRef<HTMLDivElement>(null);
  const video = useRef<HTMLVideoElement>(null);
  const [box, setBox] = useState({ width: 0, height: 0 });
  const [time, setTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [failed, setFailed] = useState(false);
  const duration = info.duration;

  // Fit the video's aspect ratio inside whatever space the stage has.
  useLayoutEffect(() => {
    const el = stage.current;
    if (!el) return;
    const fit = () => {
      const scale = Math.min(el.clientWidth / info.width, el.clientHeight / info.height);
      setBox({ width: Math.floor(info.width * scale), height: Math.floor(info.height * scale) });
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, [info.width, info.height]);

  // `timeupdate` fires only a few times a second; captions need every frame.
  useEffect(() => {
    if (!playing) return;
    let frame = requestAnimationFrame(function tick() {
      if (video.current) setTime(video.current.currentTime);
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [playing]);

  const active = activeIndex(captions, time);
  const lastReported = useRef(-2);
  useEffect(() => {
    if (active !== lastReported.current) {
      lastReported.current = active;
      onActiveChange(active);
    }
  }, [active, onActiveChange]);

  function seek(t: number) {
    const v = video.current;
    if (!v) return;
    const clamped = Math.min(Math.max(t, 0), duration);
    v.currentTime = clamped;
    setTime(clamped);
  }

  function toggle() {
    const v = video.current;
    if (!v) return;
    if (v.paused) void v.play().catch(() => setFailed(true));
    else v.pause();
  }

  useImperativeHandle(ref, () => ({ seek, toggle, time: () => video.current?.currentTime ?? 0 }));

  // Line breaks can only be measured once the caption font has loaded.
  const [fontReady, setFontReady] = useState(false);
  useEffect(() => {
    void captionFontReady().finally(() => setFontReady(true));
  }, []);

  const text =
    active === -1 || !fontReady
      ? ""
      : wrapLines(captions[active].english, maxLineEm(info.width, info.height, style), measureEm).join("\n");

  return (
    <div className="preview">
      <div className="preview-stage" ref={stage}>
        <div className="preview-frame" style={box} onClick={toggle}>
          <video
            ref={video}
            src={url}
            playsInline
            preload="auto"
            onPlay={() => setPlaying(true)}
            onPause={() => setPlaying(false)}
            onSeeked={(e) => setTime(e.currentTarget.currentTime)}
            onError={() => setFailed(true)}
          />
          {text && box.height > 0 && (
            <div
              className="caption-layer"
              style={{ width: info.width, height: info.height, transform: `scale(${box.height / info.height})` }}
            >
              <div className="caption-overlay" style={overlayCss(info.width, info.height, style)}>
                {text}
              </div>
            </div>
          )}
          {failed && <div className="preview-error">The preview could not be played. Captions and export still work.</div>}
        </div>
      </div>
      <div className="transport">
        <button className="play" onClick={toggle} aria-label={playing ? "Pause" : "Play"}>
          {playing ? "❚❚" : "▶"}
        </button>
        <input
          type="range"
          min={0}
          max={duration}
          step={0.01}
          value={Math.min(time, duration)}
          onChange={(e) => seek(Number(e.target.value))}
          aria-label="Position"
        />
        <span className="time">
          {clock(time)} / {clock(duration)}
        </span>
      </div>
    </div>
  );
});
