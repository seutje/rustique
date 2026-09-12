import { create } from "zustand";

export interface ProjectSummary {
  path: string;
  engineVersion: string;
  durationSeconds: number;
  fps: number;
  particleCount: number;
}

interface EditorState {
  project: ProjectSummary | null;
  selectedPreset: string | null;
  setProject: (project: ProjectSummary) => void;
  setSelectedPreset: (preset: string) => void;
}

export const useEditorStore = create<EditorState>((set) => ({
  project: null,
  selectedPreset: null,
  setProject: (project) => set({ project }),
  setSelectedPreset: (selectedPreset) => set({ selectedPreset }),
}));
