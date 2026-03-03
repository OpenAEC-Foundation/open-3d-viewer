import * as THREE from 'three';

/**
 * MemoryBudget — Tracks GPU and CPU memory usage.
 *
 * Provides warnings when approaching limits and can evict
 * least-recently-used geometry when over budget.
 */

export interface MemoryUsage {
  /** Estimated GPU memory in bytes */
  gpuBytes: number;
  /** Estimated CPU memory in bytes */
  cpuBytes: number;
  /** GPU memory budget in bytes */
  gpuBudget: number;
  /** Fraction of GPU budget used (0-1+) */
  gpuUsageRatio: number;
}

interface TrackedGeometry {
  id: string;
  geometry: THREE.BufferGeometry;
  sizeBytes: number;
  lastAccessTime: number;
}

export class MemoryBudget {
  /** Default GPU budget: 2GB */
  private gpuBudget: number;
  private tracked = new Map<string, TrackedGeometry>();
  private totalGpuBytes = 0;
  private totalCpuBytes = 0;

  constructor(gpuBudgetMB = 2048) {
    this.gpuBudget = gpuBudgetMB * 1024 * 1024;
  }

  /**
   * Set the GPU memory budget in MB.
   */
  setBudget(mb: number): void {
    this.gpuBudget = mb * 1024 * 1024;
  }

  /**
   * Track a geometry's memory usage.
   */
  track(id: string, geometry: THREE.BufferGeometry): void {
    const size = this.estimateGeometrySize(geometry);
    this.tracked.set(id, {
      id,
      geometry,
      sizeBytes: size,
      lastAccessTime: performance.now(),
    });
    this.totalGpuBytes += size;
    this.totalCpuBytes += size; // CPU copy exists too
  }

  /**
   * Mark geometry as recently accessed (for LRU eviction).
   */
  touch(id: string): void {
    const entry = this.tracked.get(id);
    if (entry) {
      entry.lastAccessTime = performance.now();
    }
  }

  /**
   * Untrack a geometry (when it's been disposed).
   */
  untrack(id: string): void {
    const entry = this.tracked.get(id);
    if (!entry) return;
    this.totalGpuBytes -= entry.sizeBytes;
    this.totalCpuBytes -= entry.sizeBytes;
    this.tracked.delete(id);
  }

  /**
   * Get current memory usage info.
   */
  getUsage(): MemoryUsage {
    return {
      gpuBytes: this.totalGpuBytes,
      cpuBytes: this.totalCpuBytes,
      gpuBudget: this.gpuBudget,
      gpuUsageRatio: this.gpuBudget > 0 ? this.totalGpuBytes / this.gpuBudget : 0,
    };
  }

  /**
   * Check if we're over budget.
   */
  isOverBudget(): boolean {
    return this.totalGpuBytes > this.gpuBudget;
  }

  /**
   * Check if we're approaching the budget (>80%).
   */
  isNearBudget(): boolean {
    return this.totalGpuBytes > this.gpuBudget * 0.8;
  }

  /**
   * Get geometries to evict (LRU order) to get back within budget.
   * Returns IDs of geometries to evict.
   */
  getEvictionCandidates(targetFreeBytes?: number): string[] {
    const target = targetFreeBytes ?? (this.totalGpuBytes - this.gpuBudget);
    if (target <= 0) return [];

    // Sort by last access time (oldest first)
    const sorted = Array.from(this.tracked.values())
      .sort((a, b) => a.lastAccessTime - b.lastAccessTime);

    const candidates: string[] = [];
    let freed = 0;

    for (const entry of sorted) {
      if (freed >= target) break;
      candidates.push(entry.id);
      freed += entry.sizeBytes;
    }

    return candidates;
  }

  /**
   * Estimate the GPU memory size of a BufferGeometry.
   */
  private estimateGeometrySize(geometry: THREE.BufferGeometry): number {
    let size = 0;

    for (const name in geometry.attributes) {
      const attr = geometry.attributes[name];
      if (attr instanceof THREE.BufferAttribute) {
        size += attr.array.byteLength;
      } else if (attr instanceof THREE.InterleavedBufferAttribute) {
        // Only count once per interleaved buffer
        size += attr.data.array.byteLength;
      }
    }

    if (geometry.index) {
      size += geometry.index.array.byteLength;
    }

    return size;
  }

  /**
   * Get total tracked geometry count.
   */
  get trackedCount(): number {
    return this.tracked.size;
  }

  /**
   * Format bytes as human-readable string.
   */
  static formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }
}

/** Singleton */
export const memoryBudget = new MemoryBudget();
