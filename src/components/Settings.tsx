import { useState } from "react";
import { api, errorMessage } from "../lib/api";
import type { Engine, Provider, Quality, Status } from "../lib/types";

interface Props {
  status: Status | null;
  onChanged: () => Promise<void>;
  onClose: () => void;
}

interface ProviderInfo {
  id: Provider;
  name: string;
  use: string;
  placeholder: string;
  where: string;
}

const ELEVENLABS: ProviderInfo = {
  id: "elevenlabs",
  name: "ElevenLabs",
  use: "Transcribes the speech. Your video's audio is uploaded to ElevenLabs.",
  placeholder: "sk_…",
  where: "elevenlabs.io → Developers → API Keys",
};

const TRANSLATORS: { engine: Engine; title: string; detail: string; provider: ProviderInfo }[] = [
  {
    engine: "gemini",
    title: "Gemini 3.8 Flash (free)",
    detail:
      "Free on Google's free tier, within its per-minute and daily request limits. On the free tier Google may use the transcript text to improve its products, and people at Google may read it.",
    provider: {
      id: "gemini",
      name: "Gemini",
      use: "Translates the transcript. Only the text is sent.",
      placeholder: "AIza…",
      where: "aistudio.google.com → Get API key",
    },
  },
  {
    engine: "claude",
    title: "Claude Sonnet 5.5 (paid)",
    detail: "Paid per use, roughly $0.04 per minute of video, from prepaid Anthropic credit. Not used for training.",
    provider: {
      id: "anthropic",
      name: "Anthropic",
      use: "Translates the transcript with Claude. Only the text is sent.",
      placeholder: "sk-ant-…",
      where: "console.anthropic.com → Settings → API Keys",
    },
  },
];

const QUALITIES: { quality: Quality; title: string; detail: string }[] = [
  {
    quality: "high",
    title: "High (recommended)",
    detail: "Visually identical to the original, usually close to its file size. Plays everywhere.",
  },
  {
    quality: "lossless",
    title: "Lossless",
    detail:
      "Every pixel outside the captions exactly as in the original. Files are several times larger, and some phones and browsers can't play them. For editing or archiving, not uploading.",
  },
];

function hasKey(status: Status | null, provider: Provider): boolean {
  if (!status) return false;
  if (provider === "elevenlabs") return status.has_elevenlabs_key;
  if (provider === "gemini") return status.has_gemini_key;
  return status.has_anthropic_key;
}

export function Settings({ status, onChanged, onClose }: Props) {
  const current = status?.settings.translator ?? "gemini";
  const exportQuality = status?.settings.export_quality ?? "high";
  const [error, setError] = useState<string | null>(null);

  async function chooseQuality(quality: Quality) {
    setError(null);
    try {
      await api.setExportQuality(quality);
      await onChanged();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function choose(engine: Engine) {
    setError(null);
    try {
      await api.setTranslator(engine);
      await onChanged();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-label="Settings">
        <header>
          <h2>Settings</h2>
          <button className="ghost" onClick={onClose}>
            Close
          </button>
        </header>
        <p className="muted">
          Captions are generated with your own API keys, billed to your own accounts. Keys are stored in this computer's
          keychain and sent only to the service they belong to.
        </p>
        {status?.keychain_error && <div className="banner error">{status.keychain_error}</div>}
        {error && <div className="banner error">{error}</div>}

        <KeyRow provider={ELEVENLABS} saved={hasKey(status, "elevenlabs")} onChanged={onChanged} />

        <section className="key-row">
          <h3>Translation</h3>
          <p className="muted small">
            Pick which service turns the transcript into English captions. To compare them, translate a video with one,
            switch here, and use Translate again.
          </p>
          <div className="choices" role="radiogroup" aria-label="Translator">
            {TRANSLATORS.map((t) => (
              <label key={t.engine} className={`choice${current === t.engine ? " chosen" : ""}`}>
                <input
                  type="radio"
                  name="translator"
                  checked={current === t.engine}
                  onChange={() => void choose(t.engine)}
                  disabled={!status}
                />
                <span>
                  <strong>{t.title}</strong>
                  <span className="muted small">{t.detail}</span>
                </span>
              </label>
            ))}
          </div>
        </section>

        {TRANSLATORS.filter((t) => t.engine === current).map((t) => (
          <KeyRow key={t.provider.id} provider={t.provider} saved={hasKey(status, t.provider.id)} onChanged={onChanged} />
        ))}

        <section className="key-row">
          <h3>Export quality</h3>
          <div className="choices" role="radiogroup" aria-label="Export quality">
            {QUALITIES.map((q) => (
              <label key={q.quality} className={`choice${exportQuality === q.quality ? " chosen" : ""}`}>
                <input
                  type="radio"
                  name="export-quality"
                  checked={exportQuality === q.quality}
                  onChange={() => void chooseQuality(q.quality)}
                  disabled={!status}
                />
                <span>
                  <strong>{q.title}</strong>
                  <span className="muted small">{q.detail}</span>
                </span>
              </label>
            ))}
          </div>
        </section>

        <p className="muted small">
          {status?.tools.ok
            ? `ffmpeg ${status.tools.version ?? ""} is ready.`
            : status
              ? `ffmpeg problem: ${status.tools.missing.join(", ")}.`
              : "Checking ffmpeg…"}
        </p>
      </div>
    </div>
  );
}

function KeyRow({ provider, saved, onChanged }: { provider: ProviderInfo; saved: boolean; onChanged: () => Promise<void> }) {
  const [value, setValue] = useState("");
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run(action: () => Promise<void>) {
    setWorking(true);
    setError(null);
    try {
      await action();
      setValue("");
      await onChanged();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setWorking(false);
    }
  }

  return (
    <section className="key-row">
      <div className="key-head">
        <h3>{provider.name} API key</h3>
        <span className={saved ? "tag ok" : "tag"}>{saved ? "Saved" : "Not set"}</span>
      </div>
      <p className="muted small">
        {provider.use} Get a key at {provider.where}.
      </p>
      <div className="key-input">
        <input
          type="password"
          value={value}
          placeholder={saved ? "Paste a new key to replace it" : provider.placeholder}
          onChange={(e) => setValue(e.target.value)}
          autoComplete="off"
          spellCheck={false}
        />
        <button disabled={working || value.trim() === ""} onClick={() => run(() => api.setApiKey(provider.id, value))}>
          Save
        </button>
        {saved && (
          <button className="ghost" disabled={working} onClick={() => run(() => api.deleteApiKey(provider.id))}>
            Remove
          </button>
        )}
      </div>
      {error && <p className="error-text small">{error}</p>}
    </section>
  );
}
