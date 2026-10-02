import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { errorMessage } from "../lib/api";

/** Checks GitHub Releases once on launch and offers a newer version. */
export function UpdateBanner() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Offline, or no release published yet: nothing to offer.
    check()
      .then(setUpdate)
      .catch(() => {});
  }, []);

  if (!update) return null;

  async function install(u: Update) {
    setError(null);
    setProgress(0);
    let total = 0;
    let done = 0;
    try {
      await u.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        else if (event.event === "Progress") {
          done += event.data.chunkLength;
          if (total > 0) setProgress(done / total);
        }
      });
      await relaunch();
    } catch (e) {
      setProgress(null);
      setError(errorMessage(e));
    }
  }

  return (
    <div className={`banner update${error ? " error" : ""}`}>
      <span>
        {error
          ? `The update didn't install: ${error}`
          : progress !== null
            ? `Downloading version ${update.version}… ${Math.round(progress * 100)}%`
            : `Version ${update.version} is available.`}
      </span>
      {progress === null && (
        <span className="update-actions">
          <button className="primary small" onClick={() => void install(update)}>
            {error ? "Try again" : "Update and restart"}
          </button>
          <button className="ghost small" onClick={() => setUpdate(null)}>
            Later
          </button>
        </span>
      )}
    </div>
  );
}
