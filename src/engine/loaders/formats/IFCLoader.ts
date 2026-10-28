import * as THREE from 'three';
import webIfcWasmUrl from 'web-ifc/web-ifc.wasm?url';
import type { ThreeJSResult, ProgressCallback } from '../types';
import { finalizeScene } from './sceneUtils';

/**
 * IFCLoader — native IFC (2x3, 4, 4.3) via web-ifc (WASM).
 * Elements are grouped per IFC class (IfcWall, IfcSlab, ...) so each class
 * shows up as a layer; each mesh carries its IFC name, GlobalId, class,
 * material, building storey and (when stated in the file) weight.
 */

let apiPromise: Promise<any> | null = null;

/** web-ifc returns Y-up geometry; IFC itself and this viewer are Z-up */
const Y_UP_TO_Z_UP = new THREE.Matrix4().makeRotationX(Math.PI / 2);

/** Metadata that is not part of the geometry stream */
interface ElementMetadata {
  /** expressID → material names (dominant first) */
  materials: Map<number, string[]>;
  /** expressID → weight in kg (from quantities or properties) */
  weights: Map<number, number>;
}

/** Building storey an element belongs to */
interface StoreyInfo {
  /** expressID of the IfcBuildingStorey */
  id: number;
  name: string;
  /** Elevation as stated in the file (NaN when missing) */
  elevation: number;
}

const WEIGHT_NAME = /weight|gewicht|mass|massa/i;

/** Read a line by expressID or reference; null when it cannot be read */
function ifcLine(api: any, modelID: number, ref: any): any {
  const id = typeof ref === 'number' ? ref : ref?.value;
  if (typeof id !== 'number') return null;
  try { return api.GetLine(modelID, id); } catch { return null; }
}

/** expressIDs of all lines of an IFC type */
function ifcLineIds(api: any, modelID: number, type: number): number[] {
  const vec = api.GetLineIDsWithType(modelID, type);
  const out: number[] = [];
  for (let i = 0; i < vec.size(); i++) out.push(vec.get(i));
  return out;
}

/**
 * Resolve the building storey of every element: directly via
 * IfcRelContainedInSpatialStructure, or through the parent it is aggregated
 * into (stair flight → stair, space → storey) via IfcRelAggregates.
 */
function readStoreys(
  api: any,
  modelID: number,
  types: { storeys: number; contained: number; aggregates: number },
): (expressID: number) => StoreyInfo | undefined {
  const storeys = new Map<number, StoreyInfo>();
  for (const id of ifcLineIds(api, modelID, types.storeys)) {
    const s = ifcLine(api, modelID, id);
    const elevation = s?.Elevation?.value;
    storeys.set(id, {
      id,
      name: s?.Name?.value || s?.LongName?.value || `#${id}`,
      elevation: typeof elevation === 'number' ? elevation : NaN,
    });
  }
  if (storeys.size === 0) return () => undefined;

  // child → parent; containment wins over aggregation
  const parentOf = new Map<number, number>();
  for (const relId of ifcLineIds(api, modelID, types.aggregates)) {
    const rel = ifcLine(api, modelID, relId);
    const parent = rel?.RelatingObject?.value;
    if (typeof parent !== 'number') continue;
    for (const obj of rel.RelatedObjects ?? []) {
      if (typeof obj?.value === 'number') parentOf.set(obj.value, parent);
    }
  }
  for (const relId of ifcLineIds(api, modelID, types.contained)) {
    const rel = ifcLine(api, modelID, relId);
    const structure = rel?.RelatingStructure?.value;
    if (typeof structure !== 'number') continue;
    for (const obj of rel.RelatedElements ?? []) {
      if (typeof obj?.value === 'number') parentOf.set(obj.value, structure);
    }
  }

  return (expressID: number) => {
    let id: number | undefined = expressID;
    for (let depth = 0; depth < 16 && id !== undefined; depth++) {
      const storey = storeys.get(id);
      if (storey) return storey;
      id = parentOf.get(id);
    }
    return undefined;
  };
}

/**
 * Read material associations and weights of all elements:
 * IfcRelAssociatesMaterial (materials, layer sets, profile sets, constituents)
 * and IfcRelDefinesByProperties (IfcQuantityWeight, or a "Weight" property).
 */
