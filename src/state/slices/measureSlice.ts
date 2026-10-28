/**
 * Measure Slice - Manages the measure tools and the takeoff (uittrekstaat)
 *
 * Measurements themselves live in the engine (MeasureManager); this slice
 * holds the tool settings, a summary of the placed measurements (for totals)
 * and the saved takeoff rows. Takeoff rows are kept for the session only.
 */

import {
  DEFAULT_UNIT_SETTINGS,
  type MeasureCategory,
  type UnitSettings,
} from '../../engine/measure/measureFormat';
import type { WeightSettings } from '../../engine/measure/measureObjects';

// ============================================================================
// Types
// ============================================================================

export type MeasureTool =
  | 'off'
  // Length
  | 'distance' | 'perpendicular' | 'point'
  // Area
  | 'area' | 'polygon' | 'areaTotal'
  // Volume & weight
  | 'volume' | 'weight'
  // Angle
  | 'angle' | 'angle3'
  // Counting
  | 'count' | 'countEntity';

export const TOOL_CATEGORY: Record<Exclude<MeasureTool, 'off'>, MeasureCategory> = {
  distance: 'length',
  perpendicular: 'length',
  point: 'length',
  area: 'area',
  polygon: 'area',
  areaTotal: 'area',
  volume: 'volume',
  weight: 'weight',
  angle: 'angle',
  angle3: 'angle',
  count: 'count',
  countEntity: 'count',
};

/** Snap targets for the measure tools */
export interface MeasureSnapSettings {
  vertex: boolean;
  midpoint: boolean;
  edge: boolean;
  face: boolean;
}

/** A placed measurement, as reported by the engine */
export interface MeasurementSummary {
  id: number;
  category: MeasureCategory;
  method: Exclude<MeasureTool, 'off'>;
  /** SI value: m, m², m³, kg, rad or 1 for a counted item */
  value: number;
  objectName?: string;
  material?: string;
  /** Extra info, e.g. the counted entity kind or the density used */
  detail?: string;
  /** Already saved to the takeoff */
  saved: boolean;
}

/** A row of the takeoff table */
export interface TakeoffRow {
  id: string;
  name: string;
  group: string;
  category: MeasureCategory;
  method: Exclude<MeasureTool, 'off'>;
  /** SI value (for counts: the number of items) */
  value: number;
  objectName?: string;
  material?: string;
  detail?: string;
}

// ============================================================================
// State & Actions
// ============================================================================

export interface MeasureState {
  measureTool: MeasureTool;
  /** Last used method per category (what the category button starts) */
  measureLastMethod: Record<MeasureCategory, Exclude<MeasureTool, 'off'>>;
  measureSnap: MeasureSnapSettings;
  /** Keep consecutive measurements and show their total */
  measureMulti: boolean;
  /** Show the dX / dY / dZ legs of distances */
  measureShowComponents: boolean;
  measureUnits: UnitSettings;
  weightSettings: WeightSettings;
  /** Picks made in the measurement in progress */
  measureStage: number;
  measurements: MeasurementSummary[];
  takeoffRows: TakeoffRow[];
  takeoffGroupCounter: number;
  takeoffOpen: boolean;
  materialsDialogOpen: boolean;
}

export interface MeasureActions {
  setMeasureTool: (tool: MeasureTool) => void;
  setMeasureSnap: (snap: Partial<MeasureSnapSettings>) => void;
  setMeasureMulti: (multi: boolean) => void;
  setMeasureShowComponents: (show: boolean) => void;
  setMeasureUnits: (units: Partial<UnitSettings>) => void;
  setWeightSettings: (settings: Partial<WeightSettings>) => void;
  setMaterialDensity: (material: string, density: number | null) => void;
  setMeasureStage: (stage: number) => void;
  setMeasurements: (measurements: MeasurementSummary[]) => void;
  /** Turn the unsaved measurements into takeoff rows; returns the saved measurement ids */
  saveMeasurementsToTakeoff: (group: string) => number[];
  updateTakeoffRow: (id: string, changes: Partial<Pick<TakeoffRow, 'name' | 'group'>>) => void;
  removeTakeoffRow: (id: string) => void;
  clearTakeoff: () => void;
  setTakeoffOpen: (open: boolean) => void;
  setMaterialsDialogOpen: (open: boolean) => void;
}

