import { useRef, useCallback } from 'react';
import { useAppStore } from '../../../state/appStore';

/** Supported 3D file extensions */
export const SUPPORTED_EXTENSIONS = [
  // Native Three.js loaders
  '.gltf', '.glb', '.obj', '.stl', '.fbx', '.3ds', '.dae',
  '.ply', '.pcd', '.3mf', '.amf', '.wrl', '.vrml',
  '.xyz', '.vtk', '.vtp', '.gcode', '.kmz', '.vox',
  // Custom loader
  '.skp',
  // Assimp WASM
  '.step', '.stp', '.dxf', '.igs', '.iges', '.ifc',
  '.x3d', '.blend', '.lwo', '.x', '.off',
];

export const FILE_INPUT_ACCEPT = SUPPORTED_EXTENSIONS.join(',');

export function useViewerActions() {
  const fileInputRef = useRef<HTMLInputElement>(null);

  const isTauri = !!(window as any).__TAURI_INTERNALS__;

  const handleImport = useCallback(async () => {
    if (isTauri) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const result = await open({
          multiple: true,
          filters: [{ name: '3D Files', extensions: SUPPORTED_EXTENSIONS.map(e => e.slice(1)) }],
        });
        if (!result) return;

        const files = Array.isArray(result) ? result : [result];

        for (const filePath of files) {
          const fileName = filePath.split(/[/\\]/).pop() || 'unknown';
          const ext = fileName.substring(fileName.lastIndexOf('.')).toLowerCase();
          const format = ext.replace('.', '').toUpperCase();

          const id = crypto.randomUUID();
          useAppStore.getState().addModel({
            id,
            fileName,
            filePath,
            format,
            vertices: 0,
            triangles: 0,
            bounds: { minX: 0, minY: 0, minZ: 0, maxX: 0, maxY: 0, maxZ: 0 },
            visible: true,
            loadingProgress: 0,
            loadingPhase: 'Reading file...',
          });

          useAppStore.getState().addLoadingFile(id, fileName);

          // Dispatch custom event for the viewer to pick up
          window.dispatchEvent(new CustomEvent('open3d:import', {
            detail: { id, filePath, fileName, format: ext }
          }));
        }
      } catch (err) {
        console.error('Import failed:', err);
      }
    } else {
      fileInputRef.current?.click();
    }
  }, [isTauri]);

  const handleFileInputChange = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files) return;

    for (const file of Array.from(files)) {
      const ext = file.name.substring(file.name.lastIndexOf('.')).toLowerCase();
      const format = ext.replace('.', '').toUpperCase();

      const id = crypto.randomUUID();
      useAppStore.getState().addModel({
        id,
        fileName: file.name,
        filePath: file.name,
        format,
        vertices: 0,
        triangles: 0,
        bounds: { minX: 0, minY: 0, minZ: 0, maxX: 0, maxY: 0, maxZ: 0 },
        visible: true,
        loadingProgress: 0,
        loadingPhase: 'Reading file...',
      });

      useAppStore.getState().addLoadingFile(id, file.name);

      // Dispatch custom event with the File object
      window.dispatchEvent(new CustomEvent('open3d:import-file', {
        detail: { id, file, format: ext }
      }));
    }

    e.target.value = '';
  }, []);

  return {
    fileInputRef,
    FILE_INPUT_ACCEPT,
    handleImport,
    handleFileInputChange,
  };
}
