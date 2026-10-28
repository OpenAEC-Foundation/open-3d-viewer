/**
 * Loading Slice - Tracks file loading progress
 */

export interface FileLoadingEntry {
  id: string;
  fileName: string;
  progress: number;
  phase: string;
  error: string | null;
  /** When loading started and when progress last changed (ms; the loading card's time and busy state) */
  startedAt: number;
  updatedAt: number;
}

export interface LoadingState {
  loadingFiles: FileLoadingEntry[];
}

export interface LoadingActions {
  addLoadingFile: (id: string, fileName: string) => void;
  updateLoadingFile: (id: string, progress: number, phase: string) => void;
  setLoadingFileError: (id: string, error: string) => void;
  removeLoadingFile: (id: string) => void;
}

export type LoadingSlice = LoadingState & LoadingActions;

export const initialLoadingState: LoadingState = {
  loadingFiles: [],
};

interface FullStore extends LoadingState {}

export const createLoadingSlice = (
  set: (fn: (state: FullStore) => void) => void,
  _get: () => FullStore
): LoadingActions => ({
  addLoadingFile: (id: string, fileName: string) => {
    set((s) => {
      const now = Date.now();
      s.loadingFiles.push({ id, fileName, progress: 0, phase: 'Starting...', error: null, startedAt: now, updatedAt: now });
    });
  },

  updateLoadingFile: (id: string, progress: number, phase: string) => {
    set((s) => {
      const f = s.loadingFiles.find((f) => f.id === id);
      if (f) {
        if (f.progress !== progress) f.updatedAt = Date.now();
        f.progress = progress;
        f.phase = phase;
      }
    });
  },

  setLoadingFileError: (id: string, error: string) => {
    set((s) => {
      const f = s.loadingFiles.find((f) => f.id === id);
      if (f) {
        f.error = error;
        f.phase = 'Error';
      }
    });
  },

  removeLoadingFile: (id: string) => {
    set((s) => {
      s.loadingFiles = s.loadingFiles.filter((f) => f.id !== id);
    });
  },
});
