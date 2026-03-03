import * as THREE from 'three';

/**
 * LODManager — Distance-based Level of Detail management.
 *
 * Manages multiple detail levels for models and switches between them
 * based on camera distance. Uses Three.js LOD objects internally.
 */

export interface LODLevel {
  /** Distance threshold at which this level activates */
  distance: number;
  /** The Three.js object for this detail level */
  object: THREE.Object3D;
  /** Approximate vertex count for memory tracking */
  vertexCount: number;
}

interface LODEntry {
  id: string;
  lod: THREE.LOD;
  levels: LODLevel[];
}

export class LODManager {
  private entries = new Map<string, LODEntry>();
  private scene: THREE.Scene;

  constructor(scene: THREE.Scene) {
    this.scene = scene;
  }

  /**
   * Register a model with multiple LOD levels.
   */
  addModel(id: string, levels: LODLevel[]): THREE.LOD {
    // Sort by distance ascending
    const sorted = [...levels].sort((a, b) => a.distance - b.distance);

    const lod = new THREE.LOD();
    for (const level of sorted) {
      lod.addLevel(level.object, level.distance);
    }

    this.entries.set(id, { id, lod, levels: sorted });
    this.scene.add(lod);
    return lod;
  }

  /**
   * Remove an LOD model.
   */
  removeModel(id: string): void {
    const entry = this.entries.get(id);
    if (!entry) return;

    this.scene.remove(entry.lod);

    // Dispose geometry and materials
    for (const level of entry.levels) {
      level.object.traverse((child) => {
        if (child instanceof THREE.Mesh) {
          child.geometry.dispose();
          const materials = Array.isArray(child.material) ? child.material : [child.material];
          materials.forEach((m) => m.dispose());
        }
      });
    }

    this.entries.delete(id);
  }

  /**
   * Update all LOD objects based on camera position.
   * Call this each frame.
   */
  update(camera: THREE.Camera): void {
    for (const entry of this.entries.values()) {
      entry.lod.update(camera);
    }
  }

  /**
   * Get the currently active vertex count (based on visible LOD levels).
   */
  getActiveVertexCount(): number {
    let total = 0;
    for (const entry of this.entries.values()) {
      // LOD.getCurrentLevel() returns the index of the active level
      const currentLevel = entry.lod.getCurrentLevel();
      if (currentLevel < entry.levels.length) {
        total += entry.levels[currentLevel].vertexCount;
      }
    }
    return total;
  }

  /**
   * Generate simplified LOD levels from a mesh using vertex decimation.
   * Returns a list of LOD levels with decreasing detail.
   */
  static generateLevels(
    original: THREE.Object3D,
    vertexCount: number,
    maxDistance: number
  ): LODLevel[] {
    const levels: LODLevel[] = [
      { distance: 0, object: original, vertexCount },
    ];

    // Medium LOD at 50% of max distance, simplified geometry
    const mediumGroup = LODManager.simplifyObject(original, 0.5);
    if (mediumGroup) {
      levels.push({
        distance: maxDistance * 0.3,
        object: mediumGroup,
        vertexCount: Math.floor(vertexCount * 0.5),
      });
    }

    // Low LOD at max distance, bounding box only
    const lowGroup = LODManager.createBBoxProxy(original);
    levels.push({
      distance: maxDistance * 0.7,
      object: lowGroup,
      vertexCount: 36, // bbox = 12 triangles = 36 vertices
    });

    return levels;
  }

  /**
   * Create a simplified version of an object by reducing geometry.
   * Uses stride-based vertex decimation (simple but fast).
   */
  private static simplifyObject(
    source: THREE.Object3D,
    ratio: number
  ): THREE.Group | null {
    const group = new THREE.Group();
    let hasMeshes = false;

    source.traverse((child) => {
      if (!(child instanceof THREE.Mesh)) return;
      hasMeshes = true;

      const geom = child.geometry as THREE.BufferGeometry;
      const positions = geom.attributes.position;
      if (!positions) return;

      const stride = Math.max(2, Math.round(1 / ratio));
      const newCount = Math.ceil(positions.count / stride);
      const newPositions = new Float32Array(newCount * 3);

      for (let i = 0, j = 0; i < positions.count && j < newCount; i += stride, j++) {
        newPositions[j * 3] = positions.getX(i);
        newPositions[j * 3 + 1] = positions.getY(i);
        newPositions[j * 3 + 2] = positions.getZ(i);
      }

      const newGeom = new THREE.BufferGeometry();
      newGeom.setAttribute('position', new THREE.BufferAttribute(newPositions, 3));
      newGeom.computeVertexNormals();

      const mat = (child.material as THREE.Material).clone();
      const mesh = new THREE.Mesh(newGeom, mat);
      mesh.position.copy(child.position);
      mesh.rotation.copy(child.rotation);
      mesh.scale.copy(child.scale);
      group.add(mesh);
    });

    return hasMeshes ? group : null;
  }

  /**
   * Create a bounding box proxy for an object (lowest LOD).
   */
  private static createBBoxProxy(source: THREE.Object3D): THREE.Group {
    const box = new THREE.Box3().setFromObject(source);
    const size = box.getSize(new THREE.Vector3());
    const center = box.getCenter(new THREE.Vector3());

    const geom = new THREE.BoxGeometry(size.x, size.y, size.z);
    const mat = new THREE.MeshStandardMaterial({
      color: 0x888888,
      opacity: 0.3,
      transparent: true,
      wireframe: true,
    });

    const mesh = new THREE.Mesh(geom, mat);
    mesh.position.copy(center);

    const group = new THREE.Group();
    group.add(mesh);
    return group;
  }

  dispose(): void {
    for (const id of Array.from(this.entries.keys())) {
      this.removeModel(id);
    }
  }
}
