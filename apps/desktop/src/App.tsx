import { useEffect, useRef, useState } from "react";
import { Inspector } from "./Inspector";
import { loadPreviewAudio, loadProject, queryGpuInfo, queryPreviewStats, resetPreview, resizeViewport, saveProject, seekPreview, setPreviewPlaying, setPreviewQuality, updateProject, type GpuSummary, type PreviewStats } from "./native";
import { useEditorStore, type ProjectData } from "./store";
const presets = ["Star System", "Nebula", "Liquid Chrome", "Green Slime", "Water Droplets"];
export function App() {
  const { document, dirty, setDocument, replaceProject } = useEditorStore(); const [selectedPreset, setSelectedPreset] = useState<string | null>(null);
  const [projectPath, setProjectPath] = useState("examples/star-orbit.rustique.json"); const [audioPath, setAudioPath] = useState(""); const [gpu, setGpu] = useState<GpuSummary | null>(null); const [stats, setStats] = useState<PreviewStats | null>(null); const [error, setError] = useState<string | null>(null); const viewportRef = useRef<HTMLDivElement>(null);
  const previewUpdateTimer = useRef<number | null>(null);
  const previewUpdateGeneration = useRef(0);
  useEffect(() => { const viewport = viewportRef.current; if (!viewport) return; const updateBounds = () => { const bounds = viewport.getBoundingClientRect(); const scale = window.devicePixelRatio; void resizeViewport(Math.round(bounds.x * scale), Math.round(bounds.y * scale), Math.round(bounds.width * scale), Math.round(bounds.height * scale)); }; const observer = new ResizeObserver(updateBounds); observer.observe(viewport); window.addEventListener("resize", updateBounds); updateBounds(); return () => { observer.disconnect(); window.removeEventListener("resize", updateBounds); }; }, []);
  useEffect(() => { const timer = window.setInterval(() => void queryPreviewStats().then(setStats).catch(() => undefined), 250); return () => window.clearInterval(timer); }, []);
  useEffect(() => () => { if (previewUpdateTimer.current !== null) window.clearTimeout(previewUpdateTimer.current); }, []);
  async function run(action: () => Promise<void>) { try { setError(null); await action(); } catch (reason) { setError(String(reason)); } }
  function edit(project: ProjectData) {
    replaceProject(project);
    const generation = ++previewUpdateGeneration.current;
    if (previewUpdateTimer.current !== null) window.clearTimeout(previewUpdateTimer.current);
    previewUpdateTimer.current = window.setTimeout(() => {
      previewUpdateTimer.current = null;
      void run(async () => {
        const validated = await updateProject(project);
        if (previewUpdateGeneration.current === generation) replaceProject(validated);
      });
    }, 200);
  }
  async function openProject() {
    previewUpdateGeneration.current += 1;
    if (previewUpdateTimer.current !== null) window.clearTimeout(previewUpdateTimer.current);
    previewUpdateTimer.current = null;
    setDocument(await loadProject(projectPath));
  }
  async function saveCurrentProject() {
    if (!document) return;
    previewUpdateGeneration.current += 1;
    if (previewUpdateTimer.current !== null) window.clearTimeout(previewUpdateTimer.current);
    previewUpdateTimer.current = null;
    const validated = await updateProject(document.project);
    await saveProject(document.path, validated);
    replaceProject(validated, false);
  }
  const project = document?.project; const lastFrame = project ? Math.max(0, Math.ceil(project.duration_seconds * project.fps) - 1) : 0;
  return <main className="studio-shell"><header className="topbar"><div><span className="brand">RUSTIQUE</span><span className="subtitle">particle studio {dirty ? "• unsaved" : ""}</span></div><button onClick={() => void run(async () => setGpu(await queryGpuInfo()))}>Inspect GPU</button></header>
    <aside className="panel presets"><h2>Visual presets</h2>{presets.map((preset) => <button className={selectedPreset === preset ? "selected" : ""} key={preset} onClick={() => setSelectedPreset(preset)}>{preset}</button>)}</aside>
    <section className="viewport"><div className="viewport-grid" ref={viewportRef} /><div className="transport"><button onClick={() => void resetPreview()}>Reset</button><button className={stats?.playing ? "playing" : ""} onClick={() => void setPreviewPlaying(!(stats?.playing ?? false))}>{stats?.playing ? "Pause" : "Play"}</button><span>frame {stats?.frameIndex ?? 0} / {lastFrame}</span><span>{project ? ((stats?.frameIndex ?? 0) / project.fps).toFixed(2) : "0.00"}s</span><span>{stats?.framesPerSecond.toFixed(1) ?? "0.0"} fps</span><span>{stats?.particleCount.toLocaleString() ?? 0} particles</span><span>GPU {stats?.gpuRenderMs?.toFixed(2) ?? "—"} ms</span></div></section>
    <aside className="panel inspector"><h2>Inspector</h2><label>Project path</label><input value={projectPath} onChange={(e) => setProjectPath(e.target.value)} /><div className="button-row"><button onClick={() => void run(openProject)}>Load</button><button disabled={!project} onClick={() => void run(saveCurrentProject)}>Save</button></div><label>Preview audio</label><input value={audioPath} placeholder="C:\\music\\track.wav" onChange={(e) => setAudioPath(e.target.value)} /><button onClick={() => void run(() => loadPreviewAudio(audioPath))} disabled={!audioPath}>Load audio</button><label>Preview quality</label><select defaultValue="preview" onChange={(e) => void setPreviewQuality(e.target.value)}><option value="draft">Draft · 100K</option><option value="preview">Preview · 500K</option><option value="final">Final count</option></select>{project && document && <Inspector project={project} schema={document.parameters} macros={document.macros} active={stats?.activeModulations ?? []} onChange={edit} />}{gpu && <dl><dt>GPU</dt><dd>{gpu.adapter}</dd><dt>Backend</dt><dd>{gpu.backend}</dd><dt>Driver</dt><dd>{gpu.driver}</dd></dl>}{error && <p className="error">{error}</p>}</aside>
    <section className="panel timeline"><h2>Audio & timeline</h2><input className="scrubber" type="range" min="0" max={Math.max(1, lastFrame)} value={Math.min(stats?.frameIndex ?? 0, lastFrame)} onChange={(e) => void seekPreview(Number(e.target.value))} /><div className="waveform" /></section><aside className="panel render"><h2>Render</h2><label>Output</label><select defaultValue="preview"><option value="preview">Preview · 1080p</option><option value="final">Final · 4K</option></select><button disabled>Queue render</button></aside>
  </main>;
}
