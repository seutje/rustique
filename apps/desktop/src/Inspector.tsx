import type { ChangeEvent } from "react";
import type { MacroSchema, ModulationMapping, ModulationSource, ModulationTarget, ParameterSchema, ProjectData } from "./store";
import type { ActiveModulation } from "./native";
import { LiveInput } from "./LiveInput";

const sources: ModulationSource[] = ["sub", "bass", "low_mids", "mids", "high_mids", "highs", "rms", "transient", "spectral_centroid", "spectral_flux"];
const targets: ModulationTarget[] = ["gravity_strength", "particle_size", "brightness", "hue_shift", "burst_emission", "camera_fov", "camera_shake", "material_roughness", "reflection_intensity", "surface_scale", "volume_density", "volume_motion", "droplet_density", "droplet_size", "droplet_refraction", "droplet_gravity", "flocking_separation", "flocking_cohesion", "flocking_turbulence", "flocking_speed", "flocking_randomness", "flocking_impulse", "fire_emission", "fire_base_width", "fire_height", "fire_sway", "fire_turbulence", "fire_flicker", "fire_shimmer", "fire_sparks", "fire_temperature", "fire_beat_wave"];
const analysisProfiles = [
  ["Techno", "../profiles/analysis/techno.json"],
  ["Drum & Bass", "../profiles/analysis/drum-and-bass.json"],
  ["Ambient", "../profiles/analysis/ambient.json"],
  ["Cinematic", "../profiles/analysis/cinematic.json"],
] as const;
const reactionProfiles = [
  ["Punchy", "../profiles/reaction/punchy.json"],
  ["Fluid", "../profiles/reaction/fluid.json"],
  ["Dreamy", "../profiles/reaction/dreamy.json"],
  ["Aggressive", "../profiles/reaction/aggressive.json"],
] as const;
const label = (value: string) => value.replaceAll("_", " ");
function getPath(project: ProjectData, path: string): unknown { return path.split(".").reduce<unknown>((value, key) => (value as Record<string, unknown>)[key], project); }
function setPath(project: ProjectData, path: string, value: unknown): ProjectData { const copy = structuredClone(project) as ProjectData; const keys = path.split("."); let cursor = copy as unknown as Record<string, unknown>; keys.slice(0, -1).forEach((key) => { cursor = cursor[key] as Record<string, unknown>; }); cursor[keys.at(-1)!] = value; return copy; }
function colorHex(channels: number[]) { return `#${channels.slice(0, 3).map((v) => Math.round(v * 255).toString(16).padStart(2, "0")).join("")}`; }
function hexChannels(hex: string, alpha?: number) { const rgb = [1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16) / 255); return alpha === undefined ? rgb : rgb.concat(alpha); }

