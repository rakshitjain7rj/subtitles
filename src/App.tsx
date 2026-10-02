import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage } from "./lib/api";
import type { ProjectView, Status } from "./lib/types";
import { Home } from "./components/Home";
import { Editor } from "./components/Editor";
import { Settings } from "./components/Settings";
import { Setup, needsSetup } from "./components/Setup";
import { UpdateBanner } from "./components/UpdateBanner";
import "./App.css";

export default function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [open, setOpen] = useState<{ view: ProjectView; fresh: boolean } | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  // Shown on launch while keys are missing, until finished or put off.
  const [setupDismissed, setSetupDismissed] = useState(false);
  // Keeps the screen up after the last key is saved, so it can say "done".
  const setupShownThisRun = useRef(false);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.status());
      setStatusError(null);
    } catch (e) {
      setStatusError(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  return (
    <div className="app">
      <UpdateBanner />
      {status && !setupDismissed && !open && (needsSetup(status) || setupShownThisRun.current) ? (
        <Setup
          status={status}
          onChanged={async () => {
            setupShownThisRun.current = true;
            await refreshStatus();
          }}
          onLater={() => setSetupDismissed(true)}
        />
      ) : open ? (
        <Editor
          key={open.view.project.id}
          initial={open.view}
          fresh={open.fresh}
          status={status}
          onBack={() => setOpen(null)}
          onOpenSettings={() => setShowSettings(true)}
        />
      ) : (
        <Home
          status={status}
          statusError={statusError}
          onOpen={(view, fresh = false) => setOpen({ view, fresh })}
          onOpenSettings={() => setShowSettings(true)}
        />
      )}
      {showSettings && (
        <Settings
          status={status}
          onChanged={refreshStatus}
          onClose={() => setShowSettings(false)}
        />
      )}
    </div>
  );
}
