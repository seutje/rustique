import { invoke } from "@tauri-apps/api/core";
import type { ProjectSummary } from "./store";

export interface GpuSummary { adapter: string; backend: string; deviceType: string; driver: string; }
export interface PreviewStats {
  frameIndex: number; framesPerSecond: number; frameTimeMs: number;
  particleCount: number; gpuComputeMs: number | null; gpuRenderMs: number | null; playing: boolean;
}

export const queryGpuInfo = () => invoke<GpuSummary>("gpu_info");
export const loadProject = (path: string) => invoke<ProjectSummary>("load_project", { path });
export const loadPreviewAudio = (path: string) => invoke<void>("load_preview_audio", { path });
export const resizeViewport = (x: number, y: number, width: number, height: number) => invoke<void>("viewport_resize", { x, y, width, height });
export const setPreviewPlaying = (playing: boolean) => invoke<void>("preview_play", { playing });
export const seekPreview = (frame: number) => invoke<void>("preview_seek", { frame });
export const resetPreview = () => invoke<void>("preview_reset");
export const setPreviewQuality = (quality: string) => invoke<void>("preview_quality", { quality });
export const queryPreviewStats = () => invoke<PreviewStats>("preview_stats");