function readElementMetadata(api: any, modelID: number, types: { rels: number; props: number }): ElementMetadata {
  const materials = new Map<number, string[]>();
  const weights = new Map<number, number>();
  const line = (ref: any) => ifcLine(api, modelID, ref);
  const ids = (type: number): number[] => ifcLineIds(api, modelID, type);

  // Material definitions nest (usage → set → layer → material); walk them generically.
  // Layers are sorted thickest first so the dominant material comes first.
  const materialNames = (ref: any, depth = 0): string[] => {
    const l = line(ref);
    if (!l || depth > 6) return [];
    const list = l.MaterialLayers ?? l.Materials ?? l.MaterialConstituents ?? l.MaterialProfiles;
    if (Array.isArray(list)) {
      const items = list.map((r: any) => ({ l: line(r), r }));
      items.sort((a: any, b: any) => (b.l?.LayerThickness?.value ?? 0) - (a.l?.LayerThickness?.value ?? 0));
      return items.flatMap((it: any) => materialNames(it.r, depth + 1));
    }
    const inner = l.Material ?? l.ForLayerSet ?? l.ForProfileSet;
    if (inner) return materialNames(inner, depth + 1);
    const name = l.Name?.value;
    return typeof name === 'string' && name ? [name] : [];
  };

  for (const relId of ids(types.rels)) {
    const rel = line(relId);
    if (!rel) continue;
    const names = [...new Set(materialNames(rel.RelatingMaterial))];
    if (names.length === 0) continue;
    for (const obj of rel.RelatedObjects ?? []) {
      if (typeof obj?.value === 'number' && !materials.has(obj.value)) materials.set(obj.value, names);
    }
  }

  for (const relId of ids(types.props)) {
    const rel = line(relId);
    const def = rel && line(rel.RelatingPropertyDefinition);
    if (!def) continue;
    let weight: number | undefined;
    for (const ref of def.Quantities ?? []) {
      const q = line(ref);
      const value = q?.WeightValue?.value;
      if (typeof value === 'number') {
        // Prefer the net weight over gross / other weights
        if (weight === undefined || /net/i.test(q.Name?.value ?? '')) weight = value;
      }
    }
    if (weight === undefined) {
      for (const ref of def.HasProperties ?? []) {
        const p = line(ref);
        const value = p?.NominalValue?.value;
        if (typeof value === 'number' && WEIGHT_NAME.test(p.Name?.value ?? '')) { weight = value; break; }
      }
    }
    if (weight === undefined || !(weight > 0)) continue;
    for (const obj of rel.RelatedObjects ?? []) {
      if (typeof obj?.value === 'number' && !weights.has(obj.value)) weights.set(obj.value, weight);
    }
  }

  return { materials, weights };
}

function getApi(): Promise<any> {
  if (!apiPromise) {
    apiPromise = import('web-ifc').then(async ({ IfcAPI }) => {
      const api = new IfcAPI();
      await api.Init((path: string) => (path.endsWith('.wasm') ? webIfcWasmUrl : path), true);
      return api;
    });
    apiPromise.catch(() => { apiPromise = null; });
  }
  return apiPromise;
}

function toClassName(raw: string, line: any): string {
  // web-ifc reports upper-case type names (IFCWALLSTANDARDCASE); the line's class
  // has the proper casing unless a minifier renamed it, so verify before using it
  const ctorName: string | undefined = line?.constructor?.name;
  if (ctorName && ctorName.toUpperCase() === raw.toUpperCase()) return ctorName;
  const lower = raw.toLowerCase();
  return lower.startsWith('ifc') ? 'Ifc' + lower.charAt(3).toUpperCase() + lower.slice(4) : raw;
}

