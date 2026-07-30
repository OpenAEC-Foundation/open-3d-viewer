/**
 * Viewer Slice - Manages 3D viewer state
 *
 * Tracks loaded models, camera mode, section box, and render settings.
 */

// ============================================================================
// Types
// ============================================================================

export interface ModelEntry {
  id: string;
  fileName: string;
  filePath: string;
  format: string;
  vertices: number;
  triangles: number;
  bounds: {
    minX: number; minY: number; minZ: number;
    maxX: number; maxY: number; maxZ: number;
  };
  visible: boolean;
  loadingProgress: number;
  loadingPhase: string;
}

export type CameraMode = 'orbit' | 'walk';
export type RenderMode = 'solid' | 'wireframe' | 'points' | 'hiddenline';
export type ProjectionMode = 'perspective' | 'orthographic';
export type MeasureMode = 'off' | 'point' | 'face';

export interface MeasurePoint { x: number; y: number; z: number }

export interface Measurement {
  id: string;
  type: 'point' | 'face';
  points: MeasurePoint[];
  distance: number;
}

// ============================================================================
// State Interface
// ============================================================================

export interface ViewerState {
  models: ModelEntry[];
  activeModelId: string | null;
  cameraMode: CameraMode;
  renderMode: RenderMode;
  projectionMode: ProjectionMode;
  walkSpeed: number;
  sectionBoxEnabled: boolean;
  sectionCapsEnabled: boolean;
  sectionBoxBounds: {
    minX: number; minY: number; minZ: number;
    maxX: number; maxY: number; maxZ: number;
  } | null;
  showGrid: boolean;
  ambientLightEnabled: boolean;
  directionalLightEnabled: boolean;
  /** Renderer type detected at runtime */
  rendererType: 'webgpu' | 'webgl';
  /** Current FPS */
  fps: number;
  /** Currently selected object info */
  selectedObject: {
    name: string;
    triangles: number;
    sizeX: number; sizeY: number; sizeZ: number;
    centerX: number; centerY: number; centerZ: number;
  } | null;
  /** Scene layers (definition groups) */
  layers: { name: string; visible: boolean; triangles: number }[];
  /** Undo stack for layer visibility (snapshots of visible flags) */
  layerVisibilityHistory: boolean[][];
  /** Redo stack for layer visibility */
  layerVisibilityFuture: boolean[][];
  /** Measurement mode */
  measureMode: MeasureMode;
  /** Active measurement points (building current measurement) */
  measurePoints: MeasurePoint[];
  /** Completed measurements */
  measurements: Measurement[];
}

// ============================================================================
// Actions Interface
// ============================================================================

export interface ViewerActions {
  addModel: (entry: ModelEntry) => void;
  removeModel: (id: string) => void;
  setActiveModelId: (id: string | null) => void;
  setModelVisible: (id: string, visible: boolean) => void;
  updateModelProgress: (id: string, progress: number, phase: string) => void;
  updateModelStats: (id: string, vertices: number, triangles: number, bounds: ModelEntry['bounds']) => void;
  setCameraMode: (mode: CameraMode) => void;
  setRenderMode: (mode: RenderMode) => void;
  setProjectionMode: (mode: ProjectionMode) => void;
  setWalkSpeed: (speed: number) => void;
  setSectionBoxEnabled: (enabled: boolean) => void;
  setSectionCapsEnabled: (enabled: boolean) => void;
  setSectionBoxBounds: (bounds: ViewerState['sectionBoxBounds']) => void;
  setShowGrid: (show: boolean) => void;
  setAmbientLightEnabled: (enabled: boolean) => void;
  setDirectionalLightEnabled: (enabled: boolean) => void;
  setRendererType: (type: 'webgpu' | 'webgl') => void;
  setFps: (fps: number) => void;
  setSelectedObject: (obj: ViewerState['selectedObject']) => void;
  setLayers: (layers: ViewerState['layers']) => void;
  setLayerVisible: (name: string, visible: boolean) => void;
  setMultipleLayersVisible: (updates: { name: string; visible: boolean }[]) => void;
  undoLayerVisibility: () => void;
  redoLayerVisibility: () => void;
  clearModels: () => void;
  setMeasureMode: (mode: MeasureMode) => void;
  addMeasurePoint: (point: MeasurePoint) => void;
  clearMeasurePoints: () => void;
  addMeasurement: (m: Measurement) => void;
  removeMeasurement: (id: string) => void;
  clearMeasurements: () => void;
}

