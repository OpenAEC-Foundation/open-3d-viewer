import * as THREE from 'three';
import occtWasmUrl from 'occt-import-js/dist/occt-import-js.wasm?url';
import type { ThreeJSResult, ProgressCallback } from '../types';
import { finalizeScene, defaultMaterial, getExt } from './sceneUtils';

/**
 * OCCTLoader — STEP, IGES and BREP via occt-import-js (OpenCascade compiled to WASM).
 * Produces exact B-rep tessellation with per-face colors, entirely in the browser.
 */

interface OcctMesh {
  name: string;
  color?: [number, number, number];
  brep_faces?: { first: number; last: number; color: [number, number, number] | null }[];
  attributes: { position: { array: number[] }; normal?: { array: number[] } };
  index: { array: number[] };
}

interface OcctNode {
  name: string;
  meshes: number[];
  children: OcctNode[];
}

let occtPromise: Promise<any> | null = null;

function getOcct(): Promise<any> {
  if (!occtPromise) {
    occtPromise = import('occt-import-js').then((mod: any) => {
      let factory = mod.default ?? mod;
      if (typeof factory !== 'function' && typeof factory?.default === 'function') factory = factory.default;
      return factory({ locateFile: (path: string) => (path.endsWith('.wasm') ? occtWasmUrl : path) });
    });
    occtPromise.catch(() => { occtPromise = null; });
  }
  return occtPromise;
}

function buildMesh(m: OcctMesh): THREE.Mesh {
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.Float32BufferAttribute(m.attributes.position.array, 3));
  if (m.attributes.normal) {
    geo.setAttribute('normal', new THREE.Float32BufferAttribute(m.attributes.normal.array, 3));
  }
  geo.setIndex(m.index.array);
  if (!m.attributes.normal) geo.computeVertexNormals();

  const baseColor = m.color ? new THREE.Color(m.color[0], m.color[1], m.color[2]) : new THREE.Color(0x9aa3ad);
  const faces = m.brep_faces ?? [];
  const hasFaceColors = faces.some((f) => f.color);

  if (!hasFaceColors) {
    const mesh = new THREE.Mesh(geo, defaultMaterial(baseColor));
    mesh.name = m.name || 'Shape';
    return mesh;
  }

  // One material per distinct face color, faces mapped to geometry groups
  const materials: THREE.Material[] = [defaultMaterial(baseColor)];
  const colorIndex = new Map<string, number>();
  for (const f of faces) {
    let matIdx = 0;
    if (f.color) {
      const key = f.color.join(',');
      if (!colorIndex.has(key)) {
        colorIndex.set(key, materials.length);
        materials.push(defaultMaterial(new THREE.Color(f.color[0], f.color[1], f.color[2])));
      }
      matIdx = colorIndex.get(key)!;
    }
    geo.addGroup(f.first * 3, (f.last - f.first + 1) * 3, matIdx);
  }
  const mesh = new THREE.Mesh(geo, materials);
  mesh.name = m.name || 'Shape';
  return mesh;
}

function buildNode(node: OcctNode, meshes: OcctMesh[]): THREE.Group {
  const group = new THREE.Group();
  group.name = node.name || 'Part';
  for (const idx of node.meshes) group.add(buildMesh(meshes[idx]));
  for (const child of node.children) group.add(buildNode(child, meshes));
  return group;
}

export async function loadWithOCCT(
  data: ArrayBuffer,
  fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  const ext = getExt(fileName);
  onProgress?.(0.25, `Loading OpenCascade engine (${ext.toUpperCase()})...`);
  const occt = await getOcct();

  onProgress?.(0.4, `Tessellating ${ext.toUpperCase()} geometry...`);
  const bytes = new Uint8Array(data);
  const params = {
    linearUnit: 'millimeter',
    linearDeflectionType: 'bounding_box_ratio',
    linearDeflection: 0.001,
    angularDeflection: 0.5,
  };

  let result: { success: boolean; root: OcctNode; meshes: OcctMesh[] };
  if (ext === 'step' || ext === 'stp') result = occt.ReadStepFile(bytes, params);
  else if (ext === 'iges' || ext === 'igs') result = occt.ReadIgesFile(bytes, params);
  else result = occt.ReadBrepFile(bytes, params);

  if (!result.success || result.meshes.length === 0) {
    throw new Error(`${ext.toUpperCase()}: OpenCascade could not read this file or it contains no solids/surfaces`);
  }

  onProgress?.(0.85, 'Building scene...');
  const root = buildNode(result.root, result.meshes);
  if (!root.name || root.name === 'Part') root.name = fileName;
  return finalizeScene(root);
}
