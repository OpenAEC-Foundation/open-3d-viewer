import * as THREE from 'three';
import { memoryBudget } from '../memory/MemoryBudget';

/**
 * IncrementalSceneBuilder — Progressively adds geometry to the scene
 * as it's loaded, providing visual feedback during large file parsing.
 *
 * Instead of waiting for a full model to load, geometry batches are
 * added to the scene as soon as they're available.
 */

export interface GeometryBatch {
  /** Unique batch identifier */
  batchIndex: number;
  /** Vertex positions (Float32Array, xyz interleaved) */
  positions: Float32Array;
  /** Optional triangle indices */
  indices?: Uint32Array;
  /** Optional normals */
  normals?: Float32Array;
  /** Optional vertex colors */
  colors?: Float32Array;
  /** Optional UVs */
  uvs?: Float32Array;
  /** Is this the last batch? */
  isLast: boolean;
  /** Overall progress (0-1) */
  totalProgress: number;
}

export class IncrementalSceneBuilder {
  private scene: THREE.Scene;
  private group: THREE.Group;
  private meshes: THREE.Mesh[] = [];
  private boundingBox = new THREE.Box3();
  private totalVertices = 0;
  private totalTriangles = 0;
  private defaultMaterial: THREE.MeshStandardMaterial;

  constructor(scene: THREE.Scene, modelId: string) {
    this.scene = scene;
    this.group = new THREE.Group();
    this.group.name = `model-${modelId}`;
    this.scene.add(this.group);

    this.defaultMaterial = new THREE.MeshStandardMaterial({
      color: 0x888888,
      metalness: 0.1,
      roughness: 0.7,
      side: THREE.DoubleSide,
    });
  }

  /**
   * Add a geometry batch to the scene.
   */
  addBatch(batch: GeometryBatch): void {
    const geometry = new THREE.BufferGeometry();

    // Positions
    geometry.setAttribute(
      'position',
      new THREE.BufferAttribute(batch.positions, 3)
    );

    // Indices
    if (batch.indices) {
      geometry.setIndex(new THREE.BufferAttribute(batch.indices, 1));
    }

    // Normals
    if (batch.normals) {
      geometry.setAttribute(
        'normal',
        new THREE.BufferAttribute(batch.normals, 3)
      );
    } else {
      geometry.computeVertexNormals();
    }

    // UVs
    if (batch.uvs) {
      geometry.setAttribute(
        'uv',
        new THREE.BufferAttribute(batch.uvs, 2)
      );
    }

    // Colors
    if (batch.colors) {
      geometry.setAttribute(
        'color',
        new THREE.BufferAttribute(batch.colors, 3)
      );
      this.defaultMaterial.vertexColors = true;
    }

    // Track memory
    memoryBudget.track(`batch-${batch.batchIndex}`, geometry);

    // Create mesh
    const mesh = new THREE.Mesh(geometry, this.defaultMaterial);
    this.group.add(mesh);
    this.meshes.push(mesh);

    // Update stats
    const vertexCount = batch.positions.length / 3;
    const triangleCount = batch.indices
      ? batch.indices.length / 3
      : vertexCount / 3;

    this.totalVertices += vertexCount;
    this.totalTriangles += triangleCount;

    // Expand bounding box
    geometry.computeBoundingBox();
    if (geometry.boundingBox) {
      this.boundingBox.union(geometry.boundingBox);
    }
  }

  /**
   * Get the current bounding box (grows as batches are added).
   */
  getBounds(): THREE.Box3 {
    return this.boundingBox.clone();
  }

  /**
   * Get the Three.js group containing all batches.
   */
  getGroup(): THREE.Group {
    return this.group;
  }

  /**
   * Get current vertex count.
   */
  getVertexCount(): number {
    return this.totalVertices;
  }

  /**
   * Get current triangle count.
   */
  getTriangleCount(): number {
    return this.totalTriangles;
  }

  /**
   * Finalize the model — merge small geometries, compute final bounds.
   */
  finalize(): { totalVertices: number; totalTriangles: number; bounds: THREE.Box3 } {
    // Recompute bounds from object (more accurate)
    this.boundingBox.setFromObject(this.group);

    return {
      totalVertices: this.totalVertices,
      totalTriangles: this.totalTriangles,
      bounds: this.boundingBox.clone(),
    };
  }

  /**
   * Dispose all geometry and remove from scene.
   */
  dispose(): void {
    for (let i = 0; i < this.meshes.length; i++) {
      const mesh = this.meshes[i];
      mesh.geometry.dispose();
      memoryBudget.untrack(`batch-${i}`);
    }
    this.defaultMaterial.dispose();
    this.scene.remove(this.group);
    this.meshes = [];
  }
}
