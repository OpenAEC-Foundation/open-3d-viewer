import * as THREE from 'three';
import { AMFLoader as ThreeAMFLoader } from 'three/examples/jsm/loaders/AMFLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadAMF(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing AMF...');

  const loader = new ThreeAMFLoader();
  const group = loader.parse(data);

  onProgress?.(0.8, 'Processing meshes...');

  let totalVertices = 0;
  let totalTriangles = 0;

  group.traverse((child) => {
    if (child instanceof THREE.Mesh) {
      const geo = child.geometry;
      totalVertices += geo.attributes.position ? geo.attributes.position.count : 0;
      totalTriangles += geo.index ? geo.index.count / 3 : (geo.attributes.position ? geo.attributes.position.count / 3 : 0);
    }
  });

  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles,
    bounds: { min: box.min, max: box.max },
  };
}
