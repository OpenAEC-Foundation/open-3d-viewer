import * as THREE from 'three';
import { GCodeLoader as ThreeGCodeLoader } from 'three/examples/jsm/loaders/GCodeLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadGCode(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing GCode...');

  const text = new TextDecoder().decode(data);
  const loader = new ThreeGCodeLoader();
  const group = loader.parse(text);

  onProgress?.(0.8, 'Building geometry...');

  let totalVertices = 0;

  group.traverse((child) => {
    if ((child as THREE.Line).isLine) {
      const geo = (child as THREE.Line).geometry;
      totalVertices += geo.attributes.position ? geo.attributes.position.count : 0;
    }
  });

  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles: 0, // GCode produces lines, not triangles
    bounds: { min: box.min, max: box.max },
  };
}
