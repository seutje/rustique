import { invoke } from "@tauri-apps/api/core";
import type { EditorProject, ProjectData } from "./store";
export interface GpuSummary { adapter: string; backend: string; deviceType: string; driver: string; }
export interface ActiveModulation { target: string; sourceValue: number; outputValue: number; }
export interface PreviewStats { frameIndex: number; framesPerSecond: number; frameTimeMs: number; particleCount: number; gpuComputeMs: number | null; gpuRenderMs: number | null; playing: boolean; activeModulations: ActiveModulation[]; }
export interface TimelineAudio { durationSeconds: number; waveform: [number, number][]; transientTimes: number[]; beatTimes: number[]; }
export interface PreviewJob { id: number; kind: "still" | "slice"; state: string; progress: number; outputPath: string | null; error: string | null; }
export interface ProductionSettings { outputPath: string; width: number; height: number; fps: number; supersampling: number; motionBlurSamples: number; substeps: number; codec: "h264" | "hevc"; }
export interface ProductionStatus { state: string; completedFrames: number; totalFrames: number; etaSeconds: number | null; outputPath: string | null; manifestPath: string | null; error: string | null; }
export const queryGpuInfo = () => invoke<GpuSummary>("gpu_info");
export const loadProject = (path: string) => invoke<EditorProject>("load_project", { path });
export const updateProject = (project: ProjectData) => invoke<ProjectData>("update_project", { project });
export const saveProject = (path: string, project: ProjectData) => invoke<void>("save_project", { path, project });
export const loadPreviewAudio = (path: string) => invoke<TimelineAudio>("load_preview_audio", { path });
export const resizeViewport = (x: number, y: number, width: number, height: number) => invoke<void>("viewport_resize", { x, y, width, height });
export const setViewportVisible = (visible: boolean) => invoke<void>("viewport_visible", { visible });
export const setPreviewPlaying = (playing: boolean) => invoke<void>("preview_play", { playing });
export const seekPreview = (frame: number) => invoke<void>("preview_seek", { frame });
export const resetPreview = () => invoke<void>("preview_reset");
export const setPreviewQuality = (quality: string) => invoke<void>("preview_quality", { quality });
export const queryPreviewStats = () => invoke<PreviewStats>("preview_stats");
export const enqueuePreview = (kind: "still" | "slice", project: ProjectData, audioPath: string, startFrame: number, endFrame: number, useFinalSettings: boolean) => invoke<number>("enqueue_preview", { kind, project, audioPath, startFrame, endFrame, useFinalSettings });
export const queryPreviewJobs = () => invoke<PreviewJob[]>("preview_jobs");
export const clearPreviews = () => invoke<void>("clear_previews");
export const openPreview = (path: string) => invoke<void>("open_preview", { path });
export const enqueueProductionRender = (project: ProjectData, audioPath: string, settings: ProductionSettings) => invoke<void>("enqueue_production_render", { project, audioPath, settings });
export const queryProductionStatus = () => invoke<ProductionStatus>("production_render_status");