export async function loadIFC(
  data: ArrayBuffer,
  fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.25, 'Loading IFC engine...');
  const api = await getApi();
  const {
    IFCRELASSOCIATESMATERIAL, IFCRELDEFINESBYPROPERTIES,
    IFCBUILDINGSTOREY, IFCRELCONTAINEDINSPATIALSTRUCTURE, IFCRELAGGREGATES,
  } = await import('web-ifc');

  onProgress?.(0.35, 'Parsing IFC...');
  const modelID: number = api.OpenModel(new Uint8Array(data), { COORDINATE_TO_ORIGIN: true });
  if (modelID < 0) throw new Error('IFC: web-ifc could not open this file');

  const root = new THREE.Group();
  root.name = fileName;
  const classGroups = new Map<string, THREE.Group>();
  const materials = new Map<string, THREE.MeshStandardMaterial>();
  const matrix = new THREE.Matrix4();

  const getMaterial = (c: { x: number; y: number; z: number; w: number }) => {
    const key = `${c.x.toFixed(3)},${c.y.toFixed(3)},${c.z.toFixed(3)},${c.w.toFixed(3)}`;
    let mat = materials.get(key);
    if (!mat) {
      mat = new THREE.MeshStandardMaterial({
        color: new THREE.Color(c.x, c.y, c.z),
        opacity: c.w,
        transparent: c.w < 1,
        depthWrite: c.w >= 1,
        metalness: 0.0,
        roughness: 0.8,
        side: THREE.DoubleSide,
      });
      materials.set(key, mat);
    }
    return mat;
  };

  let metadata: ElementMetadata = { materials: new Map(), weights: new Map() };
  try {
    try {
      metadata = readElementMetadata(api, modelID, { rels: IFCRELASSOCIATESMATERIAL, props: IFCRELDEFINESBYPROPERTIES });
    } catch (err) {
      console.warn('IFC: could not read materials/weights', err);
    }
    let storeyOf: (expressID: number) => StoreyInfo | undefined = () => undefined;
    try {
      storeyOf = readStoreys(api, modelID, {
        storeys: IFCBUILDINGSTOREY, contained: IFCRELCONTAINEDINSPATIALSTRUCTURE, aggregates: IFCRELAGGREGATES,
      });
    } catch (err) {
      console.warn('IFC: could not read storeys', err);
    }

    api.StreamAllMeshes(modelID, (flatMesh: any, index: number, total: number) => {
      if (total > 0 && index % 50 === 0) {
        onProgress?.(0.35 + 0.55 * (index / total), `Building IFC elements (${index}/${total})...`);
      }

      const expressID: number = flatMesh.expressID;
      let className = 'IfcProduct';
      let name = '';
      let globalId = '';
      try {
        const line = api.GetLine(modelID, expressID);
        className = toClassName(api.GetNameFromTypeCode(api.GetLineType(modelID, expressID)), line);
        name = line?.Name?.value ?? '';
        globalId = line?.GlobalId?.value ?? '';
      } catch { /* metadata is optional */ }

      let group = classGroups.get(className);
      if (!group) {
        group = new THREE.Group();
        group.name = className;
        classGroups.set(className, group);
        root.add(group);
      }

      const placed = flatMesh.geometries;
      for (let i = 0; i < placed.size(); i++) {
        const pg = placed.get(i);
        const geom = api.GetGeometry(modelID, pg.geometryExpressID);
        const verts: Float32Array = api.GetVertexArray(geom.GetVertexData(), geom.GetVertexDataSize());
        const idx: Uint32Array = api.GetIndexArray(geom.GetIndexData(), geom.GetIndexDataSize());
        geom.delete();
        if (verts.length === 0 || idx.length === 0) continue;

        // Interleaved [px, py, pz, nx, ny, nz]
        const count = verts.length / 6;
        const positions = new Float32Array(count * 3);
        const normals = new Float32Array(count * 3);
        for (let v = 0; v < count; v++) {
          positions[v * 3] = verts[v * 6];
          positions[v * 3 + 1] = verts[v * 6 + 1];
          positions[v * 3 + 2] = verts[v * 6 + 2];
          normals[v * 3] = verts[v * 6 + 3];
          normals[v * 3 + 1] = verts[v * 6 + 4];
          normals[v * 3 + 2] = verts[v * 6 + 5];
        }

        const bg = new THREE.BufferGeometry();
        bg.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        bg.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
        bg.setIndex(new THREE.BufferAttribute(new Uint32Array(idx), 1));
        matrix.fromArray(pg.flatTransformation).premultiply(Y_UP_TO_Z_UP);
        bg.applyMatrix4(matrix);

        const mesh = new THREE.Mesh(bg, getMaterial(pg.color));
        mesh.name = name || `${className} #${expressID}`;
        const elementMaterials = metadata.materials.get(expressID);
        const storey = storeyOf(expressID);
        mesh.userData = {
          ifcClass: className, expressID, globalId,
          material: elementMaterials?.[0],
          materials: elementMaterials,
          weight: metadata.weights.get(expressID),
          storey: storey?.name,
          storeyId: storey?.id,
          storeyElevation: storey?.elevation,
        };
        group.add(mesh);
      }
    });
  } finally {
    api.CloseModel(modelID);
  }

  if (classGroups.size === 0) throw new Error('IFC: the file contains no geometry');

  onProgress?.(0.95, 'Finalizing...');
  return finalizeScene(root);
}
