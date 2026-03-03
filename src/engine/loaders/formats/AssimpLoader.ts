import * as THREE from 'three';
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

/**
 * AssimpLoader — Uses assimpjs (WASM) to convert any assimp-supported format
 * to GLB, then loads it with Three.js GLTFLoader.
 *
 * Works entirely in the browser — no Rust backend needed.
 *
 * Supported formats include: SKP, 3MF, AMF, BLEND, BVH, CSM, HMP,
 * IRR, LWO, LWS, MD2, MD3, MD5, MDC, MDL, MS3D, NDO, NFF, OFF, Q3D,
 * RAW, SIB, SMD, X, X3D, XGL, and many more.
 */
export async function loadWithAssimp(
  data: ArrayBuffer,
  fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  const ext = fileName.substring(fileName.lastIndexOf('.')).toLowerCase().replace('.', '').toUpperCase();

  onProgress?.(0.1, `${ext} laden via Assimp WASM...`);

  // Dynamically import assimpjs (WASM module)
  let ajs: any;
  try {
    const assimpjsModule = await import('assimpjs');
    // assimpjs CJS module: Vite wraps it — try various export shapes
    let factory: any = (assimpjsModule as any).default ?? assimpjsModule;
    // If Vite double-wraps: { default: { default: fn } }
    if (factory && typeof factory !== 'function' && typeof factory.default === 'function') {
      factory = factory.default;
    }
    if (typeof factory === 'function') {
      ajs = await factory();
    } else if (factory && typeof factory.FileList === 'function') {
      // Already initialized module
      ajs = factory;
    } else {
      throw new Error(`Onverwacht module formaat: ${typeof factory} (keys: ${Object.keys(factory || {}).join(',')})`);
    }
  } catch (err) {
    throw new Error(
      `Kan Assimp WASM niet laden: ${err instanceof Error ? err.message : String(err)}`
    );
  }

  onProgress?.(0.3, `${ext} converteren naar GLB...`);

  // Create file list and add our file
  let fileList: any;
  try {
    fileList = new ajs.FileList();
    fileList.AddFile(fileName, new Uint8Array(data));
  } catch (err) {
    throw new Error(
      `Kan bestand niet voorbereiden voor conversie: ${err instanceof Error ? err.message : String(err)}`
    );
  }

  // Convert to GLB2 (binary glTF 2.0)
  let result: any;
  try {
    result = ajs.ConvertFileList(fileList, 'glb2');
  } catch (err) {
    throw new Error(
      `Assimp conversie mislukt voor ${ext}: ${err instanceof Error ? err.message : String(err)}`
    );
  }

  if (!result.IsSuccess() || result.FileCount() === 0) {
    const errorCode = result.GetErrorCode ? result.GetErrorCode() : 'Onbekende fout';
    throw new Error(
      `Assimp kon ${ext} niet converteren: ${errorCode}`
    );
  }

  onProgress?.(0.6, 'GLB laden in Three.js...');

  // Get the GLB result
  const resultFile = result.GetFile(0);
  const glbContent = resultFile.GetContent();
  const glbBuffer = new Uint8Array(glbContent).buffer;

  // Load GLB with Three.js GLTFLoader
  const gltfLoader = new GLTFLoader();

  const gltf = await new Promise<any>((resolve, reject) => {
    gltfLoader.parse(
      glbBuffer,
      '',
      (result) => resolve(result),
      (error) => reject(new Error(
        `GLB parsing mislukt na Assimp conversie: ${error instanceof Error ? error.message : String(error)}`
      ))
    );
  });

  onProgress?.(0.9, 'Scene opbouwen...');

  const scene = gltf.scene as THREE.Group;

  // Count vertices and triangles
  let totalVertices = 0;
  let totalTriangles = 0;
  scene.traverse((child) => {
    if (child instanceof THREE.Mesh) {
      const geom = child.geometry as THREE.BufferGeometry;
      if (geom.attributes.position) {
        totalVertices += geom.attributes.position.count;
      }
      if (geom.index) {
        totalTriangles += geom.index.count / 3;
      } else if (geom.attributes.position) {
        totalTriangles += geom.attributes.position.count / 3;
      }
    }
  });

  const box = new THREE.Box3().setFromObject(scene);

  return {
    scene,
    totalVertices,
    totalTriangles,
    bounds: { min: box.min, max: box.max },
  };
}
