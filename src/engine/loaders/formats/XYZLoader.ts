import * as THREE from 'three';
import { XYZLoader as ThreeXYZLoader } from 'three/examples/jsm/loaders/XYZLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadXYZ(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing XYZ point cloud...');

  const text = new TextDecoder().decode(data);
  const loader = new ThreeXYZLoader();
  let geometry!: THREE.BufferGeometry;
  loader.parse(text, (geo) => { geometry = geo; });

  onProgress?.(0.8, 'Building point cloud...');

  // XYZLoader returns a BufferGeometry — create Points from it
  const hasColors = geometry.attributes.color !== undefined;
  const material = new THREE.PointsMaterial({ size: 0.02, vertexColors: hasColors });
  const points = new THREE.Points(geometry, material);

  const group = new THREE.Group();
  group.add(points);

  const totalVertices = geometry.attributes.position ? geometry.attributes.position.count : 0;
  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles: 0,
    bounds: { min: box.min, max: box.max },
  };
}
