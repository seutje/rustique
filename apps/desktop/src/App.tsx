import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Inspector } from "./Inspector";
import { Timeline } from "./Timeline";
import {
  cancelProductionRender, clearPreviews, enqueuePreview, enqueueProductionRender, loadPreviewAudio, loadProject,
  openProductionFolder, openProductionOutput,
  openPreview, queryGpuInfo, queryPreviewJobs, queryPreviewStats, queryProductionStatus,
  randomizeProject, morphProject, makeSeamlessLoop,
  resetPreview, resizeViewport, saveProject, seekPreview, setPreviewPlaying,
  setPreviewQuality, setViewportVisible, updateProject,
  type GpuSummary, type PreviewJob, type PreviewStats, type ProductionSettings,
  type ProductionStatus, type TimelineAudio,
} from "./native";
import { useEditorStore, type ProjectData } from "./store";

const presets = ["Star System", "Nebula", "Liquid Chrome", "Green Slime", "Water Droplets"];
const presetFiles: Record<string, string> = { "Star System": "presets/star-system.json", Nebula: "presets/nebula.json", "Liquid Chrome": "presets/liquid-chrome.json", "Green Slime": "presets/green-slime.json", "Water Droplets": "presets/water-droplets.json" };
type View = "editor" | "exporter";
type Run = (action: () => Promise<void>) => void;

interface ExportPanelProps {
  hidden: boolean;
  project?: ProjectData; audioPath: string; stats: PreviewStats | null;
  sliceRange: [number, number]; previewJobs: PreviewJob[]; production: ProductionSettings;
  productionStatus: ProductionStatus | null; useFinalSliceSettings: boolean;
  setProduction: (value: ProductionSettings) => void;
  setPreviewJobs: (value: PreviewJob[]) => void;
  setUseFinalSliceSettings: (value: boolean) => void; run: Run;
}

function ExportPanel({ hidden, project, audioPath, stats, sliceRange, previewJobs, production, productionStatus, useFinalSliceSettings, setProduction, setPreviewJobs, setUseFinalSliceSettings, run }: ExportPanelProps) {
  const productionActive = productionStatus?.state === "queued" || productionStatus?.state === "rendering";
  const productionBusy = productionActive || productionStatus?.state === "cancelling";
  const productionComplete = productionStatus?.state === "complete" && !!productionStatus.outputPath;
  return <section className="panel exporter-page" hidden={hidden}><div className="exporter-content">
    <div className="exporter-heading"><div><h1>Export</h1><p>Render the complete project or create a review preview.</p></div><div className="export-summary"><span>{project ? `${project.duration_seconds.toFixed(2)} seconds` : "No project loaded"}</span><span>{audioPath || "No audio loaded"}</span></div></div>
    <div className="exporter-columns">
      <section className="export-card"><h2>Production video</h2><label>Output path</label><input value={production.outputPath} onChange={(e) => setProduction({ ...production, outputPath: e.target.value })}/><div className="render-grid">
        <select value={`${production.width}x${production.height}`} onChange={(e) => { const [width, height] = e.target.value.split("x").map(Number); setProduction({ ...production, width, height }); }}><option value="3840x2160">4K</option><option value="1920x1080">1080p</option></select>
        <select value={production.fps} onChange={(e) => setProduction({ ...production, fps: Number(e.target.value) })}><option value="30">30 fps</option><option value="60">60 fps</option></select>
        <label>Supersampling<input type="number" min="1" max="2" step="0.25" value={production.supersampling} onChange={(e) => setProduction({ ...production, supersampling: Number(e.target.value) })}/></label>
        <label>Motion blur<input type="number" min="1" max="16" value={production.motionBlurSamples} onChange={(e) => setProduction({ ...production, motionBlurSamples: Number(e.target.value) })}/></label>
        <label>Simulation steps<input type="number" min="1" max="16" value={production.substeps} onChange={(e) => setProduction({ ...production, substeps: Number(e.target.value) })}/></label>
        <select value={production.codec} onChange={(e) => setProduction({ ...production, codec: e.target.value as ProductionSettings["codec"] })}><option value="h264">H.264</option><option value="hevc">HEVC</option><option value="prores422hq">ProRes 422 HQ</option><option value="prores4444">ProRes 4444 + alpha</option></select>
      </div><button className="primary-action" disabled={!project || !audioPath || productionBusy} onClick={() => project && run(() => enqueueProductionRender(project, audioPath, production))}>Render complete video</button>
      {productionStatus && <div className="production-status"><progress max="1" value={productionStatus.totalFrames ? productionStatus.completedFrames / productionStatus.totalFrames : 0}/><span>{productionStatus.state}{productionStatus.etaSeconds != null ? ` · ETA ${Math.ceil(productionStatus.etaSeconds)}s` : ""}</span><small>{productionStatus.error ?? productionStatus.manifestPath ?? productionStatus.outputPath}</small><div className="production-actions"><button disabled={!productionComplete} onClick={() => run(openProductionOutput)}>Open</button><button disabled={!productionComplete} onClick={() => run(openProductionFolder)}>Folder</button><button className="danger" disabled={!productionActive} onClick={() => run(cancelProductionRender)}>Cancel</button></div></div>}</section>
      <section className="export-card"><h2>Preview renders</h2><p>Create a final-quality still at the playhead or render the selected timeline range.</p>
        <button disabled={!project || !audioPath} onClick={() => project && run(async () => { await enqueuePreview("still", project, audioPath, stats?.frameIndex ?? 0, (stats?.frameIndex ?? 0) + 1, true); setPreviewJobs(await queryPreviewJobs()); })}>Queue 4K still</button>
        <button disabled={!project || !audioPath} onClick={() => project && run(async () => { await enqueuePreview("slice", project, audioPath, Math.round(sliceRange[0] * project.fps), Math.round(sliceRange[1] * project.fps), useFinalSliceSettings); setPreviewJobs(await queryPreviewJobs()); })}>Queue timeline slice</button>
        <label className="check-label"><input type="checkbox" checked={useFinalSliceSettings} onChange={(e) => setUseFinalSliceSettings(e.target.checked)}/> Use final settings for slice</label>
        <button onClick={() => run(async () => { await clearPreviews(); setPreviewJobs([]); })}>Clear completed</button>
        {previewJobs.map((job) => <div className="preview-job" key={job.id}><div className="preview-job-title"><span>{job.kind} · {job.state}</span><button disabled={job.state !== "complete" || !job.outputPath} onClick={() => job.outputPath && run(() => openPreview(job.outputPath!))}>Open</button></div><progress value={job.progress} max="1"/><small>{job.error ?? job.outputPath}</small></div>)}
      </section>
    </div></div></section>;
}

