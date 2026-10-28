import * as THREE from 'three';
import { t } from '../../../i18n/useTranslation';
import type { ThreeJSResult, ProgressCallback } from '../types';

const isTauri = !!(window as any).__TAURI_INTERNALS__;

const KIND_MESH = 0;
const KIND_LINES = 1;
// 2 = points (anything that is not a mesh or lines)

/**
 * AutoCAD DWG (3D model space) and E57 point clouds. Both are converted by the Tauri backend
 * (acadrust / e57 crates) into one binary "O3DS" buffer, metres and Z up:
 * "O3DS", u32 version, 3 x f64 origin, u32 note length + notes (UTF-8, padded to 4),
 * u32 group count; per group u32 name length + name (padded), u32 item count; per item
 * u32 kind (0 mesh, 1 line segments, 2 points), 3 x f32 colour, u32 vertex count, u32 extra,
 * positions (f32 x 3n), then for meshes normals (f32 x 3n) and `extra` u32 indices, for points
 * with extra == 1 RGB bytes (padded to 4); finally 6 x f32 bounds.
 * Each group (DWG layer, E57 scan) becomes a named THREE.Group so it shows up as a layer.
 * Models far from the origin (e.g. RD coordinates) are shifted by `origin`, kept in
 * `scene.userData.originOffset`.
 */
export async function loadDWG(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  return convertAndBuild(data, fileName, 'DWG', 'convert_dwg', onProgress);
}

export async function loadE57(data: ArrayBuffer, fileName: string, onProgress?: ProgressCallback): Promise<ThreeJSResult> {
  return convertAndBuild(data, fileName, 'E57', 'convert_e57', onProgress);
}

async function convertAndBuild(
  data: ArrayBuffer,
  fileName: string,
  format: string,
  command: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  if (!isTauri) {
    throw new Error(t('loader:rust.tauriRequired', { format }));
  }
  onProgress?.(0.2, t('loader:rust.convertingShort', { format }));
  const { invoke } = await import('@tauri-apps/api/core');
  let response: ArrayBuffer;
  try {
    response = await invoke<ArrayBuffer>(command, new Uint8Array(data));
  } catch (err) {
    throw new Error(t('loader:rust.converterError', { format, error: err instanceof Error ? err.message : String(err) }));
  }
  onProgress?.(0.7, t('loader:rust.buildingScene'));
  const result = buildScene(response, fileName, format);
  onProgress?.(0.95, t('loader:rust.finishing'));
  return result;
}

function buildScene(buffer: ArrayBuffer, fileName: string, format: string): ThreeJSResult {
  if (buffer.byteLength < 8 || new TextDecoder().decode(new Uint8Array(buffer, 0, 4)) !== 'O3DS') {
    throw new Error(t('loader:rust.conversionFailed', { format }));
  }
  const view = new DataView(buffer);
  let o = 4;
  const u32 = () => { const v = view.getUint32(o, true); o += 4; return v; };
  const f32 = () => { const v = view.getFloat32(o, true); o += 4; return v; };
  const f64 = () => { const v = view.getFloat64(o, true); o += 8; return v; };
  const str = () => {
    const n = u32();
    const s = new TextDecoder().decode(new Uint8Array(buffer, o, n));
    o += n + ((4 - (n % 4)) % 4);
    return s;
  };
  // slice() copies, so every attribute owns an aligned buffer
  const take = (bytes: number) => { const b = buffer.slice(o, o + bytes); o += bytes; return b; };

  u32(); // version
  const origin = [f64(), f64(), f64()];
  const notes = str();
  const groupCount = u32();

  const root = new THREE.Group();
  root.name = fileName;
  root.userData.originOffset = origin;
  root.userData.importNotes = notes;
  let totalVertices = 0;
  let totalTriangles = 0;
  // one material per object: viewer tools (selection, x-ray) may change materials per object
  const material = (kind: number, color: THREE.Color, vertexColors: boolean): THREE.Material => {
    if (kind === KIND_MESH) {
      return new THREE.MeshStandardMaterial({
        color,
        metalness: 0.0,
        roughness: 0.85,
        side: THREE.DoubleSide,
        // keep coplanar edges (drawn as lines) on top of the faces
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 1,
      });
    }
    if (kind === KIND_LINES) {
      return new THREE.LineBasicMaterial({ color });
    }
    return new THREE.PointsMaterial({ size: 2, sizeAttenuation: false, vertexColors, color: vertexColors ? 0xffffff : color });
  };

  for (let g = 0; g < groupCount; g++) {
    const group = new THREE.Group();
    group.name = str();
    const itemCount = u32();
    for (let i = 0; i < itemCount; i++) {
      const kind = u32();
      const color = new THREE.Color(f32(), f32(), f32());
      const n = u32();
      const extra = u32();
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(take(n * 12)), 3));
      let object: THREE.Object3D;
      if (kind === KIND_MESH) {
        geometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(take(n * 12)), 3));
        geometry.setIndex(new THREE.BufferAttribute(new Uint32Array(take(extra * 4)), 1));
        object = new THREE.Mesh(geometry, material(kind, color, false));
        totalTriangles += extra / 3;
      } else if (kind === KIND_LINES) {
        object = new THREE.LineSegments(geometry, material(kind, color, false));
      } else {
        const hasColors = extra === 1;
        if (hasColors) {
          const rgb = new Uint8Array(take(n * 3));
          o += (4 - ((n * 3) % 4)) % 4;
          geometry.setAttribute('color', new THREE.BufferAttribute(rgb, 3, true));
        }
        object = new THREE.Points(geometry, material(kind, color, hasColors));
      }
      object.name = group.name;
      group.add(object);
      totalVertices += n;
    }
    root.add(group);
  }

  const [x0, y0, z0, x1, y1, z1] = [f32(), f32(), f32(), f32(), f32(), f32()];
  const min = new THREE.Vector3(x0, y0, z0);
  const max = new THREE.Vector3(x1, y1, z1);
  if (min.equals(max)) {
    const box = new THREE.Box3().setFromObject(root);
    min.copy(box.min);
    max.copy(box.max);
  }
  if (notes) console.info(`[${format}] ${fileName}\n${notes}`);
  if (origin.some((v) => v !== 0)) {
    console.info(`[${format}] ${fileName}: model shifted by (${origin.join(', ')}) m to keep precision`);
  }
  return { scene: root, totalVertices, totalTriangles: Math.round(totalTriangles), bounds: { min, max } };
}
