/**
 * App Store — Zustand store for Open 3D Viewer Studio
 *
 * Contains UI theme state + viewer state + loading state.
 */

import { create } from 'zustand';
import { immer } from 'zustand/middleware/immer';

import {
  type ViewerState,
  type ViewerActions,
  initialViewerState,
  createViewerSlice,
  type LoadingState,
  type LoadingActions,
  initialLoadingState,
  createLoadingSlice,
} from './slices';

// ============================================================================
// UI Theme
// ============================================================================

export type UITheme = 'dark' | 'light' | 'blue' | 'highContrast';

export const UI_THEMES: { id: UITheme; label: string }[] = [
  { id: 'dark', label: 'Dark' },
  { id: 'light', label: 'Light' },
  { id: 'blue', label: 'Blue' },
  { id: 'highContrast', label: 'High Contrast' },
];

export type DisplayUnit = 'mm' | 'cm' | 'm';

export interface UIState {
  uiTheme: UITheme;
  rightPanelOpen: boolean;
  displayUnit: DisplayUnit;
}

export interface UIActions {
  setUITheme: (theme: UITheme) => void;
  toggleRightPanel: () => void;
  setDisplayUnit: (unit: DisplayUnit) => void;
}

const initialUIState: UIState = {
  uiTheme: 'light',
  rightPanelOpen: true,
  displayUnit: 'mm',
};

// ============================================================================
// Combined State
// ============================================================================

export type AppState = UIState & UIActions & ViewerState & ViewerActions & LoadingState & LoadingActions;

export const useAppStore = create<AppState>()(
  immer((set, get) => ({
    ...initialUIState,
    ...initialViewerState,
    ...initialLoadingState,

    // UI actions
    setUITheme: (theme: UITheme) => {
      set((s) => { s.uiTheme = theme; });
    },
    toggleRightPanel: () => {
      set((s) => { s.rightPanelOpen = !s.rightPanelOpen; });
    },
    setDisplayUnit: (unit: DisplayUnit) => {
      set((s) => { s.displayUnit = unit; });
    },

    // Viewer actions
    ...createViewerSlice(set as any, get as any),

    // Loading actions
    ...createLoadingSlice(set as any, get as any),
  }))
);
