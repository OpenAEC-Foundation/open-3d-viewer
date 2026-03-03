import * as THREE from 'three';
import { ColladaLoader as ThreeColladaLoader } from 'three/examples/jsm/loaders/ColladaLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadCollada(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing Collada...');

  const text = new TextDecoder().decode(data);
  const loader = new ThreeColladaLoader();
  const collada = loader.parse(text, '');
  if (!collada) throw new Error('Failed to parse Collada file');

  onProgress?.(0.8, 'Processing scene...');

  const scene = collada.scene;
  let totalVertices = 0;
  let totalTriangles = 0;

  scene.traverse((child) => {
    if (child instanceof THREE.Mesh) {
      const geo = child.geometry;
      totalVertices += geo.attributes.position ? geo.attributes.position.count : 0;
      totalTriangles += geo.index ? geo.index.count / 3 : (geo.attributes.position ? geo.attributes.position.count / 3 : 0);
    }
  });

  const box = new THREE.Box3().setFromObject(scene);

  // Wrap in a group to ensure consistent return type
  const group = new THREE.Group();
  group.add(scene);

  return {
    scene: group,
    totalVertices,
    totalTriangles,
    bounds: { min: box.min, max: box.max },
  };
}
