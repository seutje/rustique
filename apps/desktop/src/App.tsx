import { useEffect, useRef, useState } from "react";
import { Inspector } from "./Inspector";
import { Timeline } from "./Timeline";
import { clearPreviews, enqueuePreview, enqueueProductionRender, loadPreviewAudio, loadProject, openPreview, queryGpuInfo, queryPreviewJobs, queryPreviewStats, queryProductionStatus, resetPreview, resizeViewport, saveProject, seekPreview, setPreviewPlaying, setPreviewQuality, updateProject, type GpuSummary, type PreviewJob, type PreviewStats, type ProductionSettings, type ProductionStatus, type TimelineAudio } from "./native";
import { useEditorStore, type ProjectData } from "./store";
const presets = ["Star System", "Nebula", "Liquid Chrome", "Green Slime", "Water Droplets"];
export function App() {
  const { document, dirty, setDocument, replaceProject } = useEditorStore(); const [selectedPreset, setSelectedPreset] = useState<string | null>(null);
  const [projectPath, setProjectPath] = useState("examples/star-orbit.rustique.json"); const [audioPath, setAudioPath] = useState(""); const [gpu, setGpu] = useState<GpuSummary | null>(null); const [stats, setStats] = useState<PreviewStats | null>(null); const [error, setError] = useState<string | null>(null); const viewportRef = useRef<HTMLDivElement>(null);
  const [timelineAudio, setTimelineAudio] = useState<TimelineAudio | null>(null);
  const [sliceRange, setSliceRange] = useState<[number, number]>([0, 5]);
  const [previewJobs, setPreviewJobs] = useState<PreviewJob[]>([]);
  const [useFinalSliceSettings, setUseFinalSliceSettings] = useState(false);
  const [production, setProduction] = useState<ProductionSettings>({ outputPath: "rustique-render.mp4", width: 3840, height: 2160, fps: 60, supersampling: 1, motionBlurSamples: 1, substeps: 2, codec: "h264" });
  const [productionStatus, setProductionStatus] = useState<ProductionStatus | null>(null);
  const previewUpdateTimer = useRef<number | null>(null);
  const previewUpdateGeneration = useRef(0);
  useEffect(() => { const viewport = viewportRef.current; if (!viewport) return; const updateBounds = () => { const bounds = viewport.getBoundingClientRect(); const scale = window.devicePixelRatio; void resizeViewport(Math.round(bounds.x * scale), Math.round(bounds.y * scale), Math.round(bounds.width * scale), Math.round(bounds.height * scale)); }; const observer = new ResizeObserver(updateBounds); observer.observe(viewport); window.addEventListener("resize", updateBounds); updateBounds(); return () => { observer.disconnect(); window.removeEventListener("resize", updateBounds); }; }, []);
  useEffect(() => { const timer = window.setInterval(() => void queryPreviewStats().then(setStats).catch(() => undefined), 250); return () => window.clearInterval(timer); }, []);
  useEffect(() => { const timer = window.setInterval(() => void queryPreviewJobs().then(setPreviewJobs).catch(() => undefined), 500); return () => window.clearInterval(timer); }, []);
  useEffect(() => { const timer = window.setInterval(() => void queryProductionStatus().then(setProductionStatus).catch(() => undefined), 500); return () => window.clearInterval(timer); }, []);
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
    <aside className="panel inspector"><h2>Inspector</h2><label>Project path</label><input value={projectPath} onChange={(e) => setProjectPath(e.target.value)} /><div className="button-row"><button onClick={() => void run(openProject)}>Load</button><button disabled={!project} onClick={() => void run(saveCurrentProject)}>Save</button></div><label>Preview audio</label><input value={audioPath} placeholder="C:\\music\\track.wav" onChange={(e) => setAudioPath(e.target.value)} /><button onClick={() => void run(async () => setTimelineAudio(await loadPreviewAudio(audioPath)))} disabled={!audioPath}>Load audio</button><label>Preview quality</label><select defaultValue="preview" onChange={(e) => void setPreviewQuality(e.target.value)}><option value="draft">Draft · 100K</option><option value="preview">Preview · 500K</option><option value="final">Final count</option></select>{project && document && <Inspector project={project} schema={document.parameters} macros={document.macros} active={stats?.activeModulations ?? []} onChange={edit} />}{gpu && <dl><dt>GPU</dt><dd>{gpu.adapter}</dd><dt>Backend</dt><dd>{gpu.backend}</dd><dt>Driver</dt><dd>{gpu.driver}</dd></dl>}{error && <p className="error">{error}</p>}</aside>
    <section className="panel timeline"><h2>Audio & timeline</h2><Timeline project={project ?? null} audio={timelineAudio} cursorSeconds={project ? (stats?.frameIndex ?? 0) / project.fps : 0} onSeek={(seconds) => project && void seekPreview(Math.round(seconds * project.fps))} onProjectChange={edit} onSliceChange={setSliceRange} /></section><aside className="panel render"><h2>Production export</h2><input value={production.outputPath} onChange={(e) => setProduction({ ...production, outputPath: e.target.value })}/><div className="render-grid"><select value={`${production.width}x${production.height}`} onChange={(e) => { const [width, height] = e.target.value.split("x").map(Number); setProduction({ ...production, width, height }); }}><option value="3840x2160">4K</option><option value="1920x1080">1080p</option></select><select value={production.fps} onChange={(e) => setProduction({ ...production, fps: Number(e.target.value) })}><option value="30">30 fps</option><option value="60">60 fps</option></select><label>SS<input type="number" min="1" max="2" step="0.25" value={production.supersampling} onChange={(e) => setProduction({ ...production, supersampling: Number(e.target.value) })}/></label><label>Blur<input type="number" min="1" max="16" value={production.motionBlurSamples} onChange={(e) => setProduction({ ...production, motionBlurSamples: Number(e.target.value) })}/></label><label>Steps<input type="number" min="1" max="16" value={production.substeps} onChange={(e) => setProduction({ ...production, substeps: Number(e.target.value) })}/></label><select value={production.codec} onChange={(e) => setProduction({ ...production, codec: e.target.value as "h264" | "hevc" })}><option value="h264">H.264</option><option value="hevc">HEVC</option></select></div><button disabled={!project || !audioPath || productionStatus?.state === "rendering" || productionStatus?.state === "queued"} onClick={() => project && void run(() => enqueueProductionRender(project, audioPath, production))}>Render complete video</button>{productionStatus && <div className="production-status"><progress max="1" value={productionStatus.totalFrames ? productionStatus.completedFrames / productionStatus.totalFrames : 0}/><span>{productionStatus.state}{productionStatus.etaSeconds != null ? ` · ETA ${Math.ceil(productionStatus.etaSeconds)}s` : ""}</span><small>{productionStatus.error ?? productionStatus.manifestPath}</small></div>}<details><summary>Preview renders</summary><button disabled={!project || !audioPath} onClick={() => project && void run(async () => { await enqueuePreview("still", project, audioPath, stats?.frameIndex ?? 0, (stats?.frameIndex ?? 0) + 1, true); setPreviewJobs(await queryPreviewJobs()); })}>Queue 4K still</button><button disabled={!project || !audioPath} onClick={() => project && void run(async () => { await enqueuePreview("slice", project, audioPath, Math.round(sliceRange[0] * project.fps), Math.round(sliceRange[1] * project.fps), useFinalSliceSettings); setPreviewJobs(await queryPreviewJobs()); })}>Queue slice</button><label className="check-label"><input type="checkbox" checked={useFinalSliceSettings} onChange={(e) => setUseFinalSliceSettings(e.target.checked)}/> Use final settings</label><button onClick={() => void run(async () => { await clearPreviews(); setPreviewJobs([]); })}>Clear completed</button>{previewJobs.map((job) => <div className="preview-job" key={job.id}><div className="preview-job-title"><span>{job.kind} · {job.state}</span><button disabled={job.state !== "complete" || !job.outputPath} onClick={() => job.outputPath && void run(() => openPreview(job.outputPath!))}>Open</button></div><progress value={job.progress} max="1"/><small>{job.error ?? job.outputPath}</small></div>)}</details></aside>
  </main>;
}
