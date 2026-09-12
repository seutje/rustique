import { useEffect, useRef, useState } from "react";
import {
  loadPreviewAudio, loadProject, queryGpuInfo, queryPreviewStats, resetPreview,
  resizeViewport, seekPreview, setPreviewPlaying, setPreviewQuality,
  type GpuSummary, type PreviewStats,
} from "./native";
import { useEditorStore } from "./store";

const presets = ["Star System", "Nebula", "Liquid Chrome", "Green Slime", "Water Droplets"];

export function App() {
  const { project, selectedPreset, setProject, setSelectedPreset } = useEditorStore();
  const [projectPath, setProjectPath] = useState("examples/star-orbit.rustique.json");
  const [audioPath, setAudioPath] = useState("");
  const [gpu, setGpu] = useState<GpuSummary | null>(null);
  const [stats, setStats] = useState<PreviewStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const viewportRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const updateBounds = () => {
      const bounds = viewport.getBoundingClientRect();
      const scale = window.devicePixelRatio;
      void resizeViewport(Math.round(bounds.x * scale), Math.round(bounds.y * scale), Math.round(bounds.width * scale), Math.round(bounds.height * scale));
    };
    const observer = new ResizeObserver(updateBounds);
    observer.observe(viewport);
    window.addEventListener("resize", updateBounds);
    updateBounds();
    return () => { observer.disconnect(); window.removeEventListener("resize", updateBounds); };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => void queryPreviewStats().then(setStats), 250);
    return () => window.clearInterval(timer);
  }, []);

  async function inspectGpu() {
    try { setError(null); setGpu(await queryGpuInfo()); } catch (reason) { setError(String(reason)); }
  }
  async function openProject() {
    try { setError(null); setProject(await loadProject(projectPath)); } catch (reason) { setError(String(reason)); }
  }
  async function openAudio() {
    try { setError(null); await loadPreviewAudio(audioPath); } catch (reason) { setError(String(reason)); }
  }

  const lastFrame = project ? Math.max(0, Math.ceil(project.durationSeconds * project.fps) - 1) : 0;
  return (
    <main className="studio-shell">
      <header className="topbar"><div><span className="brand">RUSTIQUE</span><span className="subtitle">particle studio</span></div><button onClick={inspectGpu}>Inspect GPU</button></header>
      <aside className="panel presets"><h2>Visual presets</h2>{presets.map((preset) => <button className={selectedPreset === preset ? "selected" : ""} key={preset} onClick={() => setSelectedPreset(preset)}>{preset}</button>)}</aside>
      <section className="viewport">
        <div className="viewport-grid" ref={viewportRef} />
        <div className="transport"><button onClick={() => void resetPreview()}>Reset</button><button className={stats?.playing ? "playing" : ""} onClick={() => void setPreviewPlaying(!(stats?.playing ?? false))}>{stats?.playing ? "Pause" : "Play"}</button><span>frame {stats?.frameIndex ?? 0} / {lastFrame}</span><span>{project ? ((stats?.frameIndex ?? 0) / project.fps).toFixed(2) : "0.00"}s</span><span>{stats?.framesPerSecond.toFixed(1) ?? "0.0"} fps</span><span>{stats?.particleCount.toLocaleString() ?? 0} particles</span><span>GPU {stats?.gpuRenderMs?.toFixed(2) ?? "—"} ms</span></div>
      </section>
      <aside className="panel inspector">
        <h2>Inspector</h2><label>Project path</label><input value={projectPath} onChange={(event) => setProjectPath(event.target.value)} /><button onClick={openProject}>Load project</button>
        <label>Preview audio</label><input value={audioPath} placeholder="C:\\music\\track.wav" onChange={(event) => setAudioPath(event.target.value)} /><button onClick={openAudio} disabled={!audioPath}>Load audio</button>
        <label>Preview quality</label><select defaultValue="preview" onChange={(event) => void setPreviewQuality(event.target.value)}><option value="draft">Draft · 100K</option><option value="preview">Preview · 500K</option><option value="final">Final count</option></select>
        {project && <dl><dt>Particles</dt><dd>{project.particleCount.toLocaleString()}</dd><dt>Timeline</dt><dd>{project.fps} fps</dd><dt>Engine</dt><dd>{project.engineVersion}</dd></dl>}
        {gpu && <dl><dt>GPU</dt><dd>{gpu.adapter}</dd><dt>Backend</dt><dd>{gpu.backend}</dd><dt>Driver</dt><dd>{gpu.driver}</dd></dl>}{error && <p className="error">{error}</p>}
      </aside>
      <section className="panel timeline"><h2>Audio & timeline</h2><input className="scrubber" type="range" min="0" max={Math.max(1, lastFrame)} value={Math.min(stats?.frameIndex ?? 0, lastFrame)} onChange={(event) => void seekPreview(Number(event.target.value))} /><div className="waveform" /></section>
      <aside className="panel render"><h2>Render</h2><label>Output</label><select defaultValue="preview"><option value="preview">Preview · 1080p</option><option value="final">Final · 4K</option></select><button disabled>Queue render</button></aside>
    </main>
  );
}
