import { useState, useEffect } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

interface Clip {
  start: string;
  end: string;
  title: string;
  reason: string;
  score: number;
  preview_path?: string | null;
}

interface ProgressEvent {
  payload: {
    stage: string;
    message: string;
    clips: Clip[];
  };
}

interface ClipSettings {
  clip_count: number;
  min_duration: number;
  max_duration: number;
  model: string;
  temperature: number;
}

type RunState = "idle" | "running" | "preview" | "done" | "error";

function App() {
  const [url, setUrl] = useState("");
  const [outputDir, setOutputDir] = useState("");
  const [stage, setStage] = useState("");
  const [message, setMessage] = useState("");
  const [clips, setClips] = useState<Clip[]>([]);
  const [previewErrors, setPreviewErrors] = useState<Record<number, string>>({});
  const [state, setState] = useState<RunState>("idle");
  const [error, setError] = useState("");
  const [customContext, setCustomContext] = useState("");
  const [showSettings, setShowSettings] = useState(false);
  const [settings, setSettings] = useState<ClipSettings>({
    clip_count: 5,
    min_duration: 30,
    max_duration: 60,
    model: "deepseek-chat",
    temperature: 0.3,
  });

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await listen<ProgressEvent["payload"]>("pipeline-progress", (e) => {
        setStage(e.payload.stage);
        setMessage(e.payload.message);
        if (e.payload.clips.length > 0) setClips(e.payload.clips);
      });
    })();
    return () => {
      unlisten?.();
    };
  }, []);

  async function pickFolder() {
    const dir = await open({ directory: true });
    if (dir) setOutputDir(dir as string);
  }

  async function run(forceDownload = true) {
    if (!url.trim() || !outputDir.trim()) return;
    setState("running");
    setError("");
    setPreviewErrors({});
    setClips([]);
    setStage("");
    setMessage("");
    try {
      await invoke("run_pipeline", {
        url: url.trim(),
        outputDir,
        forceDownload,
        settings,
        customContext,
        cutFinal: false,
      });
      setState("preview");
    } catch (e) {
      setError(String(e));
      setState("error");
    }
  }

  async function openFolder() {
    if (outputDir) await invoke("open_output_folder", { path: outputDir });
  }

  async function cutClips() {
    if (!outputDir || clips.length === 0) return;
    setState("running");
    setError("");
    try {
      await invoke("cut_cached_clips", { outputDir, clips });
      setState("done");
    } catch (e) {
      setError(String(e));
      setState("error");
    }
  }

  return (
    <main className="container">
      <h1>Clipster</h1>
      <p className="subtitle">Drop a YouTube link. Get viral clips.</p>

      <div className="input-group">
        <input
          className="url-input"
          type="text"
          placeholder="https://youtube.com/watch?v=…"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          disabled={state === "running"}
        />
      </div>

      <div className="input-group">
        <button className="folder-btn" onClick={pickFolder} disabled={state === "running"}>
          {outputDir ? outputDir : "Choose output folder…"}
        </button>
      </div>

      <textarea
        className="context-input"
        placeholder="Optional AI direction for this video: e.g. 'Prioritize founder lessons, controversial takes, and clips useful for startup Twitter. Avoid inside jokes.'"
        value={customContext}
        onChange={(e) => setCustomContext(e.target.value)}
        disabled={state === "running"}
      />

      <button
        className="settings-toggle"
        onClick={() => setShowSettings((v) => !v)}
        disabled={state === "running"}
      >
        {showSettings ? "Hide settings" : "Show settings"}
      </button>

      {showSettings && (
        <section className="settings-panel">
          <label>
            Clips
            <input
              type="number"
              min="1"
              max="20"
              value={settings.clip_count}
              onChange={(e) => setSettings({ ...settings, clip_count: Number(e.target.value) })}
            />
          </label>
          <label>
            Min seconds
            <input
              type="number"
              min="5"
              max="300"
              value={settings.min_duration}
              onChange={(e) => setSettings({ ...settings, min_duration: Number(e.target.value) })}
            />
          </label>
          <label>
            Max seconds
            <input
              type="number"
              min="5"
              max="300"
              value={settings.max_duration}
              onChange={(e) => setSettings({ ...settings, max_duration: Number(e.target.value) })}
            />
          </label>
          <label>
            Model
            <input
              type="text"
              value={settings.model}
              onChange={(e) => setSettings({ ...settings, model: e.target.value })}
            />
          </label>
          <label>
            Temperature
            <input
              type="number"
              min="0"
              max="2"
              step="0.1"
              value={settings.temperature}
              onChange={(e) => setSettings({ ...settings, temperature: Number(e.target.value) })}
            />
          </label>
        </section>
      )}

      <div className="actions">
        <button
          className="run-btn"
          onClick={() => run(true)}
          disabled={state === "running" || !url.trim() || !outputDir.trim()}
        >
          {state === "running" ? "Working…" : "Find Viral Clips"}
        </button>
        <button
          className="reanalyze-btn"
          onClick={() => run(false)}
          disabled={state === "running" || !url.trim() || !outputDir.trim()}
          title="Skip download if this YouTube URL is already cached"
        >
          Re-analyze cached video
        </button>
      </div>

      {state === "running" && (
        <div className="progress">
          <div className="stage">{stage}</div>
          <div className="message">{message}</div>
        </div>
      )}

      {error && <div className="error">{error}</div>}

      {clips.length > 0 && (
        <div className="clips">
          <h2>{clips.length} clips detected</h2>
          <div className="preview-grid">
            {clips.map((c, i) => c.preview_path ? (
              <article className="preview-card" key={`${c.start}-${i}`}>
                <video
                  src={convertFileSrc(c.preview_path)}
                  controls
                  preload="metadata"
                  playsInline
                  onError={(e) => {
                    const code = e.currentTarget.error?.code ?? 0;
                    setPreviewErrors((prev) => ({
                      ...prev,
                      [i]: `Preview failed to load (media error ${code}).`,
                    }));
                    console.error("preview failed", c.preview_path, e.currentTarget.error);
                  }}
                />
                <div className="preview-title">{i + 1}. {c.title}</div>
                {previewErrors[i] && <div className="preview-error">{previewErrors[i]}</div>}
                <div className="preview-meta">{c.start} → {c.end} · score {c.score}</div>
              </article>
            ) : null)}
          </div>
          <table>
            <thead>
              <tr>
                <th>#</th>
                <th>Time</th>
                <th>Score</th>
                <th>Hook</th>
                <th>Why</th>
              </tr>
            </thead>
            <tbody>
              {clips.map((c, i) => (
                <tr key={i} className={c.score >= 75 ? "high" : ""}>
                  <td>{i + 1}</td>
                  <td className="ts">
                    {c.start} → {c.end}
                  </td>
                  <td className="score">{c.score}</td>
                  <td className="title">{c.title}</td>
                  <td className="reason">{c.reason}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {state === "done" && (
        <button className="open-folder-btn" onClick={openFolder}>
          Open output folder
        </button>
      )}

      {state === "preview" && clips.length > 0 && (
        <button className="run-btn" onClick={cutClips}>
          Cut reviewed clips
        </button>
      )}
    </main>
  );
}

export default App;
