import { invoke } from "@tauri-apps/api/core";
import type { ProjectSummary } from "./store";

export interface GpuSummary {
  adapter: string;
  backend: string;
  deviceType: string;
  driver: string;
}

export const queryGpuInfo = () => invoke<GpuSummary>("gpu_info");

export const loadProject = (path: string) =>
  invoke<ProjectSummary>("load_project", { path });
