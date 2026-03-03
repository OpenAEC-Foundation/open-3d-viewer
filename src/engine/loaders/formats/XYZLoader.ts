import * as THREE from 'three';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadXYZ(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing XYZ point cloud...');

  const text = new TextDecoder().decode(data);

  onProgress?.(0.8, 'Building point cloud...');

  // Parse XYZ manually: each line = "x y z" or "x y z r g b"
  const lines = text.trim().split('\n').filter(l => l.trim() && !l.startsWith('#'));
  const positions: number[] = [];
  const colors: number[] = [];
  let hasColors = false;

  for (const line of lines) {
    const parts = line.trim().split(/\s+/).map(Number);
    if (parts.length >= 3 && !isNaN(parts[0])) {
      positions.push(parts[0], parts[1], parts[2]);
      if (parts.length >= 6) {
        hasColors = true;
        // Colors: if values > 1, assume 0-255 range
        const scale = parts[3] > 1 || parts[4] > 1 || parts[5] > 1 ? 1 / 255 : 1;
        colors.push(parts[3] * scale, parts[4] * scale, parts[5] * scale);
      }
    }
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  if (hasColors && colors.length === positions.length) {
    geometry.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
  }

  const material = new THREE.PointsMaterial({
    size: 0.05,
    vertexColors: hasColors,
    color: hasColors ? undefined : 0x44aaff,
  });
  const points = new THREE.Points(geometry, material);

  const group = new THREE.Group();
  group.add(points);

  const totalVertices = positions.length / 3;
  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles: 0,
    bounds: { min: box.min, max: box.max },
  };
}
