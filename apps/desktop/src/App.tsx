import { useState } from "react";
import { loadProject, queryGpuInfo, type GpuSummary } from "./native";
import { useEditorStore } from "./store";

const presets = ["Star System", "Nebula", "Liquid Chrome", "Green Slime", "Water Droplets"];

export function App() {
  const { project, selectedPreset, setProject, setSelectedPreset } = useEditorStore();
  const [projectPath, setProjectPath] = useState("../../examples/star-orbit.rustique.json");
  const [gpu, setGpu] = useState<GpuSummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function inspectGpu() {
    try {
      setError(null);
      setGpu(await queryGpuInfo());
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function openProject() {
    try {
      setError(null);
      setProject(await loadProject(projectPath));
    } catch (reason) {
      setError(String(reason));
    }
  }

  return (
    <main className="studio-shell">
      <header className="topbar">
        <div><span className="brand">RUSTIQUE</span><span className="subtitle">particle studio</span></div>
        <button onClick={inspectGpu}>Inspect GPU</button>
      </header>

      <aside className="panel presets">
        <h2>Visual presets</h2>
        {presets.map((preset) => (
          <button className={selectedPreset === preset ? "selected" : ""} key={preset} onClick={() => setSelectedPreset(preset)}>
            {preset}
          </button>
        ))}
      </aside>

      <section className="viewport">
        <div className="viewport-grid"><span>Viewport arrives in Phase 17</span></div>
        <div className="transport"><button>◀</button><button>▶</button><span>00:00:00 / {project?.durationSeconds.toFixed(1) ?? "--"}s</span></div>
      </section>

      <aside className="panel inspector">
        <h2>Inspector</h2>
        <label>Project path</label>
        <input value={projectPath} onChange={(event) => setProjectPath(event.target.value)} />
        <button onClick={openProject}>Load project</button>
        {project && <dl><dt>Particles</dt><dd>{project.particleCount.toLocaleString()}</dd><dt>Timeline</dt><dd>{project.fps} fps</dd><dt>Engine</dt><dd>{project.engineVersion}</dd></dl>}
        {gpu && <dl><dt>GPU</dt><dd>{gpu.adapter}</dd><dt>Backend</dt><dd>{gpu.backend}</dd><dt>Driver</dt><dd>{gpu.driver}</dd></dl>}
        {error && <p className="error">{error}</p>}
      </aside>

      <section className="panel timeline"><h2>Audio & timeline</h2><div className="waveform" /></section>
      <aside className="panel render"><h2>Render</h2><label>Output</label><select defaultValue="preview"><option value="preview">Preview · 1080p</option><option value="final">Final · 4K</option></select><button disabled>Queue render</button></aside>
    </main>
  );
}
