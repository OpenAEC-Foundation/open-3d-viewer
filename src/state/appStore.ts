/**
 * App Store — Zustand store for Open 3D Viewer Studio
 *
 * Contains UI theme state + viewer state + loading state.
 */

import { create } from 'zustand';
import { immer } from 'zustand/middleware/immer';
import { getPref, setPref } from './preferences';

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

/** OpenAEC theme variants (OpenAEC-style-book themes.css). */
export type UITheme = 'forge' | 'light' | 'openaec' | 'blueprint' | 'contrast';

export const UI_THEMES: { id: UITheme; label: string }[] = [
  { id: 'forge', label: 'Deep Forge' },
  { id: 'light', label: 'Light' },
  { id: 'openaec', label: 'Night Build' },
  { id: 'blueprint', label: 'Blueprint' },
  { id: 'contrast', label: 'High Contrast' },
];

/** Themes of earlier versions, mapped to their OpenAEC counterpart. */
const LEGACY_THEMES: Record<string, UITheme> = { dark: 'forge', blue: 'blueprint', highContrast: 'contrast' };

function toTheme(value: string): UITheme {
  if (UI_THEMES.some((t) => t.id === value)) return value as UITheme;
  return LEGACY_THEMES[value] ?? 'forge';
}

export interface UIState {
  uiTheme: UITheme;
  rightPanelOpen: boolean;
}

export interface UIActions {
  setUITheme: (theme: UITheme) => void;
  toggleRightPanel: () => void;
}

const initialUIState: UIState = {
  uiTheme: 'forge',
  rightPanelOpen: true,
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
      setPref('uiTheme', theme);
    },
    toggleRightPanel: () => {
      set((s) => { s.rightPanelOpen = !s.rightPanelOpen; });
      setPref('rightPanelOpen', get().rightPanelOpen);
    },

    // Viewer actions
    ...createViewerSlice(set as any, get as any),

    // Loading actions
    ...createLoadingSlice(set as any, get as any),
  }))
);

export async function loadPreferences() {
  const [theme, rightPanelOpen, language] = await Promise.all([
    getPref<string>('uiTheme', 'forge'),
    getPref<boolean>('rightPanelOpen', true),
    getPref<string>('language', ''),
  ]);
  useAppStore.setState({ uiTheme: toTheme(theme), rightPanelOpen });
  if (language) {
    const { default: i18next } = await import('../i18n/config');
    i18next.changeLanguage(language);
  }
}
