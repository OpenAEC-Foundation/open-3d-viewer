import * as THREE from 'three';
import { VRMLLoader as ThreeVRMLLoader } from 'three/examples/jsm/loaders/VRMLLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadVRML(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing VRML...');

  const text = new TextDecoder().decode(data);
  const loader = new ThreeVRMLLoader();
  const scene = loader.parse(text, '');

  onProgress?.(0.8, 'Processing scene...');

  const group = new THREE.Group();
  // VRMLLoader returns a Scene — wrap children in a Group
  while (scene.children.length > 0) {
    group.add(scene.children[0]);
  }

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
