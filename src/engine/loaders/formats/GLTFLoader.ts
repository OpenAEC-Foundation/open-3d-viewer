import * as THREE from 'three';
import { GLTFLoader as ThreeGLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js';
import { DRACOLoader } from 'three/examples/jsm/loaders/DRACOLoader.js';
import type { ThreeJSResult, ProgressCallback } from '../types';

let dracoLoader: DRACOLoader | null = null;

function getDracoLoader(): DRACOLoader {
  if (!dracoLoader) {
    dracoLoader = new DRACOLoader();
    dracoLoader.setDecoderPath('https://www.gstatic.com/draco/versioned/decoders/1.5.7/');
    dracoLoader.setDecoderConfig({ type: 'js' });
  }
  return dracoLoader;
}

export async function loadGLTF(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback
): Promise<ThreeJSResult> {
  onProgress?.(0.3, 'Parsing GLTF...');

  const loader = new ThreeGLTFLoader();
  loader.setDRACOLoader(getDracoLoader());

  return new Promise((resolve, reject) => {
    loader.parse(
      data,
      '',
      (gltf) => {
        onProgress?.(0.8, 'Processing scene...');

        const scene = gltf.scene;
        let totalVertices = 0;
        let totalTriangles = 0;

        scene.traverse((child) => {
          if (child instanceof THREE.Mesh) {
            const geo = child.geometry;
            totalVertices += geo.attributes.position ? geo.attributes.position.count : 0;
            totalTriangles += geo.index ? geo.index.count / 3 : (geo.attributes.position ? geo.attributes.position.count / 3 : 0);
          }
        });

        const box = new THREE.Box3().setFromObject(scene);

        resolve({
          scene: scene as THREE.Group,
          totalVertices,
          totalTriangles,
          bounds: { min: box.min, max: box.max },
        });
      },
      (error) => {
        reject(new Error(`GLTF parse error: ${error.message}`));
      }
    );
  });
}
