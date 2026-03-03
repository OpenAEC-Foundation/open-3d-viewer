import { getLoader, isSupported } from './LoaderRegistry';
import { StreamReader } from './StreamReader';
import type { LoaderResult, ProgressCallback } from './types';

/**
 * Main entry point for loading 3D models.
 * Reads a file (File object or ArrayBuffer), detects format, and dispatches to the right loader.
 * Uses StreamReader for chunked reading of large files.
 */
export class ModelLoader {
  private streamReader = new StreamReader();

  /**
   * Load a 3D model from a File object (browser drag-drop or file input).
   */
  async loadFile(file: File, onProgress?: ProgressCallback): Promise<LoaderResult> {
    const ext = this.getExtension(file.name);

    if (!isSupported(ext)) {
      throw new Error(
        `Niet-ondersteund formaat: ${ext}\n` +
        `Ondersteunde formaten: GLB, GLTF, OBJ, STL, FBX, PLY, 3DS, DAE, PCD, STEP, STP, SKP, 3MF, X3D, BLEND`
      );
    }

    onProgress?.(0, `${file.name} lezen...`);

    let buffer: ArrayBuffer;
    try {
      buffer = await this.streamReader.readFile(file, (_chunk, offset, total) => {
        const readProgress = StreamReader.progress(offset + _chunk.byteLength, total);
        onProgress?.(readProgress * 0.15, `Bestand lezen... ${Math.round(readProgress * 100)}%`);
      });
    } catch (err) {
      throw new Error(
        `Kan bestand niet lezen: ${file.name}\n` +
        `${err instanceof Error ? err.message : String(err)}`
      );
    }

    return this.loadBuffer(buffer, file.name, onProgress);
  }

  /**
   * Load a 3D model from an ArrayBuffer (e.g., from Tauri fs).
   */
  async loadBuffer(
    buffer: ArrayBuffer,
    fileName: string,
    onProgress?: ProgressCallback
  ): Promise<LoaderResult> {
    const ext = this.getExtension(fileName);

    onProgress?.(0.15, 'Loader ophalen...');

    let loader;
    try {
      loader = await getLoader(ext);
    } catch (err) {
      throw new Error(
        `Kan loader niet laden voor ${ext}: ${err instanceof Error ? err.message : String(err)}`
      );
    }

    if (!loader) {
      throw new Error(`Geen loader beschikbaar voor: ${ext}`);
    }

    onProgress?.(0.2, 'Parseren...');

    let result: LoaderResult;
    try {
      result = await loader(buffer, fileName, onProgress);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      // Re-throw with file context if not already included
      if (msg.includes(fileName) || msg.includes(ext.replace('.', '').toUpperCase())) {
        throw err;
      }
      throw new Error(`Fout bij laden van ${fileName}: ${msg}`);
    }

    onProgress?.(1, 'Klaar');
    return result;
  }

  private getExtension(fileName: string): string {
    const dotIdx = fileName.lastIndexOf('.');
    if (dotIdx === -1) return '';
    return fileName.substring(dotIdx).toLowerCase();
  }
}

/** Singleton instance */
export const modelLoader = new ModelLoader();
