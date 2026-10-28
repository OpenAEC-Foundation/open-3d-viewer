import i18next from '../../i18n/config';

/**
 * Number formatting for measurements. The scene is in meters; values are
 * stored in SI units (m, m², m³, kg, rad) and converted to the display units
 * and number of decimals the user picked.
 */

export type MeasureCategory = 'length' | 'area' | 'volume' | 'weight' | 'angle' | 'count';

export type LengthUnit = 'mm' | 'cm' | 'm';
export type AreaUnit = 'mm2' | 'cm2' | 'm2';
export type VolumeUnit = 'cm3' | 'dm3' | 'm3';
export type WeightUnit = 'kg' | 't';

export interface UnitSettings {
  length: { unit: LengthUnit; decimals: number };
  area: { unit: AreaUnit; decimals: number };
  volume: { unit: VolumeUnit; decimals: number };
  weight: { unit: WeightUnit; decimals: number };
  angle: { decimals: number };
}

export const DEFAULT_UNIT_SETTINGS: UnitSettings = {
  length: { unit: 'mm', decimals: 0 },
  area: { unit: 'm2', decimals: 2 },
  volume: { unit: 'm3', decimals: 3 },
  weight: { unit: 'kg', decimals: 1 },
  angle: { decimals: 1 },
};

export const UNIT_FACTORS = {
  length: { mm: 1000, cm: 100, m: 1 },
  area: { mm2: 1e6, cm2: 1e4, m2: 1 },
  volume: { cm3: 1e6, dm3: 1e3, m3: 1 },
  weight: { kg: 1, t: 0.001 },
} as const;

export const UNIT_LABELS: Record<string, string> = {
  mm: 'mm', cm: 'cm', m: 'm',
  mm2: 'mm²', cm2: 'cm²', m2: 'm²',
  cm3: 'cm³', dm3: 'dm³', m3: 'm³',
  kg: 'kg', t: 't',
};

const formatters = new Map<string, Intl.NumberFormat>();

export function formatNumber(value: number, decimals: number): string {
  const lang = i18next.language || 'nl';
  const key = `${lang}:${decimals}`;
  let nf = formatters.get(key);
  if (!nf) {
    nf = new Intl.NumberFormat(lang, { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
    formatters.set(key, nf);
  }
  // Avoid "-0"
  const rounded = Math.abs(value) < 0.5 * 10 ** -decimals ? 0 : value;
  return nf.format(rounded);
}

/**
 * Convert an SI value to the display unit: returns the number (rounded to the
 * chosen decimals), its unit label and the decimals used.
 */
export function toDisplay(
  category: MeasureCategory, value: number, settings: UnitSettings,
): { value: number; unit: string; decimals: number } {
  switch (category) {
    case 'length':
      return { value: value * UNIT_FACTORS.length[settings.length.unit], unit: UNIT_LABELS[settings.length.unit], decimals: settings.length.decimals };
    case 'area':
      return { value: value * UNIT_FACTORS.area[settings.area.unit], unit: UNIT_LABELS[settings.area.unit], decimals: settings.area.decimals };
    case 'volume':
      return { value: value * UNIT_FACTORS.volume[settings.volume.unit], unit: UNIT_LABELS[settings.volume.unit], decimals: settings.volume.decimals };
    case 'weight':
      return { value: value * UNIT_FACTORS.weight[settings.weight.unit], unit: UNIT_LABELS[settings.weight.unit], decimals: settings.weight.decimals };
    case 'angle':
      return { value: value * 180 / Math.PI, unit: '°', decimals: settings.angle.decimals };
    case 'count':
      return { value, unit: '', decimals: 0 };
  }
}

/** Formatted value with unit, e.g. "1.234 mm" or "12,50 m²" */
export function formatValue(category: MeasureCategory, value: number, settings: UnitSettings): string {
  const d = toDisplay(category, value, settings);
  const num = formatNumber(d.value, d.decimals);
  if (category === 'angle') return `${num}°`;
  return d.unit ? `${num} ${d.unit}` : num;
}

/** Length value without the unit (coordinates show the unit once) */
export function formatCoordinate(meters: number, settings: UnitSettings): string {
  return formatNumber(meters * UNIT_FACTORS.length[settings.length.unit], settings.length.decimals);
}

export function formatDensity(kgPerM3: number): string {
  return `${formatNumber(kgPerM3, 0)} kg/m³`;
}
