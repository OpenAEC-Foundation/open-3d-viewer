import * as THREE from 'three';
import { VTKLoader as ThreeVTKLoader } from 'three/examples/jsm/loaders/VTKLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadVTK(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing VTK...');

  const loader = new ThreeVTKLoader();
  const geometry = loader.parse(data, '');

  onProgress?.(0.8, 'Building mesh...');

  // VTKLoader returns a BufferGeometry — wrap in a mesh
  if (!geometry.attributes.normal) {
    geometry.computeVertexNormals();
  }

  const material = new THREE.MeshStandardMaterial({
    color: 0x88aacc,
    vertexColors: geometry.attributes.color !== undefined,
    side: THREE.DoubleSide,
  });
  const mesh = new THREE.Mesh(geometry, material);

  const group = new THREE.Group();
  group.add(mesh);

  const totalVertices = geometry.attributes.position ? geometry.attributes.position.count : 0;
  const totalTriangles = geometry.index ? geometry.index.count / 3 : totalVertices / 3;
  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles,
    bounds: { min: box.min, max: box.max },
  };
}
