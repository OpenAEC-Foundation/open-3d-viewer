import * as THREE from 'three';
import webIfcWasmUrl from 'web-ifc/web-ifc.wasm?url';
import type { ThreeJSResult, ProgressCallback } from '../types';
import { finalizeScene } from './sceneUtils';

/**
 * IFCLoader — native IFC (2x3, 4, 4.3) via web-ifc (WASM).
 * Elements are grouped per IFC class (IfcWall, IfcSlab, ...) so each class
 * shows up as a layer; each mesh carries its IFC name, GlobalId and class.
 */

let apiPromise: Promise<any> | null = null;

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

  try {
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
        matrix.fromArray(pg.flatTransformation);
        bg.applyMatrix4(matrix);

        const mesh = new THREE.Mesh(bg, getMaterial(pg.color));
        mesh.name = name || `${className} #${expressID}`;
        mesh.userData = { ifcClass: className, expressID, globalId };
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