interface Props { project: ProjectData; schema: ParameterSchema[]; macros: MacroSchema[]; active: ActiveModulation[]; onChange: (project: ProjectData) => void; }
export function Inspector({ project, schema, macros, active, onChange }: Props) {
  function selectProfile(kind: "analysis_profile" | "reaction_profile", source: string) {
    onChange({ ...project, [kind]: source ? { source, overrides: {} } : null });
  }
  function addMapping(target: ModulationTarget = "brightness") { const mapping: ModulationMapping = { enabled: true, source: "bass", target, amount: 1, offset: 0, minimum: 0, maximum: 1, polarity: "normal", curve: "linear", combine: "replace", attack_seconds: 0.02, release_seconds: 0.2 }; onChange({ ...project, modulation_mappings: [...project.modulation_mappings, mapping] }); }
  function updateMapping(index: number, patch: Partial<ModulationMapping>) { onChange({ ...project, modulation_mappings: project.modulation_mappings.map((m, at) => at === index ? { ...m, ...patch } : m) }); }
  function numeric(index: number, key: keyof ModulationMapping, event: ChangeEvent<HTMLInputElement>) { updateMapping(index, { [key]: Number(event.target.value) }); }
  function updateMacro(macro: MacroSchema, value: number) { let next = structuredClone(project) as ProjectData; const selection = next.visual_preset as { overrides?: { macros?: Record<string, number> } } | null; const oldValue = selection?.overrides?.macros?.[macro.id] ?? macro.default; if (selection) { selection.overrides ??= {}; selection.overrides.macros ??= {}; selection.overrides.macros[macro.id] = value; } const paths: Record<string, string> = { particle_count: "particle_system.count", particle_size: "render_defaults.particle_size_pixels", orbit_speed: "camera.orbit_degrees_per_second", dolly_speed: "camera.dolly_units_per_second", camera_fov: "camera.vertical_fov_degrees", camera_shake: "camera.shake_amplitude", fire_base_radius: "particle_system.initialization.base_radius", fire_height: "particle_system.initialization.flame_height", fire_buoyancy: "particle_system.initialization.buoyancy", fire_turbulence: "particle_system.initialization.turbulence", fire_flicker: "particle_system.initialization.flicker", fire_spark_amount: "particle_system.initialization.spark_ratio", fire_spark_velocity: "particle_system.initialization.spark_velocity", fire_spark_lifetime: "particle_system.initialization.spark_lifetime", fire_temperature: "particle_system.initialization.temperature", fire_audio_reactivity: "particle_system.initialization.audio_reactivity", fire_beat_wave: "particle_system.initialization.beat_wave_strength" }; if (paths[macro.target]) next = setPath(next, paths[macro.target], macro.target === "particle_count" ? Math.round(value) : value); if (macro.target === "force_strength_scale" && oldValue !== 0) { const ratio = value / oldValue; next.forces = next.forces.map((force) => { const copy = structuredClone(force) as Record<string, unknown>; if (typeof copy.strength === "number") copy.strength *= ratio; if (typeof copy.coefficient === "number") copy.coefficient *= ratio; if (Array.isArray(copy.acceleration)) copy.acceleration = copy.acceleration.map((component) => Number(component) * ratio); return copy; }); } onChange(next); }
  return <>
    <LiveInput />
    <h3>Audio profiles</h3>
    <label htmlFor="analysis-profile">Analysis profile</label>
    <select id="analysis-profile" value={project.analysis_profile?.source ?? ""} onChange={(event) => selectProfile("analysis_profile", event.target.value)}>
      <option value="">None</option>
      {project.analysis_profile && !analysisProfiles.some(([, source]) => source === project.analysis_profile?.source) && <option value={project.analysis_profile.source}>{project.analysis_profile.source}</option>}
      {analysisProfiles.map(([name, source]) => <option value={source} key={source}>{name}</option>)}
    </select>
    <label htmlFor="reaction-profile">Reaction profile</label>
    <select id="reaction-profile" value={project.reaction_profile?.source ?? ""} onChange={(event) => selectProfile("reaction_profile", event.target.value)}>
      <option value="">None</option>
      {project.reaction_profile && !reactionProfiles.some(([, source]) => source === project.reaction_profile?.source) && <option value={project.reaction_profile.source}>{project.reaction_profile.source}</option>}
      {reactionProfiles.map(([name, source]) => <option value={source} key={source}>{name}</option>)}
    </select>
    <h3>Parameters</h3>
    {schema.map((item) => { const value = getPath(project, item.path); return <div className="parameter" key={item.path}><div className="parameter-title"><label>{item.label}</label>{item.modulationTarget && <button className="mod-button" title="Add modulation" onClick={() => addMapping(item.modulationTarget!)}>◇</button>}</div>
      {item.kind === "number" && <div className="number-control"><input type="range" min={item.minimum!} max={item.maximum!} step={item.step!} value={Number(value)} onChange={(e) => onChange(setPath(project, item.path, Number(e.target.value)))} /><input type="number" min={item.minimum!} max={item.maximum!} step={item.step!} value={Number(value)} onChange={(e) => onChange(setPath(project, item.path, Number(e.target.value)))} /></div>}
      {item.kind === "toggle" && <input type="checkbox" checked={typeof value === "boolean" ? value : value === "orbit"} onChange={(e) => onChange(setPath(project, item.path, typeof value === "boolean" ? e.target.checked : e.target.checked ? "orbit" : "static"))} />}
      {item.kind === "color" && <input type="color" value={colorHex(value as number[])} onChange={(e) => onChange(setPath(project, item.path, hexChannels(e.target.value, (value as number[])[3])))} />}
    </div>; })}
    {macros.length > 0 && <><h3>Preset macros</h3>{macros.map((macro) => { const selection = project.visual_preset as { overrides?: { macros?: Record<string, number> } } | null; const value = selection?.overrides?.macros?.[macro.id] ?? macro.default; const step = macro.target === "particle_count" ? 1000 : 0.01; return <div className="parameter" key={macro.id}><label>{macro.label}</label><div className="number-control"><input type="range" min={macro.minimum} max={macro.maximum} step={step} value={value} onChange={(e) => updateMacro(macro, Number(e.target.value))}/><input type="number" min={macro.minimum} max={macro.maximum} step={step} value={value} onChange={(e) => updateMacro(macro, Number(e.target.value))}/></div></div>; })}</>}
    <div className="section-title"><h3>Modulation</h3><button onClick={() => addMapping()}>+ Add</button></div>
    {project.modulation_mappings.map((mapping, index) => { const live = active.find((item) => item.target.replaceAll("_", "") === mapping.target.replaceAll("_", "")); return <details className="mapping" key={index} open><summary><input type="checkbox" checked={mapping.enabled} onChange={(e) => updateMapping(index, { enabled: e.target.checked })} onClick={(e) => e.stopPropagation()} /> {label(mapping.source)} → {label(mapping.target)} <output>{mapping.enabled ? (live?.outputValue.toFixed(3) ?? "—") : "off"}</output></summary>
      <div className="mapping-grid"><label>Source<select value={mapping.source} onChange={(e) => updateMapping(index, { source: e.target.value as ModulationSource })}>{sources.map((v) => <option key={v} value={v}>{label(v)}</option>)}</select></label><label>Target<select value={mapping.target} onChange={(e) => updateMapping(index, { target: e.target.value as ModulationTarget })}>{targets.map((v) => <option key={v} value={v}>{label(v)}</option>)}</select></label>
      {(["amount", "offset", "minimum", "maximum", "attack_seconds", "release_seconds"] as const).map((key) => <label key={key}>{label(key)}<input type="number" step="0.01" min={key.includes("seconds") ? 0 : undefined} value={mapping[key]} onChange={(e) => numeric(index, key, e)} /></label>)}
      <label>Curve<select value={mapping.curve} onChange={(e) => updateMapping(index, { curve: e.target.value as ModulationMapping["curve"] })}><option value="linear">Linear</option><option value="exponential">Exponential</option></select></label><label>Polarity<select value={mapping.polarity} onChange={(e) => updateMapping(index, { polarity: e.target.value as ModulationMapping["polarity"] })}><option value="normal">Normal</option><option value="inverted">Inverted</option></select></label><label>Combine<select value={mapping.combine ?? "replace"} onChange={(e) => updateMapping(index, { combine: e.target.value as ModulationMapping["combine"] })}><option value="replace">Replace</option><option value="multiply">Multiply</option><option value="add">Add</option></select></label></div>
      <button className="danger" onClick={() => onChange({ ...project, modulation_mappings: project.modulation_mappings.filter((_, at) => at !== index) })}>Remove mapping</button>
    </details>; })}
  </>;
}
