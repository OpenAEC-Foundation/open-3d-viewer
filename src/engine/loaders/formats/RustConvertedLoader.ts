import * as THREE from 'three';
import { t } from '../../../i18n/useTranslation';
import type { ThreeJSResult, ProgressCallback } from '../types';

/**
 * Conversion result from the Rust backend.
 */
interface ConversionResult {
  success: boolean;
  meshes: Array<{
    batch_index: number;
    positions: number[];
    indices: number[];
    normals: number[];
    is_last: boolean;
  }>;
  total_vertices: number;
  total_triangles: number;
  bounds_min: [number, number, number];
  bounds_max: [number, number, number];
  error: string | null;
}

const isTauri = !!(window as any).__TAURI_INTERNALS__;

/**
 * Load a file by converting it via the Rust backend.
 * Used for STEP/STP and SKP formats that don't have good JS loaders.
 *
 * Requires the Tauri desktop app — will throw a clear error in browser mode.
 */
export async function loadViaRustConverter(
  _data: ArrayBuffer,
  fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  // Extract extension for error messages
  const ext = fileName.substring(fileName.lastIndexOf('.')).toLowerCase();
  const formatName = ext.replace('.', '').toUpperCase();

  // Guard: Tauri required
  if (!isTauri) {
    throw new Error(t('loader:rust.tauriRequired', { format: formatName }));
  }

  onProgress?.(0.1, t('loader:rust.converting', { format: formatName }));

  let invoke: any;
  try {
    const core = await import('@tauri-apps/api/core');
    invoke = core.invoke;
  } catch (err) {
    throw new Error(t('loader:rust.cannotLoadTauri', { error: err instanceof Error ? err.message : String(err) }));
  }

  const fileId = crypto.randomUUID();

  onProgress?.(0.2, t('loader:rust.convertingShort', { format: formatName }));

  let result: ConversionResult;
  try {
    result = await invoke('convert_3d_file', {
      fileId,
      filePath: fileName,
    });
  } catch (err) {
    throw new Error(t('loader:rust.converterError', { format: formatName, error: err instanceof Error ? err.message : String(err) }));
  }

  if (!result.success) {
    throw new Error(result.error ?? t('loader:rust.conversionFailed', { format: formatName }));
  }

  if (result.meshes.length === 0) {
    throw new Error(t('loader:rust.noGeometry', { format: formatName }));
  }

  onProgress?.(0.7, t('loader:rust.buildingScene'));

  const group = new THREE.Group();
  let totalVertices = 0;
  let totalTriangles = 0;

  for (const mesh of result.meshes) {
    const geometry = new THREE.BufferGeometry();

    const positions = new Float32Array(mesh.positions);
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));

    if (mesh.indices.length > 0) {
      const indices = new Uint32Array(mesh.indices);
      geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    }

    if (mesh.normals.length > 0) {
      const normals = new Float32Array(mesh.normals);
      geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
    } else {
      geometry.computeVertexNormals();
    }

    const material = new THREE.MeshStandardMaterial({
      color: 0x888888,
      metalness: 0.3,
      roughness: 0.6,
      side: THREE.DoubleSide,
    });

    const threeMesh = new THREE.Mesh(geometry, material);
    group.add(threeMesh);

    totalVertices += positions.length / 3;
    totalTriangles += mesh.indices.length > 0 ? mesh.indices.length / 3 : positions.length / 9;
  }

  onProgress?.(0.9, t('loader:rust.finishing'));

  const bounds = {
    min: new THREE.Vector3(result.bounds_min[0], result.bounds_min[1], result.bounds_min[2]),
    max: new THREE.Vector3(result.bounds_max[0], result.bounds_max[1], result.bounds_max[2]),
  };

  if (bounds.min.equals(bounds.max)) {
    const box = new THREE.Box3().setFromObject(group);
    bounds.min.copy(box.min);
    bounds.max.copy(box.max);
  }

  return {
    scene: group,
    totalVertices,
    totalTriangles,
    bounds,
  };
}
