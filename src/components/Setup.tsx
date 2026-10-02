import { openUrl } from "@tauri-apps/plugin-opener";
import { hasTranslatorKey, type Provider, type Status } from "../lib/types";
import { KeyInput } from "./Settings";

interface Props {
  status: Status;
  onChanged: () => Promise<void>;
  onLater: () => void;
}

interface Step {
  provider: Provider;
  title: string;
  why: string;
  cost: string;
  url: string;
  site: string;
  how: string[];
  placeholder: string;
}

const STEPS: Step[] = [
  {
    provider: "elevenlabs",
    title: "ElevenLabs: turns the speech into text",
    why: "Your video's audio is uploaded to ElevenLabs to be transcribed.",
    cost: "The free plan includes some transcription every month. Past that it is about ₹19 per hour of audio, billed to your own account.",
    url: "https://elevenlabs.io/app/developers/api-keys",
    site: "Open ElevenLabs",
    how: [
      "Sign up or log in (Google sign-in works).",
      "Click Create API key and name it Subtitles.",
      "If it asks about permissions, set Speech to Text to Access. Everything else can stay off.",
      "Copy the key (it is shown only once) and paste it below.",
    ],
    placeholder: "sk_…",
  },
  {
    provider: "gemini",
    title: "Gemini: translates it into English",
    why: "Only the transcript text is sent. On the free tier Google may use it to improve its products.",
    cost: "Free, within Google's free-tier limits.",
    url: "https://aistudio.google.com/apikey",
    site: "Open Google AI Studio",
    how: [
      "Sign in with your Google account.",
      "Click Create API key. If it asks for a project, pick any.",
      "Copy the key and paste it below.",
    ],
    placeholder: "AIza…",
  },
];

/** Whether the keys needed to caption a video are missing. */
export function needsSetup(status: Status): boolean {
  return !status.has_elevenlabs_key || !hasTranslatorKey(status);
}

function saved(status: Status, provider: Provider): boolean {
  if (provider === "elevenlabs") return status.has_elevenlabs_key;
  if (provider === "gemini") return status.has_gemini_key;
  return status.has_anthropic_key;
}

/** First-run screen: walks through making the two free API keys. */
export function Setup({ status, onChanged, onLater }: Props) {
  const done = !needsSetup(status);
  return (
    <div className="home setup">
      <header>
        <h1>Set up Subtitles</h1>
        <p className="muted">
          The app uses two online services with your own free accounts. It takes about 10 minutes, once. Keys are kept
          in this computer's keychain and sent only to the service they belong to.
        </p>
      </header>
      {status.keychain_error && <div className="banner error">{status.keychain_error}</div>}

      {STEPS.map((step, i) => {
        const ok = saved(status, step.provider);
        return (
          <section key={step.provider} className="setup-step">
            <div className="key-head">
              <h3>
                {i + 1}. {step.title}
              </h3>
              <span className={ok ? "tag ok" : "tag"}>{ok ? "Done" : "To do"}</span>
            </div>
            <p className="muted small">
              {step.why} {step.cost}
            </p>
            {!ok && (
              <>
                <button className="small" onClick={() => void openUrl(step.url)}>
                  {step.site} ↗
                </button>
                <ol className="small">
                  {step.how.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ol>
              </>
            )}
            <KeyInput provider={step.provider} placeholder={step.placeholder} saved={ok} onChanged={onChanged} />
          </section>
        );
      })}

      <footer className="setup-footer">
        {done ? (
          <button className="primary" onClick={onLater}>
            Start captioning
          </button>
        ) : (
          <button className="ghost" onClick={onLater}>
            Set up later
          </button>
        )}
      </footer>
    </div>
  );
}
