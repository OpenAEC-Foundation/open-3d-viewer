import * as THREE from 'three';
import { VOXLoader as ThreeVOXLoader } from 'three/examples/jsm/loaders/VOXLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

export async function loadVOX(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing VOX...');

  const loader = new ThreeVOXLoader();
  const chunks = loader.parse(data);

  onProgress?.(0.8, 'Building voxel meshes...');

  const group = new THREE.Group();
  let totalVertices = 0;
  let totalTriangles = 0;

  // VOXLoader returns chunks with palette and size data
  // Build simple box geometry for each voxel
  for (const chunk of chunks as unknown as any[]) {
    if (!chunk.data || !chunk.palette) continue;
    const size = chunk.size;
    const voxelData = chunk.data;
    const palette = chunk.palette;

    for (let i = 0; i < voxelData.length; i += 4) {
      const x = voxelData[i];
      const y = voxelData[i + 1];
      const z = voxelData[i + 2];
      const c = voxelData[i + 3];
      const color = palette[c];

      const geo = new THREE.BoxGeometry(1, 1, 1);
      const mat = new THREE.MeshStandardMaterial({
        color: new THREE.Color(
          ((color >> 0) & 0xff) / 255,
          ((color >> 8) & 0xff) / 255,
          ((color >> 16) & 0xff) / 255,
        ),
      });
      const mesh = new THREE.Mesh(geo, mat);
      mesh.position.set(x - size.x / 2, z, y - size.y / 2);
      group.add(mesh);
      totalVertices += 24; // box has 24 vertices
      totalTriangles += 12; // box has 12 triangles
    }
  }

  const box = new THREE.Box3().setFromObject(group);

  return {
    scene: group,
    totalVertices,
    totalTriangles,
    bounds: { min: box.min, max: box.max },
  };
}