export type MeasureSlice = MeasureState & MeasureActions;

export const initialMeasureState: MeasureState = {
  measureTool: 'off',
  measureLastMethod: {
    length: 'perpendicular',
    area: 'area',
    volume: 'volume',
    weight: 'weight',
    angle: 'angle',
    count: 'count',
  },
  measureSnap: { vertex: true, midpoint: true, edge: true, face: true },
  measureMulti: false,
  measureShowComponents: true,
  measureUnits: DEFAULT_UNIT_SETTINGS,
  weightSettings: { materialDensities: {}, defaultDensity: 2400 },
  measureStage: 0,
  measurements: [],
  takeoffRows: [],
  takeoffGroupCounter: 0,
  takeoffOpen: false,
  materialsDialogOpen: false,
};

// ============================================================================
// Slice Creator
// ============================================================================

interface FullStore extends MeasureState {
  sectionPlaneMode: 'off' | 'picking' | 'placed';
}

let rowCounter = 0;

export const createMeasureSlice = (
  set: (fn: (state: FullStore) => void) => void,
  get: () => FullStore,
): MeasureActions => ({
  setMeasureTool: (tool) => {
    set((s) => {
      s.measureTool = tool;
      if (tool !== 'off') s.measureLastMethod[TOOL_CATEGORY[tool]] = tool;
    });
  },

  setMeasureSnap: (snap) => {
    set((s) => { s.measureSnap = { ...s.measureSnap, ...snap }; });
  },

  setMeasureMulti: (multi) => {
    set((s) => { s.measureMulti = multi; });
  },

  setMeasureShowComponents: (show) => {
    set((s) => { s.measureShowComponents = show; });
  },

  setMeasureUnits: (units) => {
    set((s) => { s.measureUnits = { ...s.measureUnits, ...units }; });
  },

  setWeightSettings: (settings) => {
    set((s) => { s.weightSettings = { ...s.weightSettings, ...settings }; });
  },

  setMaterialDensity: (material, density) => {
    set((s) => {
      const next = { ...s.weightSettings.materialDensities };
      if (density === null) delete next[material];
      else next[material] = Math.max(0, density);
      s.weightSettings = { ...s.weightSettings, materialDensities: next };
    });
  },

  setMeasureStage: (stage) => {
    set((s) => { s.measureStage = stage; });
  },

  setMeasurements: (measurements) => {
    set((s) => { s.measurements = measurements; });
  },

  saveMeasurementsToTakeoff: (group) => {
    const unsaved = get().measurements.filter((m) => !m.saved);
    if (unsaved.length === 0) return [];

    const rows: TakeoffRow[] = [];
    // Counted items become one row per object / entity kind
    const counts = new Map<string, TakeoffRow>();
    for (const m of unsaved) {
      if (m.category === 'count') {
        const key = `${m.method}|${m.objectName ?? ''}|${m.detail ?? ''}`;
        const row = counts.get(key);
        if (row) {
          row.value += 1;
        } else {
          const newRow: TakeoffRow = {
            id: `row-${++rowCounter}`, name: '', group,
            category: 'count', method: m.method, value: 1,
            objectName: m.objectName, material: m.material, detail: m.detail,
          };
          counts.set(key, newRow);
          rows.push(newRow);
        }
        continue;
      }
      rows.push({
        id: `row-${++rowCounter}`, name: '', group,
        category: m.category, method: m.method, value: m.value,
        objectName: m.objectName, material: m.material, detail: m.detail,
      });
    }

    set((s) => {
      s.takeoffRows.push(...rows);
      s.takeoffGroupCounter += 1;
    });
    return unsaved.map((m) => m.id);
  },

  updateTakeoffRow: (id, changes) => {
    set((s) => {
      const row = s.takeoffRows.find((r) => r.id === id);
      if (row) Object.assign(row, changes);
    });
  },

  removeTakeoffRow: (id) => {
    set((s) => { s.takeoffRows = s.takeoffRows.filter((r) => r.id !== id); });
  },

  clearTakeoff: () => {
    set((s) => { s.takeoffRows = []; });
  },

  setTakeoffOpen: (open) => {
    set((s) => { s.takeoffOpen = open; });
  },

  setMaterialsDialogOpen: (open) => {
    set((s) => { s.materialsDialogOpen = open; });
  },
});
