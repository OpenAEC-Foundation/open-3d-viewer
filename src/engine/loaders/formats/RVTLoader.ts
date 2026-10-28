import * as THREE from 'three';
import { t } from '../../../i18n/useTranslation';
import type { ThreeJSResult, ProgressCallback } from '../types';

const isTauri = !!(window as any).__TAURI_INTERNALS__;

/**
 * Revit (.rvt) loader. The Tauri backend reads the file with the OpenAEC rvt reader and returns
 * one mesh batch per category in the binary "RVTM" format (metres, Y up):
 * "RVTM", u32 version, u32 batch count; per batch u32 name length, name (UTF-8, padded to 4),
 * 3 × f32 colour, u32 vertex count, u32 index count, positions, normals, indices; then bounds.
 * Each batch becomes a named group, so categories show up as layers. The viewer is Z up, so the
 * geometry and bounds are turned back from Y up here.
 */
export async function loadRVT(
  data: ArrayBuffer,
  fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  if (!isTauri) {
    throw new Error(t('loader:rust.tauriRequired', { format: 'RVT' }));
  }

  onProgress?.(0.2, t('loader:rust.convertingShort', { format: 'RVT' }));
  const { invoke } = await import('@tauri-apps/api/core');
  let response: ArrayBuffer;
  try {
    response = await invoke<ArrayBuffer>('convert_rvt', new Uint8Array(data));
  } catch (err) {
    throw new Error(t('loader:rust.converterError', { format: 'RVT', error: err instanceof Error ? err.message : String(err) }));
  }

  onProgress?.(0.7, t('loader:rust.buildingScene'));
  const view = new DataView(response);
  let o = 0;
  const u32 = () => { const v = view.getUint32(o, true); o += 4; return v; };
  const f32 = () => { const v = view.getFloat32(o, true); o += 4; return v; };

  const magic = new TextDecoder().decode(new Uint8Array(response, 0, 4));
  if (magic !== 'RVTM') {
    throw new Error(t('loader:rust.conversionFailed', { format: 'RVT' }));
  }
  o = 4;
  u32(); // version
  const batchCount = u32();

  const root = new THREE.Group();
  root.name = fileName;
  const meshes: THREE.Mesh[] = [];
  let totalVertices = 0;
  let totalTriangles = 0;

  for (let b = 0; b < batchCount; b++) {
    const nameLen = u32();
    const name = new TextDecoder().decode(new Uint8Array(response, o, nameLen));
    o += nameLen + ((4 - (nameLen % 4)) % 4);
    const color = new THREE.Color(f32(), f32(), f32());
    const vertexCount = u32();
    const indexCount = u32();
    // slice() copies, so each geometry owns its own buffer
    const positions = new Float32Array(response.slice(o, o + vertexCount * 12)); o += vertexCount * 12;
    const normals = new Float32Array(response.slice(o, o + vertexCount * 12)); o += vertexCount * 12;
    const indices = new Uint32Array(response.slice(o, o + indexCount * 4)); o += indexCount * 4;

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    geometry.rotateX(Math.PI / 2); // (x, y, z) Y up -> (x, -z, y) Z up

    const material = new THREE.MeshStandardMaterial({
      color,
      metalness: 0.0,
      roughness: 0.85,
      side: THREE.DoubleSide,
    });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.name = name;
    meshes.push(mesh);
    const group = new THREE.Group();
    group.name = name;
    group.add(mesh);
    root.add(group);

    totalVertices += vertexCount;
    totalTriangles += indexCount / 3;
  }

  const [x0, y0, z0, x1, y1, z1] = [f32(), f32(), f32(), f32(), f32(), f32()];
  const min = new THREE.Vector3(x0, -z1, y0);
  const max = new THREE.Vector3(x1, -z0, y1);
  if (min.equals(max)) {
    const box = new THREE.Box3().setFromObject(root);
    min.copy(box.min);
    max.copy(box.max);
  }

  readTrailer(view, o, meshes, root);

  onProgress?.(0.95, t('loader:rust.finishing'));
  return { scene: root, totalVertices, totalTriangles, bounds: { min, max } };
}

/** One element in a batch: its id and its index range (`mesh.userData.elements`). */
export interface RvtElement {
  id: number;
  start: number;
  count: number;
}

/** sRGB byte to linear (vertex colours are in the linear working space). */
function srgbToLinear(v: number): number {
  const c = v / 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

/**
 * Optional trailer after the bounds (rvt-dll step 2309/2311; older output stops at the bounds):
 * "ELMS" per batch the element ranges, "VCOL" per batch RGBA vertex colours (material colours),
 * "PROP" element properties as JSON, "LVLS" the project levels (name, elevation in metres).
 */
function readTrailer(view: DataView, start: number, meshes: THREE.Mesh[], root: THREE.Group) {
  let o = start;
  const len = view.byteLength;
  const tag = () => (o + 4 <= len ? String.fromCharCode(view.getUint8(o), view.getUint8(o + 1), view.getUint8(o + 2), view.getUint8(o + 3)) : '');
  const u32 = () => { const v = view.getUint32(o, true); o += 4; return v; };
  const u64 = () => { const v = Number(view.getBigUint64(o, true)); o += 8; return v; };
  const text = (n: number) => {
    const s = new TextDecoder().decode(new Uint8Array(view.buffer, view.byteOffset + o, n));
    o += n + ((4 - (n % 4)) % 4);
    return s;
  };
  try {
    if (tag() === 'ELMS') {
      o += 4;
      for (const mesh of meshes) {
        const n = u32();
        const elements: RvtElement[] = [];
        for (let i = 0; i < n; i++) {
          const id = u64();
          elements.push({ id, start: u32(), count: u32() });
        }
        if (elements.length) mesh.userData.elements = elements;
      }
    }
    if (tag() === 'VCOL') {
      o += 4;
      for (const mesh of meshes) {
        if (!u32()) continue;
        const geo = mesh.geometry;
        const n = geo.attributes.position.count;
        const bytes = new Uint8Array(view.buffer, view.byteOffset + o, n * 4);
        o += n * 4;
        const colors = new Float32Array(n * 4);
        let alpha = false;
        for (let i = 0; i < n * 4; i += 4) {
          colors[i] = srgbToLinear(bytes[i]);
          colors[i + 1] = srgbToLinear(bytes[i + 1]);
          colors[i + 2] = srgbToLinear(bytes[i + 2]);
          colors[i + 3] = bytes[i + 3] / 255;
          if (bytes[i + 3] < 255) alpha = true;
        }
        geo.setAttribute('color', new THREE.BufferAttribute(colors, 4));
        const mat = mesh.material as THREE.MeshStandardMaterial;
        mat.vertexColors = true;
        mat.color.set(0xffffff);
        if (alpha) {
          mat.transparent = true;
          mat.depthWrite = true;
        }
        mat.needsUpdate = true;
      }
    }
    if (tag() === 'PROP') {
      o += 4;
      const n = u32();
      const props = new Map<number, Record<string, string>>();
      for (let i = 0; i < n; i++) {
        const id = u64();
        try {
          props.set(id, JSON.parse(text(u32())));
        } catch {
          /* one unreadable record does not stop the rest */
        }
      }
      root.userData.rvtProps = props;
    }
    if (tag() === 'LVLS') {
      o += 4;
      const n = u32();
      const levels: { name: string; elevation: number }[] = [];
      for (let i = 0; i < n; i++) {
        const name = text(u32());
        levels.push({ name, elevation: view.getFloat32(o, true) });
        o += 4;
      }
      root.userData.levels = levels;
    }
  } catch (err) {
    console.warn('RVT trailer incomplete', err);
  }
}
