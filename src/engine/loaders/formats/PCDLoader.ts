import * as THREE from 'three';
import { PCDLoader as ThreePCDLoader } from 'three/examples/jsm/loaders/PCDLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadPCD(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing PCD...');

  const loader = new ThreePCDLoader();
  const points = loader.parse(data);

  onProgress?.(0.8, 'Building point cloud...');

  const group = new THREE.Group();
  group.add(points);

  const totalVertices = points.geometry.attributes.position ? points.geometry.attributes.position.count : 0;

  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles: 0,
    bounds: { min: box.min, max: box.max },
  };
}