export function App() {
  const { document, dirty, setDocument, replaceProject } = useEditorStore();
  const [view, setView] = useState<View>("editor");
  const [selectedPreset, setSelectedPreset] = useState<string | null>(null);
  const [mutationAmount, setMutationAmount] = useState(0.25);
  const [variation, setVariation] = useState(0);
  const [morphFrom, setMorphFrom] = useState("Star System");
  const [morphTo, setMorphTo] = useState("Nebula");
  const [morphAmount, setMorphAmount] = useState(0.5);
  const [projectPath, setProjectPath] = useState("examples/star-orbit.rustique.json");
  const [audioPath, setAudioPath] = useState("");
  const [gpu, setGpu] = useState<GpuSummary | null>(null);
  const [previewQuality, setPreviewQualityState] = useState("preview");
  const [stats, setStats] = useState<PreviewStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const [timelineAudio, setTimelineAudio] = useState<TimelineAudio | null>(null);
  const [sliceRange, setSliceRange] = useState<[number, number]>([0, 5]);
  const [previewJobs, setPreviewJobs] = useState<PreviewJob[]>([]);
  const [useFinalSliceSettings, setUseFinalSliceSettings] = useState(false);
  const [production, setProduction] = useState<ProductionSettings>({ outputPath: "rustique-render.mp4", width: 3840, height: 2160, fps: 60, supersampling: 1, motionBlurSamples: 1, substeps: 2, codec: "h264" });
  const [productionStatus, setProductionStatus] = useState<ProductionStatus | null>(null);
  const previewUpdateTimer = useRef<number | null>(null);
  const previewUpdateGeneration = useRef(0);

  useEffect(() => {
    if (view !== "editor") { void setViewportVisible(false).catch(() => undefined); return; }
    const viewport = viewportRef.current;
    if (!viewport) return;
    void setViewportVisible(true).catch(() => undefined);
    const updateBounds = () => { const bounds = viewport.getBoundingClientRect(); const scale = window.devicePixelRatio; void resizeViewport(Math.round(bounds.x * scale), Math.round(bounds.y * scale), Math.round(bounds.width * scale), Math.round(bounds.height * scale)); };
    const observer = new ResizeObserver(updateBounds);
    observer.observe(viewport); window.addEventListener("resize", updateBounds); updateBounds();
    return () => { observer.disconnect(); window.removeEventListener("resize", updateBounds); void setViewportVisible(false).catch(() => undefined); };
  }, [view]);
  useEffect(() => { const timer = window.setInterval(() => void queryPreviewStats().then(setStats).catch(() => undefined), 250); return () => window.clearInterval(timer); }, []);
  useEffect(() => { const timer = window.setInterval(() => void queryPreviewJobs().then(setPreviewJobs).catch(() => undefined), 500); return () => window.clearInterval(timer); }, []);
  useEffect(() => { const timer = window.setInterval(() => void queryProductionStatus().then(setProductionStatus).catch(() => undefined), 500); return () => window.clearInterval(timer); }, []);
  useEffect(() => { void run(async () => setGpu(await queryGpuInfo())); }, []);
  useEffect(() => () => { if (previewUpdateTimer.current !== null) window.clearTimeout(previewUpdateTimer.current); }, []);

  async function run(action: () => Promise<void>) { try { setError(null); await action(); } catch (reason) { setError(String(reason)); } }
  function edit(project: ProjectData) {
    replaceProject(project);
    const generation = ++previewUpdateGeneration.current;
    if (previewUpdateTimer.current !== null) window.clearTimeout(previewUpdateTimer.current);
    previewUpdateTimer.current = window.setTimeout(() => { previewUpdateTimer.current = null; void run(async () => { const validated = await updateProject(project); if (previewUpdateGeneration.current === generation) replaceProject(validated); }); }, 200);
  }
  async function openProject() {
    previewUpdateGeneration.current += 1;
    if (previewUpdateTimer.current !== null) window.clearTimeout(previewUpdateTimer.current);
    previewUpdateTimer.current = null;
    const loaded = await loadProject(projectPath);
    setDocument(loaded); setProjectPath(loaded.path);
  }
  async function browseProject() {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Rustique project", extensions: ["json"] }],
    });
    if (selected) setProjectPath(selected);
  }
  async function browseAudio() {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Audio", extensions: ["wav", "mp3", "flac", "m4a", "aac", "mp4"] }],
    });
    if (selected) setAudioPath(selected);
  }
  async function saveCurrentProject() {
    if (!document) return;
    previewUpdateGeneration.current += 1;
    if (previewUpdateTimer.current !== null) window.clearTimeout(previewUpdateTimer.current);
    previewUpdateTimer.current = null;
    const validated = await updateProject(document.project);
    const savedPath = await saveProject(projectPath, validated);
    setDocument({ ...document, path: savedPath, project: validated });
    setProjectPath(savedPath);
  }
  const project = document?.project;
  async function loadAudio() {
    if (!project) return;
    const audio = await loadPreviewAudio(audioPath);
    setTimelineAudio(audio); edit({ ...project, duration_seconds: audio.durationSeconds }); setSliceRange([0, Math.min(5, audio.durationSeconds)]);
  }
  const lastFrame = project ? Math.max(0, Math.ceil(project.duration_seconds * project.fps) - 1) : 0;
  const previewCap = gpu?.maxPreviewParticles ?? 100_000;
  const qualityCount = (quality: string) => Math.min(quality === "draft" ? 100_000 : quality === "preview" ? 500_000 : (project?.particle_system.count ?? previewCap), previewCap);

  return <main className={`studio-shell ${view}-view`}><header className="topbar"><div><span className="brand">RUSTIQUE</span><span className="subtitle">particle studio {dirty ? "• unsaved" : ""}</span></div><div className="topbar-actions"><button className={view === "exporter" ? "selected" : ""} onClick={() => setView(view === "editor" ? "exporter" : "editor")}>{view === "editor" ? "Export" : "Back to editor"}</button><button onClick={() => void run(async () => setGpu(await queryGpuInfo()))}>Inspect GPU</button></div></header>
      <aside className="panel presets" hidden={view !== "editor"}><h2>Visual presets</h2>{presets.map((preset) => <button className={selectedPreset === preset ? "selected" : ""} key={preset} onClick={() => setSelectedPreset(preset)}>{preset}</button>)}
        <h2 className="creative-heading">Creative tools</h2><label>Mutation amount <output>{mutationAmount.toFixed(2)}</output></label><input type="range" min="0" max="1" step="0.01" value={mutationAmount} onChange={(e) => setMutationAmount(Number(e.target.value))}/><button disabled={!project} onClick={() => project && void run(async () => { const next = variation + 1; setVariation(next); edit(await randomizeProject(document?.path ?? projectPath, project, mutationAmount, next)); })}>Randomize variation</button>
        <label>Morph from</label><select value={morphFrom} onChange={(e) => setMorphFrom(e.target.value)}>{presets.map((name) => <option key={name}>{name}</option>)}</select><label>Morph to</label><select value={morphTo} onChange={(e) => setMorphTo(e.target.value)}>{presets.map((name) => <option key={name}>{name}</option>)}</select><label>Morph <output>{morphAmount.toFixed(2)}</output></label><input type="range" min="0" max="1" step="0.01" value={morphAmount} onChange={(e) => setMorphAmount(Number(e.target.value))}/><button disabled={!project} onClick={() => project && void run(async () => edit(await morphProject(project, presetFiles[morphFrom], presetFiles[morphTo], morphAmount)))}>Apply morph</button><button disabled={!project} onClick={() => project && void run(async () => edit(await makeSeamlessLoop(project)))}>Make camera loop</button>
      </aside>
      <aside className="panel inspector" hidden={view !== "editor"}><h2>Inspector</h2><label>Project path</label><div className="path-input"><input value={projectPath} onChange={(e) => setProjectPath(e.target.value)} /><button onClick={() => void run(browseProject)}>Browse</button></div><div className="button-row"><button onClick={() => void run(openProject)}>Load</button><button disabled={!project} onClick={() => void run(saveCurrentProject)}>Save</button></div><label>Preview audio</label><div className="path-input"><input value={audioPath} placeholder="C:\\music\\track.wav" onChange={(e) => setAudioPath(e.target.value)} /><button onClick={() => void run(browseAudio)}>Browse</button></div><button onClick={() => void run(loadAudio)} disabled={!audioPath || !project}>Load audio</button><label>Preview quality</label><select value={previewQuality} onChange={(e) => { setPreviewQualityState(e.target.value); void setPreviewQuality(e.target.value); }}><option value="draft">Draft · {qualityCount("draft").toLocaleString()}</option><option value="preview">Preview · {qualityCount("preview").toLocaleString()}</option><option value="final">Project · {qualityCount("final").toLocaleString()} max</option></select>{project && document && <Inspector project={project} schema={document.parameters} macros={document.macros} active={stats?.activeModulations ?? []} onChange={edit} />}{gpu && <dl><dt>GPU</dt><dd>{gpu.adapter}</dd><dt>Backend</dt><dd>{gpu.backend}</dd><dt>Driver</dt><dd>{gpu.driver}</dd><dt>Preview cap</dt><dd>{gpu.maxPreviewParticles.toLocaleString()} particles</dd><dt>Scan</dt><dd>{gpu.benchmarkGpuMs == null ? "Adapter limits" : `${gpu.benchmarkGpuMs.toFixed(2)} ms at ${gpu.benchmarkParticles.toLocaleString()}`}</dd></dl>}{error && <p className="error">{error}</p>}</aside>
      <section className="viewport" hidden={view !== "editor"}><div className="viewport-grid" ref={viewportRef}/><div className="transport"><button onClick={() => void resetPreview()}>Reset</button><button className={stats?.playing ? "playing" : ""} onClick={() => void setPreviewPlaying(!(stats?.playing ?? false))}>{stats?.playing ? "Pause" : "Play"}</button><span>frame {stats?.frameIndex ?? 0} / {lastFrame}</span><span>{project ? ((stats?.frameIndex ?? 0) / project.fps).toFixed(2) : "0.00"}s</span><span>{stats?.framesPerSecond.toFixed(1) ?? "0.0"} fps</span><span>{stats?.particleCount.toLocaleString() ?? 0} particles</span><span>GPU {stats?.gpuRenderMs?.toFixed(2) ?? "—"} ms</span></div></section>
      <section className="panel timeline" hidden={view !== "editor"}><h2>Audio & timeline</h2><Timeline project={project ?? null} audio={timelineAudio} cursorSeconds={project ? (stats?.frameIndex ?? 0) / project.fps : 0} slice={sliceRange} onSeek={(seconds) => project && void seekPreview(Math.round(seconds * project.fps))} onProjectChange={edit} onSliceChange={setSliceRange}/></section>
      <ExportPanel hidden={view !== "exporter"} project={project} audioPath={audioPath} stats={stats} sliceRange={sliceRange} previewJobs={previewJobs} production={production} productionStatus={productionStatus} useFinalSliceSettings={useFinalSliceSettings} setProduction={setProduction} setPreviewJobs={setPreviewJobs} setUseFinalSliceSettings={setUseFinalSliceSettings} run={run}/> {view === "exporter" && error && <p className="global-error error">{error}</p>}
  </main>;
}