export type ViewerSlice = ViewerState & ViewerActions;

// ============================================================================
// Initial State
// ============================================================================

export const initialViewerState: ViewerState = {
  models: [],
  activeModelId: null,
  cameraMode: 'orbit',
  renderMode: 'solid',
  projectionMode: 'perspective',
  walkSpeed: 5,
  sectionBoxEnabled: false,
  sectionCapsEnabled: true,
  sectionBoxBounds: null,
  showGrid: false,
  ambientLightEnabled: true,
  directionalLightEnabled: true,
  rendererType: 'webgl',
  fps: 0,
  selectedObject: null,
  layers: [],
  layerVisibilityHistory: [],
  layerVisibilityFuture: [],
  measureMode: 'off',
  measurePoints: [],
  measurements: [],
};

// ============================================================================
// Slice Creator
// ============================================================================

interface FullStore extends ViewerState {}

export const createViewerSlice = (
  set: (fn: (state: FullStore) => void) => void,
  _get: () => FullStore
): ViewerActions => ({
  addModel: (entry: ModelEntry) => {
    set((s) => {
      s.models.push(entry);
      if (!s.activeModelId) {
        s.activeModelId = entry.id;
      }
    });
  },

  removeModel: (id: string) => {
    set((s) => {
      s.models = s.models.filter((m) => m.id !== id);
      if (s.activeModelId === id) {
        s.activeModelId = s.models.length > 0 ? s.models[0].id : null;
      }
    });
  },

  setActiveModelId: (id: string | null) => {
    set((s) => { s.activeModelId = id; });
  },

  setModelVisible: (id: string, visible: boolean) => {
    set((s) => {
      const m = s.models.find((m) => m.id === id);
      if (m) m.visible = visible;
    });
  },

  updateModelProgress: (id: string, progress: number, phase: string) => {
    set((s) => {
      const m = s.models.find((m) => m.id === id);
      if (m) {
        m.loadingProgress = progress;
        m.loadingPhase = phase;
      }
    });
  },

  updateModelStats: (id: string, vertices: number, triangles: number, bounds: ModelEntry['bounds']) => {
    set((s) => {
      const m = s.models.find((m) => m.id === id);
      if (m) {
        m.vertices = vertices;
        m.triangles = triangles;
        m.bounds = bounds;
      }
    });
  },

  setCameraMode: (mode: CameraMode) => {
    set((s) => { s.cameraMode = mode; });
  },

  setRenderMode: (mode: RenderMode) => {
    set((s) => { s.renderMode = mode; });
  },

  setProjectionMode: (mode: ProjectionMode) => {
    set((s) => { s.projectionMode = mode; });
  },

  setWalkSpeed: (speed: number) => {
    set((s) => { s.walkSpeed = Math.max(0.5, Math.min(50, speed)); });
  },

  setSectionBoxEnabled: (enabled: boolean) => {
    set((s) => { s.sectionBoxEnabled = enabled; });
  },

  setSectionCapsEnabled: (enabled: boolean) => {
    set((s) => { s.sectionCapsEnabled = enabled; });
  },

  setSectionBoxBounds: (bounds: ViewerState['sectionBoxBounds']) => {
    set((s) => { s.sectionBoxBounds = bounds; });
  },

  setShowGrid: (show: boolean) => {
    set((s) => { s.showGrid = show; });
  },

  setAmbientLightEnabled: (enabled: boolean) => {
    set((s) => { s.ambientLightEnabled = enabled; });
  },

  setDirectionalLightEnabled: (enabled: boolean) => {
    set((s) => { s.directionalLightEnabled = enabled; });
  },

  setRendererType: (type: 'webgpu' | 'webgl') => {
    set((s) => { s.rendererType = type; });
  },

  setFps: (fps: number) => {
    set((s) => { s.fps = fps; });
  },

  setSelectedObject: (obj: ViewerState['selectedObject']) => {
    set((s) => { s.selectedObject = obj; });
  },

  setLayers: (layers: ViewerState['layers']) => {
    set((s) => { s.layers = layers; });
  },

  setLayerVisible: (name: string, visible: boolean) => {
    set((s) => {
      const layer = s.layers.find((l) => l.name === name);
      if (!layer || layer.visible === visible) return;
      // Push current state to undo stack
      s.layerVisibilityHistory.push(s.layers.map((l) => l.visible));
      if (s.layerVisibilityHistory.length > 50) s.layerVisibilityHistory.shift();
      s.layerVisibilityFuture = [];
      layer.visible = visible;
    });
  },

  setMultipleLayersVisible: (updates: { name: string; visible: boolean }[]) => {
    set((s) => {
      // Check if anything actually changes
      const hasChange = updates.some((u) => {
        const layer = s.layers.find((l) => l.name === u.name);
        return layer && layer.visible !== u.visible;
      });
      if (!hasChange) return;
      // Push current state as single undo step
      s.layerVisibilityHistory.push(s.layers.map((l) => l.visible));
      if (s.layerVisibilityHistory.length > 50) s.layerVisibilityHistory.shift();
      s.layerVisibilityFuture = [];
      for (const u of updates) {
        const layer = s.layers.find((l) => l.name === u.name);
        if (layer) layer.visible = u.visible;
      }
    });
  },

  undoLayerVisibility: () => {
    set((s) => {
      if (s.layerVisibilityHistory.length === 0) return;
      const snapshot = s.layerVisibilityHistory.pop()!;
      // Save current state to redo stack
      s.layerVisibilityFuture.push(s.layers.map((l) => l.visible));
      // Restore visibility from snapshot
      for (let i = 0; i < s.layers.length && i < snapshot.length; i++) {
        s.layers[i].visible = snapshot[i];
      }
    });
  },

  redoLayerVisibility: () => {
    set((s) => {
      if (s.layerVisibilityFuture.length === 0) return;
      const snapshot = s.layerVisibilityFuture.pop()!;
      // Save current state to undo stack
      s.layerVisibilityHistory.push(s.layers.map((l) => l.visible));
      // Restore visibility from snapshot
      for (let i = 0; i < s.layers.length && i < snapshot.length; i++) {
        s.layers[i].visible = snapshot[i];
      }
    });
  },

  clearModels: () => {
    set((s) => {
      s.models = [];
      s.activeModelId = null;
      s.layers = [];
      s.layerVisibilityHistory = [];
      s.layerVisibilityFuture = [];
      s.measureMode = 'off';
      s.measurePoints = [];
      s.measurements = [];
    });
    // Notify 3D viewer to clear scene
    window.dispatchEvent(new CustomEvent('open3d:clear-scene'));
  },

  setMeasureMode: (mode: MeasureMode) => {
    set((s) => {
      s.measureMode = mode;
      s.measurePoints = [];
    });
  },

  addMeasurePoint: (point: MeasurePoint) => {
    set((s) => { s.measurePoints.push(point); });
  },

  clearMeasurePoints: () => {
    set((s) => { s.measurePoints = []; });
  },

  addMeasurement: (m: Measurement) => {
    set((s) => { s.measurements.push(m); });
  },

  removeMeasurement: (id: string) => {
    set((s) => { s.measurements = s.measurements.filter((m) => m.id !== id); });
  },

  clearMeasurements: () => {
    set((s) => { s.measurements = []; });
  },
});
