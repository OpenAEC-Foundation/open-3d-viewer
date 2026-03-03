import * as THREE from 'three';

/**
 * GeometryPool — Reuses Three.js BufferGeometry objects
 * to reduce garbage collection pressure on large scenes.
 *
 * Pools geometries by attribute signature (position count, has normals, etc.)
 * so compatible geometries can be recycled.
 */

interface PoolEntry {
  geometry: THREE.BufferGeometry;
  signature: string;
  inUse: boolean;
}

export class GeometryPool {
  private pool: PoolEntry[] = [];
  private maxPoolSize: number;

  constructor(maxPoolSize = 100) {
    this.maxPoolSize = maxPoolSize;
  }

  /**
   * Acquire a BufferGeometry from the pool or create a new one.
   * Returns a geometry with pre-allocated attributes matching the requested sizes.
   */
  acquire(
    vertexCount: number,
    hasNormals: boolean,
    hasUVs: boolean,
    hasColors: boolean,
    indexCount?: number
  ): THREE.BufferGeometry {
    const sig = this.makeSignature(vertexCount, hasNormals, hasUVs, hasColors, indexCount);

    // Try to find a matching unused geometry
    for (const entry of this.pool) {
      if (!entry.inUse && entry.signature === sig) {
        entry.inUse = true;
        return entry.geometry;
      }
    }

    // Create new geometry
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(new Float32Array(vertexCount * 3), 3));

    if (hasNormals) {
      geometry.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(vertexCount * 3), 3));
    }
    if (hasUVs) {
      geometry.setAttribute('uv', new THREE.BufferAttribute(new Float32Array(vertexCount * 2), 2));
    }
    if (hasColors) {
      geometry.setAttribute('color', new THREE.BufferAttribute(new Float32Array(vertexCount * 3), 3));
    }
    if (indexCount) {
      geometry.setIndex(new THREE.BufferAttribute(new Uint32Array(indexCount), 1));
    }

    // Track in pool
    if (this.pool.length < this.maxPoolSize) {
      this.pool.push({ geometry, signature: sig, inUse: true });
    }

    return geometry;
  }

  /**
   * Release a geometry back to the pool for reuse.
   */
  release(geometry: THREE.BufferGeometry): void {
    const entry = this.pool.find((e) => e.geometry === geometry);
    if (entry) {
      entry.inUse = false;
    }
  }

  /**
   * Update an existing geometry's attribute data in-place.
   * More efficient than creating new geometry objects.
   */
  static updatePositions(
    geometry: THREE.BufferGeometry,
    positions: Float32Array,
    offset = 0,
    count?: number
  ): void {
    const attr = geometry.attributes.position as THREE.BufferAttribute;
    if (!attr) return;

    const updateCount = count ?? positions.length;
    attr.array.set(positions.subarray(0, updateCount), offset);
    attr.needsUpdate = true;

    // Update draw range if partial
    if (count !== undefined) {
      geometry.setDrawRange(0, Math.floor((offset + updateCount) / 3));
    }
  }

  /**
   * Dispose all pooled geometries.
   */
  dispose(): void {
    for (const entry of this.pool) {
      entry.geometry.dispose();
    }
    this.pool = [];
  }

  /**
   * Get pool stats.
   */
  getStats(): { total: number; inUse: number; available: number } {
    const inUse = this.pool.filter((e) => e.inUse).length;
    return {
      total: this.pool.length,
      inUse,
      available: this.pool.length - inUse,
    };
  }

  private makeSignature(
    vertexCount: number,
    hasNormals: boolean,
    hasUVs: boolean,
    hasColors: boolean,
    indexCount?: number
  ): string {
    return `${vertexCount}:${hasNormals ? 1 : 0}:${hasUVs ? 1 : 0}:${hasColors ? 1 : 0}:${indexCount ?? 0}`;
  }
}

/** Singleton */
export const geometryPool = new GeometryPool();
