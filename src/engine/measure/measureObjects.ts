import * as THREE from 'three';

/**
 * Objects and materials for the object-based measure tools (volume, total
 * area, weight, counting).
 *
 * An IFC element can consist of several meshes (one per representation item);
 * they share `userData.expressID` and are treated as one object. Other formats
 * use the mesh itself.
 */

export interface ObjectPart {
  mesh: THREE.Mesh;
  instanceId?: number;
}

export interface ObjectInfo {
  /** Stable key for this object */
  key: string;
  name: string;
  /** IFC class, if any */
  ifcClass?: string;
  /** Material name from the file (dominant material for layered IFC elements) */
  material?: string;
  /** Weight stated in the file (kg), e.g. an IFC NetWeight quantity */
  weight?: number;
  parts: ObjectPart[];
}

/** Model root (the "model-<id>" group) of an object */
function modelRoot(obj: THREE.Object3D): THREE.Object3D {
  let root = obj;
  for (let o: THREE.Object3D | null = obj; o; o = o.parent) {
    if (o.name.startsWith('model-')) return o;
    root = o;
  }
  return root;
}

/** Cache: model root → expressID → meshes */
const elementIndex = new WeakMap<THREE.Object3D, Map<number, THREE.Mesh[]>>();

function elementMeshes(root: THREE.Object3D, expressID: number): THREE.Mesh[] {
  let index = elementIndex.get(root);
  if (!index) {
    index = new Map();
    root.traverse((o) => {
      const id = o.userData?.expressID;
      if (o instanceof THREE.Mesh && typeof id === 'number') {
        const list = index!.get(id);
        if (list) list.push(o); else index!.set(id, [o]);
      }
    });
    elementIndex.set(root, index);
  }
  return index.get(expressID) ?? [];
}

export function materialName(mesh: THREE.Mesh): string | undefined {
  const fromFile = mesh.userData?.material;
  if (typeof fromFile === 'string' && fromFile) return fromFile;
  const mat = Array.isArray(mesh.material) ? mesh.material[0] : mesh.material;
  return mat?.name || undefined;
}

/** The object a hit mesh belongs to. */
export function objectFor(mesh: THREE.Mesh, instanceId?: number): ObjectInfo {
  const ud = mesh.userData ?? {};
  if (typeof ud.expressID === 'number') {
    const root = modelRoot(mesh);
    const meshes = elementMeshes(root, ud.expressID);
    return {
      key: `${root.uuid}:${ud.expressID}`,
      name: mesh.name,
      ifcClass: ud.ifcClass,
      material: materialName(mesh),
      weight: typeof ud.weight === 'number' ? ud.weight : undefined,
      parts: (meshes.length > 0 ? meshes : [mesh]).map((m) => ({ mesh: m })),
    };
  }
  return {
    key: `${mesh.uuid}:${instanceId ?? ''}`,
    name: mesh.name || mesh.parent?.name || '',
    material: materialName(mesh),
    weight: typeof ud.weight === 'number' ? ud.weight : undefined,
    parts: [{ mesh, instanceId }],
  };
}

/** All distinct material names in the given models. */
export function collectMaterials(roots: THREE.Object3D[]): string[] {
  const names = new Set<string>();
  for (const root of roots) {
    root.traverse((o) => {
      if (o instanceof THREE.Mesh) {
        const name = materialName(o);
        if (name) names.add(name);
      }
    });
  }
  return [...names].sort((a, b) => a.localeCompare(b));
}

// ============================================================================
// Densities
// ============================================================================

export interface DensityPreset {
  id: string;
  /** kg/m³ */
  density: number;
  /** Lower-case keywords matched against material names (Dutch and English) */
  keywords: string[];
}

/** Common building materials; more specific presets come first */
export const DENSITY_PRESETS: DensityPreset[] = [
  { id: 'reinforcedConcrete', density: 2500, keywords: ['gewapend', 'reinforced'] },
  { id: 'concrete', density: 2400, keywords: ['beton', 'concrete', 'c20/25', 'c30/37', 'c35/45'] },
  { id: 'stainless', density: 7900, keywords: ['rvs', 'stainless', 'roestvast'] },
  { id: 'steel', density: 7850, keywords: ['staal', 'steel', 's235', 's275', 's355', 'metal'] },
  { id: 'aluminium', density: 2700, keywords: ['aluminium', 'aluminum'] },
  { id: 'sandLime', density: 1850, keywords: ['kalkzandsteen', 'sand-lime', 'sand lime', 'silicate'] },
  { id: 'aeratedConcrete', density: 550, keywords: ['cellenbeton', 'gasbeton', 'aerated', 'ytong'] },
  { id: 'masonry', density: 1900, keywords: ['metselwerk', 'baksteen', 'brick', 'masonry', 'steen', 'stone'] },
  { id: 'naturalStone', density: 2700, keywords: ['natuursteen', 'graniet', 'granite', 'marble', 'marmer'] },
  { id: 'hardwood', density: 750, keywords: ['hardhout', 'hardwood', 'eiken', 'oak', 'azobe', 'meranti'] },
  { id: 'softwood', density: 500, keywords: ['hout', 'wood', 'timber', 'vuren', 'grenen', 'spruce', 'pine', 'clt', 'glulam'] },
  { id: 'glass', density: 2500, keywords: ['glas', 'glass', 'glazing'] },
  { id: 'gypsum', density: 1000, keywords: ['gips', 'gypsum', 'plaster', 'stuc'] },
  { id: 'insulation', density: 30, keywords: ['isolatie', 'insulation', 'wol', 'wool', 'eps', 'xps', 'pir', 'pur'] },
  { id: 'screed', density: 2000, keywords: ['dekvloer', 'screed', 'cement'] },
  { id: 'water', density: 1000, keywords: ['water'] },
];

export function presetForMaterial(material: string | undefined): DensityPreset | undefined {
  if (!material) return undefined;
  const name = material.toLowerCase();
  return DENSITY_PRESETS.find((p) => p.keywords.some((k) => name.includes(k)));
}

export type WeightSource = 'file' | 'user' | 'preset' | 'default';

export interface WeightSettings {
  /** User-set densities per material name (kg/m³) */
  materialDensities: Record<string, number>;
  /** Density used when nothing else is known (kg/m³) */
  defaultDensity: number;
}

/**
 * Density for an object: user setting for its material, then a matching
 * preset, then the default.
 */
export function densityFor(material: string | undefined, settings: WeightSettings): { density: number; source: WeightSource; presetId?: string } {
  if (material && settings.materialDensities[material] !== undefined) {
    return { density: settings.materialDensities[material], source: 'user' };
  }
  const preset = presetForMaterial(material);
  if (preset) return { density: preset.density, source: 'preset', presetId: preset.id };
  return { density: settings.defaultDensity, source: 'default' };
}
