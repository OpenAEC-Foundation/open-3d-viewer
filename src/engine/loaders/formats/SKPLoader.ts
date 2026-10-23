/**
 * SketchUp SKP loader.
 *
 * Main path: openskp (MIT, https://github.com/iamahsanmehmood/openskp). It reads both
 * containers — the 2021+ ZIP ("VFF") format and the 2013–2020 MFC archive — and keeps
 * SketchUp's instancing, materials, textures, holes and nested components. The instanced scene
 * is exported to GLB (metres, Y up) and loaded with the regular glTF loader. The meshes are then
 * regrouped per SketchUp tag (layer), and the model is turned to the viewer's Z up.
 *
 * Fallback: the earlier reverse-engineered parser (SKPLegacyLoader.ts) for files openskp
 * rejects.
 */

import * as THREE from 'three';
import type { InstancedNode } from 'openskp';
import type { ThreeJSResult, ProgressCallback } from '../types';
import { loadGLTF } from './GLTFLoader';
import { loadSKPLegacy } from './SKPLegacyLoader';

// SketchUp's default tag is stored as "Layer0" but shown as "Untagged" since SketchUp 2020
const UNTAGGED = 'Untagged';
const DEFAULT_LAYER = 'Layer0';

export async function loadSKP(
  data: ArrayBuffer,
  fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.05, 'Parsing SketchUp file...');
  let primaryError: unknown;
  try {
    const { buildInstancedScene, toInstancedGLB } = await import('openskp');
    // parse a copy, so the legacy fallback still receives the untouched bytes
    const scene = buildInstancedScene(data.slice(0));
    if (!scene.bounds || scene.meshResources.length === 0) {
      throw new Error('no geometry');
    }
    onProgress?.(0.5, 'Building scene...');
    const glb = toInstancedGLB(scene, { textures: true });
    // toInstancedGLB writes one glTF node per hierarchy node, in pre-order
    const layers: string[] = [];
    const walk = (n: InstancedNode) => { layers.push(n.layer); n.children.forEach(walk); };
    walk(scene.sceneHierarchy);
    const result = await loadGLTF(withNodeLayers(glb, layers), fileName, onProgress);
    const root = groupByLayer(result.scene, scene.layerHidden);
    root.name = fileName;
    root.rotation.x = Math.PI / 2; // glTF Y up -> viewer Z up
    root.updateMatrixWorld(true);
    const box = new THREE.Box3().setFromObject(root);
    return { ...result, scene: root, bounds: { min: box.min, max: box.max } };
  } catch (err) {
    primaryError = err;
    console.warn(`openskp could not read ${fileName}, trying the legacy SKP parser:`, err);
  }
  try {
    return await loadSKPLegacy(data, fileName, onProgress);
  } catch (legacyError) {
    const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
    throw new Error(`${fileName}: ${msg(primaryError)}\n(legacy parser: ${msg(legacyError)})`);
  }
}

/** Copy of a GLB with `extras.layer` on every node (three.js puts extras in userData). */
function withNodeLayers(glb: Uint8Array, layers: string[]): ArrayBuffer {
  const view = new DataView(glb.buffer, glb.byteOffset, glb.byteLength);
  const jsonLength = view.getUint32(12, true);
  const json = JSON.parse(new TextDecoder().decode(glb.subarray(20, 20 + jsonLength)));
  if (json.nodes?.length !== layers.length) {
    throw new Error(`unexpected GLB node count ${json.nodes?.length} (hierarchy ${layers.length})`);
  }
  json.nodes.forEach((node: { extras?: Record<string, unknown> }, i: number) => {
    node.extras = { ...node.extras, layer: layers[i] };
  });
  let text = new TextEncoder().encode(JSON.stringify(json));
  const padded = new Uint8Array(text.length + ((4 - (text.length % 4)) % 4)).fill(0x20);
  padded.set(text);
  text = padded;
  const rest = glb.subarray(20 + jsonLength); // binary chunk, unchanged
  const out = new Uint8Array(20 + text.length + rest.length);
  const outView = new DataView(out.buffer);
  out.set(glb.subarray(0, 12));
  outView.setUint32(8, out.length, true);
  outView.setUint32(12, text.length, true);
  outView.setUint32(16, 0x4e4f534a, true); // "JSON"
  out.set(text, 20);
  out.set(rest, 20 + text.length);
  return out.buffer;
}

/** Moves every mesh into one group per tag (world transform kept), so tags show up as layers. */
function groupByLayer(gltfScene: THREE.Object3D, layerHidden: Record<string, boolean>): THREE.Group {
  gltfScene.updateMatrixWorld(true);
  const meshes: { mesh: THREE.Mesh; layer: string }[] = [];
  gltfScene.traverse((obj) => {
    if (!(obj instanceof THREE.Mesh)) return;
    let o: THREE.Object3D | null = obj;
    while (o && typeof o.userData.layer !== 'string') o = o.parent;
    meshes.push({ mesh: obj, layer: (o?.userData.layer as string) || UNTAGGED });
  });
  const root = new THREE.Group();
  const groups = new Map<string, THREE.Group>();
  for (const name of [...new Set(meshes.map((m) => m.layer))].sort((a, b) => a.localeCompare(b))) {
    const group = new THREE.Group();
    group.name = name === DEFAULT_LAYER ? UNTAGGED : name;
    group.visible = !layerHidden[name];
    groups.set(name, group);
    root.add(group);
  }
  // pre-order: a parent moves before its children, and attach() keeps every world transform
  for (const { mesh, layer } of meshes) {
    groups.get(layer)!.attach(mesh);
  }
  return root;
}
