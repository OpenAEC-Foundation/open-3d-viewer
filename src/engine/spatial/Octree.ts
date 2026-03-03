import * as THREE from 'three';

/**
 * Simple Octree for spatial indexing.
 * Used for frustum culling — only renders objects that are
 * within the camera's view frustum.
 */

interface OctreeEntry {
  object: THREE.Object3D;
  bounds: THREE.Box3;
}

export class OctreeNode {
  bounds: THREE.Box3;
  children: OctreeNode[] | null = null;
  entries: OctreeEntry[] = [];
  depth: number;

  private static MAX_ENTRIES = 8;
  private static MAX_DEPTH = 8;

  constructor(bounds: THREE.Box3, depth = 0) {
    this.bounds = bounds;
    this.depth = depth;
  }

  insert(object: THREE.Object3D, objectBounds: THREE.Box3): void {
    if (!this.bounds.intersectsBox(objectBounds)) return;

    if (this.children) {
      for (const child of this.children) {
        child.insert(object, objectBounds);
      }
      return;
    }

    this.entries.push({ object, bounds: objectBounds });

    if (this.entries.length > OctreeNode.MAX_ENTRIES && this.depth < OctreeNode.MAX_DEPTH) {
      this.subdivide();
    }
  }

  /**
   * Query all objects that intersect with the given frustum.
   */
  queryFrustum(frustum: THREE.Frustum, result: THREE.Object3D[]): void {
    if (!frustum.intersectsBox(this.bounds)) return;

    for (const entry of this.entries) {
      if (frustum.intersectsBox(entry.bounds)) {
        result.push(entry.object);
      }
    }

    if (this.children) {
      for (const child of this.children) {
        child.queryFrustum(frustum, result);
      }
    }
  }

  /**
   * Query all objects that intersect with the given box.
   */
  queryBox(box: THREE.Box3, result: THREE.Object3D[]): void {
    if (!this.bounds.intersectsBox(box)) return;

    for (const entry of this.entries) {
      if (box.intersectsBox(entry.bounds)) {
        result.push(entry.object);
      }
    }

    if (this.children) {
      for (const child of this.children) {
        child.queryBox(box, result);
      }
    }
  }

  private subdivide(): void {
    const min = this.bounds.min;
    const max = this.bounds.max;
    const mid = new THREE.Vector3().addVectors(min, max).multiplyScalar(0.5);

    this.children = [
      new OctreeNode(new THREE.Box3(new THREE.Vector3(min.x, min.y, min.z), new THREE.Vector3(mid.x, mid.y, mid.z)), this.depth + 1),
      new OctreeNode(new THREE.Box3(new THREE.Vector3(mid.x, min.y, min.z), new THREE.Vector3(max.x, mid.y, mid.z)), this.depth + 1),
      new OctreeNode(new THREE.Box3(new THREE.Vector3(min.x, mid.y, min.z), new THREE.Vector3(mid.x, max.y, mid.z)), this.depth + 1),
      new OctreeNode(new THREE.Box3(new THREE.Vector3(mid.x, mid.y, min.z), new THREE.Vector3(max.x, max.y, mid.z)), this.depth + 1),
      new OctreeNode(new THREE.Box3(new THREE.Vector3(min.x, min.y, mid.z), new THREE.Vector3(mid.x, mid.y, max.z)), this.depth + 1),
      new OctreeNode(new THREE.Box3(new THREE.Vector3(mid.x, min.y, mid.z), new THREE.Vector3(max.x, mid.y, max.z)), this.depth + 1),
      new OctreeNode(new THREE.Box3(new THREE.Vector3(min.x, mid.y, mid.z), new THREE.Vector3(mid.x, max.y, max.z)), this.depth + 1),
      new OctreeNode(new THREE.Box3(new THREE.Vector3(mid.x, mid.y, mid.z), new THREE.Vector3(max.x, max.y, max.z)), this.depth + 1),
    ];

    // Re-distribute entries
    const entries = this.entries;
    this.entries = [];
    for (const entry of entries) {
      for (const child of this.children) {
        child.insert(entry.object, entry.bounds);
      }
    }
  }
}

/**
 * High-level Octree manager for the scene.
 */
export class Octree {
  private root: OctreeNode | null = null;

  /**
   * Build the octree from scene bounds and objects.
   */
  build(sceneBounds: THREE.Box3, objects: THREE.Object3D[]): void {
    if (sceneBounds.isEmpty()) return;

    // Expand bounds slightly to avoid edge cases
    const expanded = sceneBounds.clone().expandByScalar(1);
    this.root = new OctreeNode(expanded);

    const box = new THREE.Box3();
    for (const obj of objects) {
      box.setFromObject(obj);
      if (!box.isEmpty()) {
        this.root.insert(obj, box.clone());
      }
    }
  }

  /**
   * Get all objects visible from the camera's frustum.
   */
  queryFrustum(camera: THREE.Camera): THREE.Object3D[] {
    if (!this.root) return [];

    const frustum = new THREE.Frustum();
    const projScreenMatrix = new THREE.Matrix4();
    projScreenMatrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    frustum.setFromProjectionMatrix(projScreenMatrix);

    const result: THREE.Object3D[] = [];
    this.root.queryFrustum(frustum, result);
    return result;
  }

  /**
   * Clear the octree.
   */
  clear(): void {
    this.root = null;
  }
}
