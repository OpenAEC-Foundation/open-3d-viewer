import * as THREE from 'three';
import { PLYLoader as ThreePLYLoader } from 'three/examples/jsm/loaders/PLYLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadPLY(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing PLY...');

  const loader = new ThreePLYLoader();
  const geometry = loader.parse(data);

  onProgress?.(0.8, 'Building mesh...');

  if (!geometry.attributes.normal && geometry.index) {
    geometry.computeVertexNormals();
  }

  const hasColors = !!geometry.attributes.color;
  const hasIndices = !!geometry.index;

  // If PLY has faces, render as mesh. Otherwise, render as points.
  const group = new THREE.Group();

  if (hasIndices) {
    const material = new THREE.MeshStandardMaterial({
      color: hasColors ? 0xffffff : 0x888888,
      metalness: 0.1,
      roughness: 0.7,
      vertexColors: hasColors,
      side: THREE.DoubleSide,
    });
    group.add(new THREE.Mesh(geometry, material));
  } else {
    const material = new THREE.PointsMaterial({
      size: 0.01,
      vertexColors: hasColors,
      color: hasColors ? 0xffffff : 0x888888,
    });
    group.add(new THREE.Points(geometry, material));
  }

  const totalVertices = geometry.attributes.position.count;
  const totalTriangles = geometry.index ? geometry.index.count / 3 : 0;

  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles,
    bounds: { min: box.min, max: box.max },
  };
}
