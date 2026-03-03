import * as THREE from 'three';

/**
 * Intermediate model representation from any loader.
 * Loaders convert their native format into this, then SceneManager converts to Three.js objects.
 */
export interface ParsedModel {
  /** Unique model ID */
  id: string;
  /** Source file name */
  fileName: string;
  /** Format identifier (GLTF, OBJ, STL, etc.) */
  format: string;
  /** Parsed geometry groups (a model can contain multiple meshes) */
  geometries: ParsedGeometry[];
  /** Bounding box of entire model */
  bounds: {
    min: THREE.Vector3;
    max: THREE.Vector3;
  };
  /** Total vertex count */
  totalVertices: number;
  /** Total triangle count */
  totalTriangles: number;
}

export interface ParsedGeometry {
  /** Optional name */
  name?: string;
  /** Vertex positions (Float32Array, xyz interleaved) */
  positions: Float32Array;
  /** Triangle indices (Uint32Array, optional - if absent, non-indexed geometry) */
  indices?: Uint32Array;
  /** Vertex normals (Float32Array, xyz interleaved, optional) */
  normals?: Float32Array;
  /** Vertex UVs (Float32Array, uv interleaved, optional) */
  uvs?: Float32Array;
  /** Vertex colors (Float32Array, rgb interleaved, optional) */
  colors?: Float32Array;
  /** Material for this geometry */
  material?: ParsedMaterial;
}

export interface ParsedMaterial {
  name?: string;
  color?: [number, number, number];
  opacity?: number;
  metalness?: number;
  roughness?: number;
  /** Base color texture URL or data URI */
  map?: string;
  doubleSided?: boolean;
}

/**
 * Direct Three.js result - used by loaders that return Three.js objects directly
 * (like GLTFLoader, FBXLoader, etc.)
 */
export interface ThreeJSResult {
  /** The root scene/group */
  scene: THREE.Group | THREE.Scene;
  /** Total vertex count */
  totalVertices: number;
  /** Total triangle count */
  totalTriangles: number;
  /** Bounding box */
  bounds: {
    min: THREE.Vector3;
    max: THREE.Vector3;
  };
}

export type LoaderResult = ParsedModel | ThreeJSResult;

export function isThreeJSResult(result: LoaderResult): result is ThreeJSResult {
  return 'scene' in result;
}

export type ProgressCallback = (progress: number, phase: string) => void;
