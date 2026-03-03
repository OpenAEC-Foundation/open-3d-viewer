import * as THREE from 'three';
import { STLLoader as ThreeSTLLoader } from 'three/examples/jsm/loaders/STLLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadSTL(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing STL...');

  const loader = new ThreeSTLLoader();
  const geometry = loader.parse(data);

  onProgress?.(0.8, 'Building mesh...');

  if (!geometry.attributes.normal) {
    geometry.computeVertexNormals();
  }

  const material = new THREE.MeshStandardMaterial({
    color: 0x888888,
    metalness: 0.2,
    roughness: 0.6,
    side: THREE.DoubleSide,
  });

  // If the STL has vertex colors, use them
  if (geometry.attributes.color) {
    material.vertexColors = true;
  }

  const mesh = new THREE.Mesh(geometry, material);
  const group = new THREE.Group();
  group.add(mesh);

  const totalVertices = geometry.attributes.position.count;
  const totalTriangles = geometry.index ? geometry.index.count / 3 : totalVertices / 3;

  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles,
    bounds: { min: box.min, max: box.max },
  };
}
