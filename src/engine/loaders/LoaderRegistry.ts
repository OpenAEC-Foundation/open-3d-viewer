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

// SketchUp SKP (custom reverse-engineered parser — works in browser)
registerLoader(['.skp'], async () => {
  const { loadSKP } = await import('./formats/SKPLoader');
  return loadSKP;
});

// STEP/STP (via Assimp WASM — works in browser)
registerLoader(['.step', '.stp'], async () => {
  const { loadWithAssimp } = await import('./formats/AssimpLoader');
  return loadWithAssimp;
});

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

// IFC (native web-ifc WASM parser)
registerLoader(['.ifc'], async () => {
  const { loadIFC } = await import('./formats/IFCNativeLoader');
  return loadIFC;
});

// Additional Assimp-supported formats (DXF, IGES, BLEND, X3D, LWO, etc.)
registerLoader(['.x3d', '.blend', '.lwo', '.x', '.dxf', '.igs', '.iges', '.off'], async () => {
  const { loadWithAssimp } = await import('./formats/AssimpLoader');
  return loadWithAssimp;
});
