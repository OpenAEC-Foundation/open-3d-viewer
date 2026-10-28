import * as THREE from 'three';
import { Octree } from '../spatial/Octree';

/**
 * FrustumCuller — Octree-based frustum culling.
 *
 * Hides objects outside the camera frustum to reduce draw calls.
 * Particularly important for large scenes with many objects.
 */
export class FrustumCuller {
  private octree = new Octree();
  private scene: THREE.Scene;
  private enabled = false;
  private cullableObjects: THREE.Object3D[] = [];

  constructor(scene: THREE.Scene) {
    this.scene = scene;
  }

  /**
   * Enable or disable frustum culling.
   */
  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (!enabled) {
      // Show all objects
      for (const obj of this.cullableObjects) {
        obj.visible = true;
      }
    }
  }

  /**
   * Rebuild the octree from the current scene.
   * Call this when models are added or removed.
   */
  rebuild(): void {
    this.cullableObjects = [];

    // Collect all meshes/points from the scene
    this.scene.traverse((child) => {
      if (
        (child instanceof THREE.Mesh || child instanceof THREE.Points) &&
        !child.name.startsWith('section-box') &&
        !(child.parent instanceof THREE.GridHelper) &&
        !child.userData.isGrid
      ) {
        this.cullableObjects.push(child);
      }
    });

    // Build octree from scene bounds
    const bounds = new THREE.Box3().setFromObject(this.scene);
    this.octree.build(bounds, this.cullableObjects);
  }

  /**
   * Update visibility based on camera frustum.
   * Call this every frame when culling is enabled.
   */
  update(camera: THREE.Camera): void {
    if (!this.enabled || this.cullableObjects.length === 0) return;

    // Query octree for visible objects
    const visible = new Set(this.octree.queryFrustum(camera));

    // Toggle visibility
    for (const obj of this.cullableObjects) {
      obj.visible = visible.has(obj);
    }
  }

  /**
   * Get culling stats.
   */
  getStats(): { total: number; visible: number; culled: number } {
    const visible = this.cullableObjects.filter((o) => o.visible).length;
    return {
      total: this.cullableObjects.length,
      visible,
      culled: this.cullableObjects.length - visible,
    };
  }

  dispose(): void {
    this.octree.clear();
    this.cullableObjects = [];
  }
}
