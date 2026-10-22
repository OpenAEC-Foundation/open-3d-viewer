import { useRef, useCallback } from 'react';
import { useAppStore } from '../../../state/appStore';
import { t } from '../../../i18n/useTranslation';
import { getSupportedExtensions } from '../../../engine/loaders/LoaderRegistry';

/** Categorized 3D file format filters */
export const FILE_FILTERS = [
  { name: 'All 3D Files', extensions: [] as string[] }, // populated below
  { name: 'glTF / USD (*.gltf, *.glb, *.usd, *.usdz, *.drc)', extensions: ['gltf', 'glb', 'usd', 'usda', 'usdc', 'usdz', 'drc'] },
  { name: 'Mesh Formats (*.obj, *.stl, *.ply, *.off, *.3mf, *.amf)', extensions: ['obj', 'stl', 'ply', 'off', '3mf', 'amf'] },
  { name: 'CAD Formats (*.step, *.stp, *.iges, *.igs, *.brep, *.3dm, *.dxf)', extensions: ['step', 'stp', 'iges', 'igs', 'brep', 'brp', '3dm', 'dxf'] },
  { name: 'BIM (*.ifc, *.ifczip)', extensions: ['ifc', 'ifczip'] },
  { name: 'Scene Formats (*.fbx, *.3ds, *.dae, *.x3d, *.vrml, *.wrl, *.blend)', extensions: ['fbx', '3ds', 'dae', 'x3d', 'x3db', 'vrml', 'wrl', 'blend', 'lwo', 'lws', 'lxo', 'x', 'ac', 'ase', 'cob', 'ms3d', 'b3d'] },
  { name: 'Point Cloud (*.las, *.pcd, *.xyz, *.pts, *.asc, *.csv)', extensions: ['las', 'pcd', 'xyz', 'pts', 'asc', 'csv'] },
  { name: 'Game Formats (*.md2, *.md3, *.md5mesh, *.mdl, *.smd, *.bsp, *.pmx)', extensions: ['md2', 'md3', 'md5mesh', 'mdl', 'mdc', 'smd', 'bsp', 'pk3', 'pmx', 'hmp'] },
  { name: 'Voxel (*.vox)', extensions: ['vox'] },
  { name: 'Scientific (*.vtk, *.vtp, *.pdb)', extensions: ['vtk', 'vtp', 'pdb'] },
  { name: '3D Printing (*.gcode)', extensions: ['gcode'] },
  { name: 'SketchUp (*.skp)', extensions: ['skp'] },
  { name: 'Google Earth (*.kmz)', extensions: ['kmz'] },
  { name: 'Archives (*.zip)', extensions: ['zip'] },
];

// "All 3D Files" lists every extension the loader registry knows about
FILE_FILTERS[0].extensions = getSupportedExtensions().map(e => e.replace(/^\./, ''));

/** Flat list of all supported extensions (with leading dot) */
export const SUPPORTED_EXTENSIONS = getSupportedExtensions();

export const FILE_INPUT_ACCEPT = SUPPORTED_EXTENSIONS.join(',');

export function useViewerActions() {
  const fileInputRef = useRef<HTMLInputElement>(null);

  const fallbackToFileInput = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleImport = useCallback(async () => {
    let tauriRuntime = false;
    try {
      const { isTauri } = await import('@tauri-apps/api/core');
      tauriRuntime = isTauri();
    } catch {
      tauriRuntime = false;
    }

    if (!tauriRuntime) {
      fallbackToFileInput();
      return;
    }

    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const result = await open({
        multiple: true,
        filters: FILE_FILTERS,
      });
      if (!result) return;

      const files = Array.isArray(result) ? result : [result];

      for (const filePath of files) {
        if (typeof filePath !== 'string') continue;

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
          loadingPhase: t('loader:readingFilePhase'),
        });

        useAppStore.getState().addLoadingFile(id, fileName);

        // Dispatch custom event for the viewer to pick up
        window.dispatchEvent(new CustomEvent('open3d:import', {
          detail: { id, filePath, fileName, format: ext }
        }));
      }
    } catch (err) {
      console.error('Import failed, falling back to file input:', err);
      fallbackToFileInput();
    }
  }, [fallbackToFileInput]);

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
        loadingPhase: t('loader:readingFilePhase'),
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
