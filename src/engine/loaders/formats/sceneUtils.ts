import * as THREE from 'three';
import type { ThreeJSResult } from '../types';

/**
 * Shared post-processing for loaders that produce a Three.js scene graph:
 * ensures normals, gives untextured basic meshes a lit default material,
 * and computes vertex/triangle counts and bounds.
 */
export function finalizeScene(root: THREE.Object3D): ThreeJSResult {
  const group = root instanceof THREE.Group || root instanceof THREE.Scene ? root : new THREE.Group().add(root);

  let totalVertices = 0;
  let totalTriangles = 0;

  group.traverse((child) => {
    if (child instanceof THREE.Mesh) {
      const geo = child.geometry as THREE.BufferGeometry;
      const count = geo.attributes.position ? geo.attributes.position.count : 0;
      totalVertices += count;
      totalTriangles += geo.index ? geo.index.count / 3 : count / 3;
      if (!geo.attributes.normal && count > 0) geo.computeVertexNormals();
    } else if (child instanceof THREE.Points || child instanceof THREE.Line) {
      const geo = child.geometry as THREE.BufferGeometry;
      totalVertices += geo.attributes.position ? geo.attributes.position.count : 0;
    }
  });

  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group as THREE.Group,
    totalVertices,
    totalTriangles: Math.round(totalTriangles),
    bounds: { min: box.min, max: box.max },
  };
}

export function defaultMaterial(color: THREE.ColorRepresentation = 0x888888): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({
    color,
    metalness: 0.1,
    roughness: 0.7,
    side: THREE.DoubleSide,
  });
}

export function getExt(fileName: string): string {
  const i = fileName.lastIndexOf('.');
  return i === -1 ? '' : fileName.substring(i + 1).toLowerCase();
}
