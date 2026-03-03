import * as THREE from 'three';
import { mergeGeometries } from 'three/examples/jsm/utils/BufferGeometryUtils.js';

/**
 * MergedGeometry — Merges small meshes into larger batched meshes
 * to reduce the number of draw calls.
 *
 * Particularly useful for IFC and CAD models that may contain
 * thousands of small geometry objects.
 */

interface MergeGroup {
  material: THREE.Material;
  geometries: THREE.BufferGeometry[];
  transforms: THREE.Matrix4[];
}

export class MergedGeometry {
  /**
   * Merge meshes in a group that share the same material.
   * Returns a new group with merged geometry.
   *
   * @param source - The source group containing meshes
   * @param maxVerticesPerBatch - Max vertices before splitting into a new batch
   */
  static merge(
    source: THREE.Group,
    maxVerticesPerBatch = 500_000
  ): THREE.Group {
    // Group meshes by material
    const groups = new Map<string, MergeGroup>();

    source.traverse((child) => {
      if (!(child instanceof THREE.Mesh)) return;

      const material = child.material as THREE.Material;
      const key = material.uuid;

      if (!groups.has(key)) {
        groups.set(key, { material, geometries: [], transforms: [] });
      }

      const group = groups.get(key)!;
      const geom = child.geometry.clone();

      // Apply world transform to geometry
      child.updateWorldMatrix(true, false);
      geom.applyMatrix4(child.matrixWorld);

      group.geometries.push(geom);
      group.transforms.push(child.matrixWorld.clone());
    });

    // Merge each material group
    const result = new THREE.Group();
    result.name = source.name + '-merged';

    for (const [, group] of groups) {
      // Split into batches if needed
      const batches = MergedGeometry.splitIntoBatches(
        group.geometries,
        maxVerticesPerBatch
      );

      for (const batch of batches) {
        const merged = mergeGeometries(batch, false);
        if (!merged) continue;

        const mesh = new THREE.Mesh(merged, group.material);
        result.add(mesh);
      }
    }

    return result;
  }

  /**
   * Count the total draw calls in a group.
   */
  static countDrawCalls(group: THREE.Object3D): number {
    let count = 0;
    group.traverse((child) => {
      if (child instanceof THREE.Mesh || child instanceof THREE.Points) {
        count++;
      }
    });
    return count;
  }

  /**
   * Split geometries into batches that don't exceed the vertex limit.
   */
  private static splitIntoBatches(
    geometries: THREE.BufferGeometry[],
    maxVertices: number
  ): THREE.BufferGeometry[][] {
    const batches: THREE.BufferGeometry[][] = [];
    let currentBatch: THREE.BufferGeometry[] = [];
    let currentCount = 0;

    for (const geom of geometries) {
      const vertexCount = geom.attributes.position?.count ?? 0;

      if (currentCount + vertexCount > maxVertices && currentBatch.length > 0) {
        batches.push(currentBatch);
        currentBatch = [];
        currentCount = 0;
      }

      currentBatch.push(geom);
      currentCount += vertexCount;
    }

    if (currentBatch.length > 0) {
      batches.push(currentBatch);
    }

    return batches;
  }
}
