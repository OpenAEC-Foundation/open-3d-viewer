import type { LoaderResult, ProgressCallback } from './types';

export type LoaderFunction = (
  data: ArrayBuffer,
  fileName: string,
  onProgress?: ProgressCallback
) => Promise<LoaderResult>;

const registry = new Map<string, () => Promise<LoaderFunction>>();

/**
 * Register a lazy-loaded format loader.
 * The factory is only called when the format is actually needed.
 */
export function registerLoader(extensions: string[], factory: () => Promise<LoaderFunction>) {
  for (const ext of extensions) {
    registry.set(ext.toLowerCase(), factory);
  }
}

/**
 * Get the loader for a file extension.
 */
export async function getLoader(extension: string): Promise<LoaderFunction | null> {
  const factory = registry.get(extension.toLowerCase());
  if (!factory) return null;
  return factory();
}

/**
 * Check if a file extension is supported.
 */
export function isSupported(extension: string): boolean {
  return registry.has(extension.toLowerCase());
}

/**
 * Get all supported extensions.
 */
export function getSupportedExtensions(): string[] {
  return Array.from(registry.keys());
}

// ============================================================================
// Register all loaders
// ============================================================================

// GLTF / GLB
registerLoader(['.gltf', '.glb'], async () => {
  const { loadGLTF } = await import('./formats/GLTFLoader');
  return loadGLTF;
});

// OBJ
registerLoader(['.obj'], async () => {
  const { loadOBJ } = await import('./formats/OBJLoader');
  return loadOBJ;
});

// STL
registerLoader(['.stl'], async () => {
  const { loadSTL } = await import('./formats/STLLoader');
  return loadSTL;
});

// FBX
registerLoader(['.fbx'], async () => {
  const { loadFBX } = await import('./formats/FBXLoader');
  return loadFBX;
});

// PLY
registerLoader(['.ply'], async () => {
  const { loadPLY } = await import('./formats/PLYLoader');
  return loadPLY;
});

// 3DS
registerLoader(['.3ds'], async () => {
  const { load3DS } = await import('./formats/ThreeDSLoader');
  return load3DS;
});

// Collada (DAE)
registerLoader(['.dae'], async () => {
  const { loadCollada } = await import('./formats/ColladaLoader');
  return loadCollada;
});

// PCD
registerLoader(['.pcd'], async () => {
  const { loadPCD } = await import('./formats/PCDLoader');
  return loadPCD;
});

// SketchUp SKP (openskp: 2013–2026 files; legacy parser as fallback — works in browser)
registerLoader(['.skp'], async () => {
  const { loadSKP } = await import('./formats/SKPLoader');
  return loadSKP;
});

// STEP / IGES / BREP (OpenCascade WASM — exact B-rep tessellation, works in browser)
registerLoader(['.step', '.stp', '.iges', '.igs', '.brep', '.brp'], async () => {
  const { loadWithOCCT } = await import('./formats/OCCTLoader');
  return loadWithOCCT;
});

// Revit RVT (OpenAEC rvt reader in the Tauri backend — categories as layers; desktop app only)
registerLoader(['.rvt'], async () => {
  const { loadRVT } = await import('./formats/RVTLoader');
  return loadRVT;
});

// AutoCAD DWG (acadrust in the Tauri backend — 3D model space, layers as layers; desktop app only)
registerLoader(['.dwg'], async () => (await import('./formats/CadImportLoaders')).loadDWG);

// E57 point clouds (e57 crate in the Tauri backend — scans as layers; desktop app only)
registerLoader(['.e57'], async () => (await import('./formats/CadImportLoaders')).loadE57);

// IFC (web-ifc WASM — spatial classes as layers)
registerLoader(['.ifc'], async () => {
  const { loadIFC } = await import('./formats/IFCLoader');
  return loadIFC;
});

// Rhino 3DM
registerLoader(['.3dm'], async () => (await import('./formats/ExtraLoaders')).load3DM);

// Universal Scene Description
registerLoader(['.usd', '.usda', '.usdc', '.usdz'], async () => (await import('./formats/ExtraLoaders')).loadUSD);

// Draco compressed mesh / point cloud
registerLoader(['.drc'], async () => (await import('./formats/ExtraLoaders')).loadDRC);

// LightWave (native loader)
registerLoader(['.lwo'], async () => (await import('./formats/ExtraLoaders')).loadLWO);

// Molecules, game models, motion capture
registerLoader(['.pdb'], async () => (await import('./formats/ExtraLoaders')).loadPDB);
registerLoader(['.md2'], async () => (await import('./formats/ExtraLoaders')).loadMD2);
registerLoader(['.bvh'], async () => (await import('./formats/ExtraLoaders')).loadBVH);

// Point clouds
registerLoader(['.las'], async () => (await import('./formats/ExtraLoaders')).loadLAS);
registerLoader(['.pts', '.asc', '.csv'], async () => (await import('./formats/ExtraLoaders')).loadTextPoints);

// Archives (opens the largest supported model inside)
registerLoader(['.zip', '.ifczip'], async () => (await import('./formats/ExtraLoaders')).loadZIP);

// 3MF (native Three.js — better than Assimp route)
registerLoader(['.3mf'], async () => {
  const { load3MFNative } = await import('./formats/ThreeMFNativeLoader');
  return load3MFNative;
});

// AMF
registerLoader(['.amf'], async () => {
  const { loadAMF } = await import('./formats/AMFLoader');
  return loadAMF;
});

// VRML / WRL
registerLoader(['.wrl', '.vrml'], async () => {
  const { loadVRML } = await import('./formats/VRMLLoader');
  return loadVRML;
});

// XYZ point cloud
registerLoader(['.xyz'], async () => {
  const { loadXYZ } = await import('./formats/XYZLoader');
  return loadXYZ;
});

// VTK
registerLoader(['.vtk', '.vtp'], async () => {
  const { loadVTK } = await import('./formats/VTKLoader');
  return loadVTK;
});

// GCode
registerLoader(['.gcode'], async () => {
  const { loadGCode } = await import('./formats/GCodeLoader');
  return loadGCode;
});

// KMZ (Google Earth)
registerLoader(['.kmz'], async () => {
  const { loadKMZ } = await import('./formats/KMZLoader');
  return loadKMZ;
});

// VOX (MagicaVoxel)
registerLoader(['.vox'], async () => {
  const { loadVOX } = await import('./formats/VOXLoader');
  return loadVOX;
});

// Everything else Assimp can read
export const ASSIMP_EXTENSIONS = [
  '.x3d', '.x3db', '.blend', '.x', '.dxf', '.off', '.ac', '.ac3d', '.acc', '.ase', '.ask', '.b3d', '.cob', '.csm',
  '.hmp', '.irr', '.irrmesh', '.lws', '.lxo', '.md3', '.md5mesh', '.md5anim', '.mdc', '.mdl', '.mesh',
  '.ms3d', '.ndo', '.nff', '.ogex', '.pk3', '.bsp', '.pmx', '.q3o', '.q3s', '.raw', '.sib',
  '.smd', '.ter', '.xgl', '.zgl', '.3d', '.enff',
];
registerLoader(ASSIMP_EXTENSIONS, async () => {
  const { loadWithAssimp } = await import('./formats/AssimpLoader');
  return loadWithAssimp;
});
