import * as THREE from 'three';
import { OBJLoader as ThreeOBJLoader } from 'three/examples/jsm/loaders/OBJLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadOBJ(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing OBJ...');

  const text = new TextDecoder().decode(data);
  const loader = new ThreeOBJLoader();
  const group = loader.parse(text);

  onProgress?.(0.8, 'Processing geometry...');

  let totalVertices = 0;
  let totalTriangles = 0;

  // Assign default material if none exists
  group.traverse((child) => {
    if (child instanceof THREE.Mesh) {
      const geo = child.geometry;
      totalVertices += geo.attributes.position ? geo.attributes.position.count : 0;
      totalTriangles += geo.index ? geo.index.count / 3 : (geo.attributes.position ? geo.attributes.position.count / 3 : 0);

      // Ensure normals exist
      if (!geo.attributes.normal) {
        geo.computeVertexNormals();
      }

      // Default material if needed
      if (!child.material || (child.material instanceof THREE.MeshBasicMaterial)) {
        child.material = new THREE.MeshStandardMaterial({
          color: 0x888888,
          metalness: 0.1,
          roughness: 0.7,
          side: THREE.DoubleSide,
        });
      }
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
