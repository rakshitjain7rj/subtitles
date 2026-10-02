import { useCallback, useEffect, useState } from "react";
import { api, errorMessage } from "./lib/api";
import type { ProjectView, Status } from "./lib/types";
import { Home } from "./components/Home";
import { Editor } from "./components/Editor";
import { Settings } from "./components/Settings";
import "./App.css";

export default function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [open, setOpen] = useState<ProjectView | null>(null);
  const [showSettings, setShowSettings] = useState(false);

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
      {open ? (
        <Editor
          key={open.project.id}
          initial={open}
          status={status}
          onBack={() => setOpen(null)}
          onOpenSettings={() => setShowSettings(true)}
        />
      ) : (
        <Home status={status} statusError={statusError} onOpen={setOpen} onOpenSettings={() => setShowSettings(true)} />
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
