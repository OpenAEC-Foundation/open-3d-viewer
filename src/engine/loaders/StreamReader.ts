/**
 * StreamReader — Reads large files in chunks to avoid loading entire
 * files into memory at once. Supports both browser File objects and
 * Tauri filesystem paths.
 *
 * Default chunk size: 16MB
 */

export const DEFAULT_CHUNK_SIZE = 16 * 1024 * 1024; // 16MB

export type ChunkCallback = (chunk: ArrayBuffer, offset: number, total: number) => void;

export class StreamReader {
  private chunkSize: number;

  constructor(chunkSize = DEFAULT_CHUNK_SIZE) {
    this.chunkSize = chunkSize;
  }

  /**
   * Read a browser File in chunks.
   * Calls onChunk for each chunk; returns full concatenated buffer.
   */
  async readFile(
    file: File,
    onChunk?: ChunkCallback
  ): Promise<ArrayBuffer> {
    const total = file.size;

    // Small files: read in one go
    if (total <= this.chunkSize) {
      const buffer = await file.arrayBuffer();
      onChunk?.(buffer, 0, total);
      return buffer;
    }

    // Large files: read in chunks and concatenate
    const result = new Uint8Array(total);
    let offset = 0;

    while (offset < total) {
      const end = Math.min(offset + this.chunkSize, total);
      const blob = file.slice(offset, end);
      const chunk = await blob.arrayBuffer();
      result.set(new Uint8Array(chunk), offset);
      onChunk?.(chunk, offset, total);
      offset = end;
    }

    return result.buffer;
  }

  /**
   * Read a file via Tauri filesystem in chunks.
   */
  async readTauriFile(
    filePath: string,
    onChunk?: ChunkCallback
  ): Promise<ArrayBuffer> {
    const { readFile, stat } = await import('@tauri-apps/plugin-fs');

    const info = await stat(filePath);
    const total = info.size;

    // Small files: read in one go
    if (total <= this.chunkSize) {
      const bytes = await readFile(filePath);
      const buffer = bytes.buffer as ArrayBuffer;
      onChunk?.(buffer, 0, total);
      return buffer;
    }

    // Large files: read entire (Tauri plugin-fs doesn't support range reads)
    // but report progress in simulated chunks
    const bytes = await readFile(filePath);
    const buffer = bytes.buffer as ArrayBuffer;

    // Report progress in chunks for UI feedback
    if (onChunk) {
      let offset = 0;
      while (offset < total) {
        const end = Math.min(offset + this.chunkSize, total);
        onChunk(buffer.slice(offset, end), offset, total);
        offset = end;
      }
    }

    return buffer;
  }

  /**
   * Calculate progress as a fraction [0, 1] for a given offset.
   */
  static progress(offset: number, total: number): number {
    if (total === 0) return 1;
    return Math.min(offset / total, 1);
  }
}

/** Singleton */
export const streamReader = new StreamReader();
