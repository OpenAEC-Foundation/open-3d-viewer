import * as THREE from 'three';
import * as WebIFC from 'web-ifc';
import type { LoaderResult, ProgressCallback } from '../types';

/**
 * IFC loader using web-ifc WASM engine.
 * Extracts mesh geometry directly from IFC data.
 */
export async function loadIFC(
  data: ArrayBuffer,
  _fileName: string,
  onProgress?: ProgressCallback,
): Promise<LoaderResult> {
  onProgress?.(0, 'Initialising IFC engine...');

  const ifcAPI = new WebIFC.IfcAPI();
  ifcAPI.SetWasmPath('/');
  await ifcAPI.Init();

  onProgress?.(10, 'Parsing IFC model...');

  const modelID = ifcAPI.OpenModel(new Uint8Array(data));

  const root = new THREE.Group();
  root.name = 'IFC Model';

  let totalVertices = 0;
  let totalTriangles = 0;
  const bounds = new THREE.Box3();

  // Get all mesh geometry
  onProgress?.(30, 'Extracting geometry...');

  ifcAPI.StreamAllMeshes(modelID, (mesh: WebIFC.FlatMesh) => {
    const placedGeometries = mesh.geometries;

    for (let i = 0; i < placedGeometries.size(); i++) {
      const placedGeom = placedGeometries.get(i);
      const geomData = ifcAPI.GetGeometry(modelID, placedGeom.geometryExpressID);

      const vertexData = ifcAPI.GetVertexArray(geomData.GetVertexData(), geomData.GetVertexDataSize());
      const indexData = ifcAPI.GetIndexArray(geomData.GetIndexData(), geomData.GetIndexDataSize());

      if (vertexData.length === 0 || indexData.length === 0) {
        geomData.delete();
        continue;
      }

      // web-ifc vertex format: x, y, z, nx, ny, nz (6 floats per vertex)
      const vertCount = vertexData.length / 6;
      const positions = new Float32Array(vertCount * 3);
      const normals = new Float32Array(vertCount * 3);

      for (let v = 0; v < vertCount; v++) {
        positions[v * 3] = vertexData[v * 6];
        positions[v * 3 + 1] = vertexData[v * 6 + 1];
        positions[v * 3 + 2] = vertexData[v * 6 + 2];
        normals[v * 3] = vertexData[v * 6 + 3];
        normals[v * 3 + 1] = vertexData[v * 6 + 4];
        normals[v * 3 + 2] = vertexData[v * 6 + 5];
      }

      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
      geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
      geometry.setIndex(new THREE.BufferAttribute(new Uint32Array(indexData), 1));

      // Apply placement transform
      const flatMatrix = placedGeom.flatTransformation;
      const matrix = new THREE.Matrix4();
      matrix.fromArray(flatMatrix);

      // Material from color
      const color = placedGeom.color;
      const material = new THREE.MeshStandardMaterial({
        color: new THREE.Color(color.x, color.y, color.z),
        opacity: color.w,
        transparent: color.w < 1.0,
        side: THREE.DoubleSide,
        roughness: 0.7,
        metalness: 0.1,
      });

      const mesh3d = new THREE.Mesh(geometry, material);
      mesh3d.applyMatrix4(matrix);
      mesh3d.updateMatrixWorld(true);

      root.add(mesh3d);

      totalVertices += vertCount;
      totalTriangles += indexData.length / 3;

      // Expand bounds
      geometry.computeBoundingBox();
      if (geometry.boundingBox) {
        const b = geometry.boundingBox.clone();
        b.applyMatrix4(matrix);
        bounds.union(b);
      }

      geomData.delete();
    }
  });

  ifcAPI.CloseModel(modelID);

  onProgress?.(90, 'Finalizing...');

  const min = bounds.isEmpty() ? new THREE.Vector3() : bounds.min;
  const max = bounds.isEmpty() ? new THREE.Vector3(1, 1, 1) : bounds.max;

  onProgress?.(100, 'Complete');

  return {
    scene: root,
    totalVertices,
    totalTriangles,
    bounds: { min, max },
  };
}
