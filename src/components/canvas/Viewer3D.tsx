import { useEffect, useRef, useCallback, useState } from 'react';
import * as THREE from 'three';
import { RendererManager } from '../../engine/renderer/RendererManager';
import { ControlsManager } from '../../engine/controls/ControlsManager';
import { SceneManager } from '../../engine/scene/SceneManager';
import { SectionBox } from '../../engine/sectionbox/SectionBox';
import { SectionBoxController } from '../../engine/sectionbox/SectionBoxController';
import { SectionCapRenderer } from '../../engine/sectionbox/SectionCapRenderer';
import { MeasurementManager } from '../../engine/measurement/MeasurementManager';
import { modelLoader } from '../../engine/loaders/ModelLoader';
import { isSupported } from '../../engine/loaders/LoaderRegistry';
import { formatDistance } from '../../utils/format';
import { useAppStore } from '../../state/appStore';
import { ErrorOverlay } from './ErrorOverlay';
import { ViewCube } from './ViewCube';
import { WelcomeScreen } from './WelcomeScreen';

export function Viewer3D() {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<RendererManager | null>(null);
  const sceneRef = useRef<THREE.Scene | null>(null);
  const cameraRef = useRef<THREE.Camera | null>(null);
  const perspCameraRef = useRef<THREE.PerspectiveCamera | null>(null);
  const orthoCameraRef = useRef<THREE.OrthographicCamera | null>(null);
  const controlsRef = useRef<ControlsManager | null>(null);
  const sceneManagerRef = useRef<SceneManager | null>(null);
  const sectionBoxRef = useRef<SectionBox | null>(null);
  const sectionBoxCtrlRef = useRef<SectionBoxController | null>(null);
  const sectionCapRef = useRef<SectionCapRenderer | null>(null);
  const gridRef = useRef<THREE.GridHelper | null>(null);
  const ambientRef = useRef<THREE.AmbientLight | null>(null);
  const directionalRef = useRef<THREE.DirectionalLight | null>(null);
  const rafRef = useRef<number>(0);
  const fpsFrames = useRef(0);
  const fpsLastTime = useRef(performance.now());
  const selectedRef = useRef<THREE.Mesh | null>(null);
  const prevMaterialRef = useRef<THREE.Material | null>(null);
  const raycasterRef = useRef(new THREE.Raycaster());
  const measureManagerRef = useRef<MeasurementManager | null>(null);
  const measureFaceNormalRef = useRef<THREE.Vector3 | null>(null);
  const originAxesRef = useRef<THREE.Group | null>(null);
  const [hoverCoord, setHoverCoord] = useState<{ x: number; y: number; wx: number; wy: number; wz: number } | null>(null);

  const showGrid = useAppStore((s) => s.showGrid);
  const ambientLightEnabled = useAppStore((s) => s.ambientLightEnabled);
  const directionalLightEnabled = useAppStore((s) => s.directionalLightEnabled);
  const renderMode = useAppStore((s) => s.renderMode);
  const cameraMode = useAppStore((s) => s.cameraMode);
  const walkSpeed = useAppStore((s) => s.walkSpeed);
  const sectionBoxEnabled = useAppStore((s) => s.sectionBoxEnabled);
  const sectionCapsEnabled = useAppStore((s) => s.sectionCapsEnabled);

  // Fit camera to bounding box
  const fitCameraToBox = useCallback((box: THREE.Box3) => {
    const persp = perspCameraRef.current;
    const controls = controlsRef.current;
    if (!persp || !controls || box.isEmpty()) return;

    const center = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3());
    const maxDim = Math.max(size.x, size.y, size.z);
    const fov = persp.fov * (Math.PI / 180);
    const dist = maxDim / (2 * Math.tan(fov / 2)) * 1.5;
    const near = Math.max(0.01, maxDim * 0.001);
    const far = maxDim * 100;

    // Update perspective camera
    persp.position.set(center.x + dist * 0.5, center.y + dist * 0.5, center.z + dist * 0.5);
    persp.lookAt(center);
    persp.near = near;
    persp.far = far;
    persp.updateProjectionMatrix();

    // Update ortho camera
    const ortho = orthoCameraRef.current;
    if (ortho) {
      const halfH = maxDim * 0.75;
      const halfW = halfH * persp.aspect;
      ortho.left = -halfW;
      ortho.right = halfW;
      ortho.top = halfH;
      ortho.bottom = -halfH;
      ortho.near = near;
      ortho.far = far;
      ortho.position.copy(persp.position);
      ortho.lookAt(center);
      ortho.updateProjectionMatrix();
    }

    controls.orbit.setTarget(center);
  }, []);

  // Load a file into the scene
  const loadFileIntoScene = useCallback(async (id: string, file: File) => {
    const sceneManager = sceneManagerRef.current;
    if (!sceneManager) return;

    try {
      const result = await modelLoader.loadFile(file, (progress, phase) => {
        useAppStore.getState().updateModelProgress(id, progress, phase);
        useAppStore.getState().updateLoadingFile(id, progress, phase);
      });

      sceneManager.addModel(id, result);

      // Fit camera + section box + scale origin axes
      const box = sceneManager.getSceneBounds();
      fitCameraToBox(box);
      sectionBoxRef.current?.fitToScene(box);
      measureManagerRef.current?.setSceneScale(box);
      if (originAxesRef.current && !box.isEmpty()) {
        const sz = box.getSize(new THREE.Vector3());
        const s = Math.max(sz.x, sz.y, sz.z) * 0.08;
        originAxesRef.current.scale.setScalar(s);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`Failed to load: ${msg}`);
      useAppStore.getState().setLoadingFileError(id, msg);
      useAppStore.getState().updateModelProgress(id, 0, `Error: ${msg}`);
    }
  }, [fitCameraToBox]);

  // Load from Tauri path
  const loadPathIntoScene = useCallback(async (id: string, filePath: string, fileName: string) => {
    const sceneManager = sceneManagerRef.current;
    if (!sceneManager) return;

    try {
      const { readFile } = await import('@tauri-apps/plugin-fs');
      const bytes = await readFile(filePath);
      const buffer = bytes.buffer;

      const result = await modelLoader.loadBuffer(buffer as ArrayBuffer, fileName, (progress, phase) => {
        useAppStore.getState().updateModelProgress(id, progress, phase);
        useAppStore.getState().updateLoadingFile(id, progress, phase);
      });

      sceneManager.addModel(id, result);

      const box = sceneManager.getSceneBounds();
      fitCameraToBox(box);
      sectionBoxRef.current?.fitToScene(box);
      measureManagerRef.current?.setSceneScale(box);
      if (originAxesRef.current && !box.isEmpty()) {
        const sz = box.getSize(new THREE.Vector3());
        const s = Math.max(sz.x, sz.y, sz.z) * 0.08;
        originAxesRef.current.scale.setScalar(s);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`Failed to load: ${msg}`);
      useAppStore.getState().setLoadingFileError(id, msg);
      useAppStore.getState().updateModelProgress(id, 0, `Error: ${msg}`);
    }
  }, [fitCameraToBox]);

  // Initialize scene
  useEffect(() => {
    const container = containerRef.current;
    const canvas = canvasRef.current;
    if (!container || !canvas) return;

    // Scene
    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0xffffff);
    sceneRef.current = scene;

    // Scene Manager
    const sceneManager = new SceneManager(scene);
    sceneManagerRef.current = sceneManager;

    // Section Box
    const sectionBox = new SectionBox(scene);
    sectionBoxRef.current = sectionBox;

    // Camera (perspective + orthographic)
    const camera = new THREE.PerspectiveCamera(60, container.clientWidth / container.clientHeight, 0.1, 10000);
    camera.position.set(5, 5, 5);
    camera.lookAt(0, 0, 0);
    perspCameraRef.current = camera;

    const orthoCamera = new THREE.OrthographicCamera(-10, 10, 10, -10, 0.1, 10000);
    orthoCamera.position.set(5, 5, 5);
    orthoCamera.lookAt(0, 0, 0);
    orthoCameraRef.current = orthoCamera;

    cameraRef.current = camera;

    // Renderer
    const rendererManager = new RendererManager(canvas);
    rendererManager.setSize(container.clientWidth, container.clientHeight);
    rendererRef.current = rendererManager;
    useAppStore.getState().setRendererType(rendererManager.rendererType);

    // Controls
    const controls = new ControlsManager(camera, canvas);
    controlsRef.current = controls;

    // Section Box Controller (interactive dragging of faces)
    const sbController = new SectionBoxController(sectionBox, sectionBox.gizmo, camera, canvas, {
      onDragStart: () => controls.orbit.controls.enabled = false,
      onDragEnd: () => controls.orbit.controls.enabled = true,
      onBoundsChange: () => {
        const b = sectionBox.getBounds();
        useAppStore.getState().setSectionBoxBounds(b);
      },
    });
    sectionBoxCtrlRef.current = sbController;

    // Section cap renderer (stencil-based cross-section fills)
    const sectionCapRenderer = new SectionCapRenderer(rendererManager.renderer);
    sectionCapRef.current = sectionCapRenderer;

    // Grid
    const grid = new THREE.GridHelper(50, 50, 0xcccccc, 0xe0e0e0);
    grid.visible = false;
    scene.add(grid);
    gridRef.current = grid;

    // Origin axes marker (X=red, Y=green, Z=blue)
    const originAxes = new THREE.Group();
    originAxes.name = '__origin_axes';
    const axisLen = 1;
    const axisRadius = 0.015;
    const axisDefs: [number, THREE.Vector3, THREE.Euler][] = [
      [0xff3333, new THREE.Vector3(axisLen / 2, 0, 0), new THREE.Euler(0, 0, -Math.PI / 2)],
      [0x33cc33, new THREE.Vector3(0, axisLen / 2, 0), new THREE.Euler(0, 0, 0)],
      [0x3366ff, new THREE.Vector3(0, 0, axisLen / 2), new THREE.Euler(Math.PI / 2, 0, 0)],
    ];
    for (const [color, pos, rot] of axisDefs) {
      const cyl = new THREE.Mesh(
        new THREE.CylinderGeometry(axisRadius, axisRadius, axisLen, 8),
        new THREE.MeshBasicMaterial({ color, depthTest: false }),
      );
      cyl.position.copy(pos);
      cyl.rotation.copy(rot);
      cyl.renderOrder = 999;
      originAxes.add(cyl);
    }
    // Small sphere at origin
    const originSphere = new THREE.Mesh(
      new THREE.SphereGeometry(axisRadius * 2, 8, 8),
      new THREE.MeshBasicMaterial({ color: 0x888888, depthTest: false }),
    );
    originSphere.renderOrder = 999;
    originAxes.add(originSphere);
    scene.add(originAxes);
    originAxesRef.current = originAxes;

    // Lights
    const ambient = new THREE.AmbientLight(0xffffff, 0.6);
    scene.add(ambient);
    ambientRef.current = ambient;

    const directional = new THREE.DirectionalLight(0xffffff, 0.8);
    directional.position.set(10, 20, 10);
    scene.add(directional);
    directionalRef.current = directional;

    const hemi = new THREE.HemisphereLight(0xddeeff, 0x0f0e0d, 0.3);
    scene.add(hemi);

    // Measurement manager
    const measureManager = new MeasurementManager(scene, container);
    measureManagerRef.current = measureManager;

    // Render loop
    const animate = () => {
      rafRef.current = requestAnimationFrame(animate);
      controls.update();
      rendererManager.render(scene, cameraRef.current!);

      // Section cap rendering (stencil-based cross-section fills)
      if (sectionCapRenderer.enabled && sectionBox.enabled && sectionBox.planes.length > 0) {
        sectionCapRenderer.render(scene, cameraRef.current!, sectionBox.planes);
      }

      measureManager.updateLabels(cameraRef.current!, container.clientWidth, container.clientHeight);

      fpsFrames.current++;
      const now = performance.now();
      if (now - fpsLastTime.current >= 1000) {
        useAppStore.getState().setFps(fpsFrames.current);
        fpsFrames.current = 0;
        fpsLastTime.current = now;
      }
    };
    animate();

    // Resize observer
    const resizeObserver = new ResizeObserver(() => {
      const w = container.clientWidth;
      const h = container.clientHeight;
      if (w === 0 || h === 0) return;
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      // Also update ortho frustum if active
      const oc = orthoCameraRef.current;
      if (oc) {
        const aspect = w / h;
        const halfH = (oc.top - oc.bottom) / 2;
        oc.left = -halfH * aspect;
        oc.right = halfH * aspect;
        oc.updateProjectionMatrix();
      }
      rendererManager.setSize(w, h);
    });
    resizeObserver.observe(container);

    // Camera preset handler
    const handleCamera = (e: Event) => {
      const preset = (e as CustomEvent).detail as string;
      const bounds = sceneManager.getSceneBounds();
      const center = bounds.isEmpty() ? controls.orbit.controls.target.clone() : bounds.getCenter(new THREE.Vector3());
      const size = bounds.isEmpty() ? new THREE.Vector3(10, 10, 10) : bounds.getSize(new THREE.Vector3());
      const dist = Math.max(size.x, size.y, size.z) * 1.5;
      const cam = cameraRef.current!;

      const d = dist;
      const d2 = d * 0.7071; // 1/sqrt(2) for 45-degree edges/corners
      const d3 = d * 0.5774; // 1/sqrt(3) for corners
      switch (preset) {
        // Faces
        case 'front':  cam.position.set(center.x, center.y, center.z + d); break;
        case 'back':   cam.position.set(center.x, center.y, center.z - d); break;
        case 'left':   cam.position.set(center.x - d, center.y, center.z); break;
        case 'right':  cam.position.set(center.x + d, center.y, center.z); break;
        case 'top':    cam.position.set(center.x, center.y + d, center.z); break;
        case 'bottom': cam.position.set(center.x, center.y - d, center.z); break;
        // Edges (12)
        case 'front-right':  cam.position.set(center.x + d2, center.y, center.z + d2); break;
        case 'front-left':   cam.position.set(center.x - d2, center.y, center.z + d2); break;
        case 'back-right':   cam.position.set(center.x + d2, center.y, center.z - d2); break;
        case 'back-left':    cam.position.set(center.x - d2, center.y, center.z - d2); break;
        case 'front-top':    cam.position.set(center.x, center.y + d2, center.z + d2); break;
        case 'front-bottom': cam.position.set(center.x, center.y - d2, center.z + d2); break;
        case 'back-top':     cam.position.set(center.x, center.y + d2, center.z - d2); break;
        case 'back-bottom':  cam.position.set(center.x, center.y - d2, center.z - d2); break;
        case 'top-right':    cam.position.set(center.x + d2, center.y + d2, center.z); break;
        case 'top-left':     cam.position.set(center.x - d2, center.y + d2, center.z); break;
        case 'bottom-right': cam.position.set(center.x + d2, center.y - d2, center.z); break;
        case 'bottom-left':  cam.position.set(center.x - d2, center.y - d2, center.z); break;
        // Corners (8)
        case 'front-top-right':    cam.position.set(center.x + d3, center.y + d3, center.z + d3); break;
        case 'front-top-left':     cam.position.set(center.x - d3, center.y + d3, center.z + d3); break;
        case 'front-bottom-right': cam.position.set(center.x + d3, center.y - d3, center.z + d3); break;
        case 'front-bottom-left':  cam.position.set(center.x - d3, center.y - d3, center.z + d3); break;
        case 'back-top-right':     cam.position.set(center.x + d3, center.y + d3, center.z - d3); break;
        case 'back-top-left':      cam.position.set(center.x - d3, center.y + d3, center.z - d3); break;
        case 'back-bottom-right':  cam.position.set(center.x + d3, center.y - d3, center.z - d3); break;
        case 'back-bottom-left':   cam.position.set(center.x - d3, center.y - d3, center.z - d3); break;
      }
      cam.lookAt(center);
      controls.orbit.setTarget(center);
    };
    // Nudge camera (rotate incrementally from ViewCube arrows)
    const handleNudge = (e: Event) => {
      const { dx, dy } = (e as CustomEvent).detail as { dx: number; dy: number };
      const cam = cameraRef.current!;
      const target = controls.orbit.controls.target.clone();
      const offset = cam.position.clone().sub(target);
      const spherical = new THREE.Spherical().setFromVector3(offset);
      const step = Math.PI / 12; // 15 degrees per click
      spherical.theta -= dx * step;
      spherical.phi -= dy * step;
      spherical.phi = Math.max(0.05, Math.min(Math.PI - 0.05, spherical.phi));
      offset.setFromSpherical(spherical);
      cam.position.copy(target).add(offset);
      cam.lookAt(target);
      controls.orbit.setTarget(target);
    };
    window.addEventListener('open3d:nudge-camera', handleNudge);

    window.addEventListener('open3d:camera', handleCamera);

    // Section box reset handler
    const handleSectionBoxReset = () => {
      const bounds = sceneManager.getSceneBounds();
      if (!bounds.isEmpty()) {
        sectionBox.fitToScene(bounds);
        sectionBox.applyToScene();
        useAppStore.getState().setSectionBoxBounds(sectionBox.getBounds());
      }
    };
    window.addEventListener('open3d:sectionbox-reset', handleSectionBoxReset);

    // Import handlers
    const handleImport = (e: Event) => {
      const { id, filePath, fileName } = (e as CustomEvent).detail;
      loadPathIntoScene(id, filePath, fileName);
    };
    window.addEventListener('open3d:import', handleImport);

    const handleImportFile = (e: Event) => {
      const { id, file } = (e as CustomEvent).detail;
      loadFileIntoScene(id, file);
    };
    window.addEventListener('open3d:import-file', handleImportFile);

    // Remove model handler
    const handleRemoveModel = (e: Event) => {
      const { id } = (e as CustomEvent).detail;
      sceneManager.removeModel(id);
    };
    window.addEventListener('open3d:remove-model', handleRemoveModel);

    // Clear all models handler
    const handleClearScene = () => {
      sceneManager.removeAllModels();
      measureManager.clear();
    };
    window.addEventListener('open3d:clear-scene', handleClearScene);

    // Clear measurements handler
    const handleClearMeasurements = () => {
      measureManager.clear();
    };
    window.addEventListener('open3d:clear-measurements', handleClearMeasurements);

    // Drag and drop
    const handleDragOver = (e: DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    };

    const handleDrop = async (e: DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const files = e.dataTransfer?.files;
      if (!files) return;

      for (const file of Array.from(files)) {
        const ext = file.name.substring(file.name.lastIndexOf('.')).toLowerCase();
        if (!isSupported(ext)) {
          console.warn(`Unsupported format: ${ext}`);
          continue;
        }

        const id = crypto.randomUUID();
        const format = ext.replace('.', '').toUpperCase();

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

        loadFileIntoScene(id, file);
      }
    };

    canvas.addEventListener('dragover', handleDragOver);
    canvas.addEventListener('drop', handleDrop);

    // Click-to-select: raycast on click, highlight mesh, show name
    let pointerDownPos = { x: 0, y: 0 };
    const handlePointerDown = (e: PointerEvent) => {
      pointerDownPos = { x: e.clientX, y: e.clientY };
    };
    const handleClick = (e: MouseEvent) => {
      // Ignore drag gestures (only select on actual clicks)
      const dx = e.clientX - pointerDownPos.x;
      const dy = e.clientY - pointerDownPos.y;
      if (Math.abs(dx) > 5 || Math.abs(dy) > 5) return;

      const rect = canvas.getBoundingClientRect();
      const mouse = new THREE.Vector2(
        ((e.clientX - rect.left) / rect.width) * 2 - 1,
        -((e.clientY - rect.top) / rect.height) * 2 + 1,
      );

      raycasterRef.current.setFromCamera(mouse, cameraRef.current!);
      const intersects = raycasterRef.current.intersectObjects(scene.children, true);

      // Find first mesh hit (skip grid, gizmo helpers, measurement spheres)
      const hit = intersects.find((i) => {
        const obj = i.object;
        if (!(obj instanceof THREE.Mesh)) return false;
        if (obj.parent?.name === 'SectionBoxGizmo' || obj.parent?.name === 'section-box-gizmo') return false;
        if (obj instanceof THREE.GridHelper) return false;
        if (obj.name === '__measurement_sphere') return false;
        if (obj.name === '__hiddenline_edge') return false;
        return true;
      });

      // ── Measurement mode ──
      const state = useAppStore.getState();
      const measureMode = state.measureMode;
      if (measureMode !== 'off') {
        if (!hit) return; // Must click on geometry

        const hitPoint = hit.point.clone();

        if (measureMode === 'point') {
          const currentPoints = state.measurePoints;
          if (currentPoints.length === 0) {
            // First point
            state.addMeasurePoint({ x: hitPoint.x, y: hitPoint.y, z: hitPoint.z });
            measureManager.showTempPoint(hitPoint);
          } else {
            // Second point — complete measurement
            const p1 = new THREE.Vector3(currentPoints[0].x, currentPoints[0].y, currentPoints[0].z);
            const p2 = hitPoint;
            const dist = p1.distanceTo(p2);
            const id = crypto.randomUUID();
            const unit = state.displayUnit;
            const label = formatDistance(dist, unit);

            state.addMeasurement({ id, type: 'point', points: [currentPoints[0], { x: p2.x, y: p2.y, z: p2.z }], distance: dist });
            measureManager.addMeasurement(id, p1, p2, label);
            state.clearMeasurePoints();
          }
        } else if (measureMode === 'face') {
          const currentPoints = state.measurePoints;
          if (currentPoints.length === 0 && hit.face) {
            // First face — store point and normal
            state.addMeasurePoint({ x: hitPoint.x, y: hitPoint.y, z: hitPoint.z });
            const normal = hit.face.normal.clone();
            normal.transformDirection(hit.object.matrixWorld);
            measureFaceNormalRef.current = normal;
            measureManager.showTempPoint(hitPoint);
          } else if (currentPoints.length === 1 && hit.face) {
            // Second face — compute perpendicular distance
            const p1 = new THREE.Vector3(currentPoints[0].x, currentPoints[0].y, currentPoints[0].z);
            const p2 = hitPoint;
            const normal = measureFaceNormalRef.current || hit.face.normal.clone().transformDirection(hit.object.matrixWorld);
            // Project distance onto normal for face-to-face perpendicular distance
            const diff = new THREE.Vector3().subVectors(p2, p1);
            const dist = Math.abs(diff.dot(normal));
            const id = crypto.randomUUID();
            const unit = state.displayUnit;
            const label = formatDistance(dist, unit);

            state.addMeasurement({ id, type: 'face', points: [currentPoints[0], { x: p2.x, y: p2.y, z: p2.z }], distance: dist });
            measureManager.addMeasurement(id, p1, p2, label);
            state.clearMeasurePoints();
            measureFaceNormalRef.current = null;
          }
        }
        return;
      }

      // ── Normal selection mode ──
      // Deselect previous
      if (selectedRef.current && prevMaterialRef.current) {
        selectedRef.current.material = prevMaterialRef.current;
        selectedRef.current = null;
        prevMaterialRef.current = null;
      }

      if (hit && hit.object instanceof THREE.Mesh) {
        const mesh = hit.object;
        selectedRef.current = mesh;
        prevMaterialRef.current = mesh.material as THREE.Material;

        // Create highlight material — handle both MeshStandard and MeshBasic
        const srcMat = mesh.material as THREE.Material;
        const highlightMat = srcMat.clone();
        if ('emissive' in highlightMat) {
          (highlightMat as THREE.MeshStandardMaterial).emissive = new THREE.Color(0x4488ff);
          (highlightMat as THREE.MeshStandardMaterial).emissiveIntensity = 0.4;
        } else if ('color' in highlightMat) {
          (highlightMat as THREE.MeshBasicMaterial).color = new THREE.Color(0xaaccff);
        }
        mesh.material = highlightMat;

        // Compute info
        const geo = mesh.geometry;
        const triCount = geo.index ? geo.index.count / 3 : geo.attributes.position.count / 3;
        geo.computeBoundingBox();
        const box = geo.boundingBox ? geo.boundingBox.clone() : new THREE.Box3();
        box.applyMatrix4(mesh.matrixWorld);
        const size = box.getSize(new THREE.Vector3());
        const center = box.getCenter(new THREE.Vector3());

        useAppStore.getState().setSelectedObject({
          name: mesh.name,
          triangles: Math.round(triCount),
          sizeX: parseFloat(size.x.toFixed(2)),
          sizeY: parseFloat(size.y.toFixed(2)),
          sizeZ: parseFloat(size.z.toFixed(2)),
          centerX: parseFloat(center.x.toFixed(2)),
          centerY: parseFloat(center.y.toFixed(2)),
          centerZ: parseFloat(center.z.toFixed(2)),
        });
      } else {
        useAppStore.getState().setSelectedObject(null);
      }
    };
    canvas.addEventListener('pointerdown', handlePointerDown);
    canvas.addEventListener('click', handleClick);

    // Mousemove: coordinate tooltip
    const coordRaycaster = new THREE.Raycaster();
    const handleMouseMove = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      const mouse = new THREE.Vector2(
        ((e.clientX - rect.left) / rect.width) * 2 - 1,
        -((e.clientY - rect.top) / rect.height) * 2 + 1,
      );
      coordRaycaster.setFromCamera(mouse, cameraRef.current!);
      const hits = coordRaycaster.intersectObjects(scene.children, true);
      const hit = hits.find((h) => {
        const obj = h.object;
        if (!(obj instanceof THREE.Mesh)) return false;
        if (obj.parent?.name === 'SectionBoxGizmo' || obj.parent?.name === 'section-box-gizmo') return false;
        if (obj.name === '__section_cap' || obj.name === '__measurement_sphere' || obj.name === '__hiddenline_edge') return false;
        if (obj.parent?.name === '__origin_axes') return false;
        return true;
      });
      if (hit) {
        const p = hit.point;
        setHoverCoord({
          x: e.clientX - rect.left,
          y: e.clientY - rect.top,
          wx: parseFloat(p.x.toFixed(3)),
          wy: parseFloat(p.y.toFixed(3)),
          wz: parseFloat(p.z.toFixed(3)),
        });
      } else {
        setHoverCoord(null);
      }
    };
    canvas.addEventListener('mousemove', handleMouseMove);
    canvas.addEventListener('mouseleave', () => setHoverCoord(null));

    // HH shortcut: hide selected object (double-tap H)
    let lastHTime = 0;
    const handleKeyDown = (e: KeyboardEvent) => {
      // Ctrl+Z = undo layer visibility, Ctrl+Y / Ctrl+Shift+Z = redo
      if ((e.ctrlKey || e.metaKey) && !e.altKey) {
        if (e.key === 'z' && !e.shiftKey) {
          e.preventDefault();
          useAppStore.getState().undoLayerVisibility();
          return;
        }
        if (e.key === 'y' || (e.key === 'z' && e.shiftKey) || (e.key === 'Z' && e.shiftKey)) {
          e.preventDefault();
          useAppStore.getState().redoLayerVisibility();
          return;
        }
      }

      // ESC: cancel active measurement
      if (e.key === 'Escape') {
        const ms = useAppStore.getState();
        if (ms.measureMode !== 'off' && ms.measurePoints.length > 0) {
          ms.clearMeasurePoints();
          measureManager.clearTemp();
          return;
        }
        // Also allow ESC to turn off measure mode entirely
        if (ms.measureMode !== 'off') {
          ms.setMeasureMode('off');
          return;
        }
      }

      // W key: switch to walk mode
      if (e.key.toLowerCase() === 'w' && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const tag = (e.target as HTMLElement).tagName;
        if (tag !== 'INPUT' && tag !== 'TEXTAREA' && tag !== 'SELECT') {
          useAppStore.getState().setCameraMode('walk');
        }
      }

      if (e.key.toLowerCase() === 'h') {
        const now = Date.now();
        if (now - lastHTime < 400) {
          // Double-H: hide selected mesh
          const sel = selectedRef.current;
          if (sel) {
            // Restore material first
            if (prevMaterialRef.current) {
              sel.material = prevMaterialRef.current;
            }
            // Hide the mesh (or its parent group = layer)
            const parent = sel.parent;
            if (parent instanceof THREE.Group && parent.name && parent.parent?.name?.startsWith('model-')) {
              parent.visible = false;
              useAppStore.getState().setLayerVisible(parent.name, false);
            } else {
              sel.visible = false;
            }
            selectedRef.current = null;
            prevMaterialRef.current = null;
            useAppStore.getState().setSelectedObject(null);
          }
          lastHTime = 0;
        } else {
          lastHTime = now;
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);

    return () => {
      cancelAnimationFrame(rafRef.current);
      resizeObserver.disconnect();
      window.removeEventListener('open3d:nudge-camera', handleNudge);
      window.removeEventListener('open3d:camera', handleCamera);
      window.removeEventListener('open3d:sectionbox-reset', handleSectionBoxReset);
      window.removeEventListener('open3d:import', handleImport);
      window.removeEventListener('open3d:import-file', handleImportFile);
      window.removeEventListener('open3d:remove-model', handleRemoveModel);
      window.removeEventListener('open3d:clear-scene', handleClearScene);
      window.removeEventListener('open3d:clear-measurements', handleClearMeasurements);
      canvas.removeEventListener('dragover', handleDragOver);
      canvas.removeEventListener('drop', handleDrop);
      canvas.removeEventListener('pointerdown', handlePointerDown);
      canvas.removeEventListener('click', handleClick);
      canvas.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('keydown', handleKeyDown);
      controls.dispose();
      sbController.dispose();
      sectionBox.dispose();
      sectionCapRenderer.dispose();
      measureManager.dispose();
      rendererManager.dispose();
      scene.clear();
    };
  }, [loadFileIntoScene, loadPathIntoScene]);

  // Auto-load file from URL parameter: ?file=test-kavel.skp
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const autoFile = params.get('file');
    if (!autoFile) return;

    const ext = autoFile.substring(autoFile.lastIndexOf('.')).toLowerCase();
    if (!isSupported(ext)) return;

    const id = crypto.randomUUID();
    const format = ext.replace('.', '').toUpperCase();

    useAppStore.getState().addModel({
      id,
      fileName: autoFile,
      filePath: autoFile,
      format,
      vertices: 0,
      triangles: 0,
      bounds: { minX: 0, minY: 0, minZ: 0, maxX: 0, maxY: 0, maxZ: 0 },
      visible: true,
      loadingProgress: 0,
      loadingPhase: 'Auto-loading...',
    });
    useAppStore.getState().addLoadingFile(id, autoFile);

    fetch(`/${autoFile}`)
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.blob();
      })
      .then((blob) => {
        const file = new File([blob], autoFile, { type: 'application/octet-stream' });
        loadFileIntoScene(id, file);
      })
      .catch((err) => {
        console.error(`Auto-load failed: ${err}`);
        useAppStore.getState().setLoadingFileError(id, String(err));
      });
  }, [loadFileIntoScene]);

  // Update grid visibility
  useEffect(() => {
    if (gridRef.current) gridRef.current.visible = showGrid;
  }, [showGrid]);

  // Update lights
  useEffect(() => {
    if (ambientRef.current) ambientRef.current.visible = ambientLightEnabled;
  }, [ambientLightEnabled]);

  useEffect(() => {
    if (directionalRef.current) directionalRef.current.visible = directionalLightEnabled;
  }, [directionalLightEnabled]);

  // Update render mode
  useEffect(() => {
    sceneManagerRef.current?.setRenderMode(renderMode);
  }, [renderMode]);

  // Update camera mode (orbit / walk)
  useEffect(() => {
    controlsRef.current?.setMode(cameraMode);
  }, [cameraMode]);

  // Update projection mode (perspective / orthographic)
  const projectionMode = useAppStore((s) => s.projectionMode);
  useEffect(() => {
    const persp = perspCameraRef.current;
    const ortho = orthoCameraRef.current;
    const controls = controlsRef.current;
    const sbCtrl = sectionBoxCtrlRef.current;
    if (!persp || !ortho || !controls) return;

    if (projectionMode === 'orthographic') {
      // Calculate ortho frustum from current perspective view
      const target = controls.orbit.controls.target;
      const distance = persp.position.distanceTo(target);
      const halfH = distance * Math.tan(THREE.MathUtils.degToRad(persp.fov / 2));
      const halfW = halfH * persp.aspect;
      ortho.left = -halfW;
      ortho.right = halfW;
      ortho.top = halfH;
      ortho.bottom = -halfH;
      ortho.near = persp.near;
      ortho.far = persp.far;
      ortho.position.copy(persp.position);
      ortho.rotation.copy(persp.rotation);
      ortho.zoom = 1;
      ortho.updateProjectionMatrix();
      cameraRef.current = ortho;
    } else {
      // Sync perspective camera position from ortho
      persp.position.copy(ortho.position);
      persp.rotation.copy(ortho.rotation);
      persp.updateProjectionMatrix();
      cameraRef.current = persp;
    }
    controls.setCamera(cameraRef.current);
    sbCtrl?.setCamera(cameraRef.current);
  }, [projectionMode]);

  // Update walk speed
  useEffect(() => {
    controlsRef.current?.setWalkSpeed(walkSpeed);
  }, [walkSpeed]);

  // Update section box
  useEffect(() => {
    const sb = sectionBoxRef.current;
    if (!sb) return;

    // If enabling and bounds haven't been set yet, fit to current scene
    if (sectionBoxEnabled && !sb.enabled) {
      const sceneManager = sceneManagerRef.current;
      if (sceneManager) {
        const bounds = sceneManager.getSceneBounds();
        if (!bounds.isEmpty()) {
          sb.fitToScene(bounds);
        }
      }
    }

    sb.setEnabled(sectionBoxEnabled);
    sb.applyToScene();
  }, [sectionBoxEnabled]);

  // Update section caps toggle
  useEffect(() => {
    if (sectionCapRef.current) {
      sectionCapRef.current.enabled = sectionCapsEnabled;
    }
  }, [sectionCapsEnabled]);

  // Update layer visibility
  const layers = useAppStore((s) => s.layers);
  useEffect(() => {
    const sm = sceneManagerRef.current;
    if (!sm) return;
    for (const layer of layers) {
      sm.setLayerVisible(layer.name, layer.visible);
    }
  }, [layers]);

  // Update cursor for measure mode
  const measureMode = useAppStore((s) => s.measureMode);
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.style.cursor = measureMode !== 'off' ? 'crosshair' : '';
  }, [measureMode]);

  // Update measurement labels when displayUnit changes
  const displayUnit = useAppStore((s) => s.displayUnit);
  const measurements = useAppStore((s) => s.measurements);
  useEffect(() => {
    const mm = measureManagerRef.current;
    if (!mm) return;
    for (const m of measurements) {
      mm.updateLabel(m.id, formatDistance(m.distance, displayUnit));
    }
  }, [displayUnit, measurements]);

  // Update scene background based on theme
  const uiTheme = useAppStore((s) => s.uiTheme);
  useEffect(() => {
    if (!sceneRef.current) return;
    const colors: Record<string, number> = {
      dark: 0x1a1a2e,
      light: 0xffffff,
      blue: 0x0d1b2a,
      highContrast: 0x000000,
    };
    sceneRef.current.background = new THREE.Color(colors[uiTheme] ?? 0x1a1a2e);
  }, [uiTheme]);

  return (
    <div ref={containerRef} className="w-full h-full relative">
      <canvas ref={canvasRef} className="w-full h-full block" />
      <WelcomeScreen />
      <ViewCube cameraRef={cameraRef} />
      <ErrorOverlay />
      {hoverCoord && (
        <div
          className="absolute pointer-events-none z-50"
          style={{
            left: hoverCoord.x + 16,
            top: hoverCoord.y - 8,
            background: 'rgba(0,0,0,0.75)',
            color: '#fff',
            padding: '2px 6px',
            borderRadius: 3,
            fontSize: 11,
            fontFamily: 'monospace',
            whiteSpace: 'nowrap',
          }}
        >
          X: {hoverCoord.wx} &nbsp; Y: {hoverCoord.wy} &nbsp; Z: {hoverCoord.wz}
        </div>
      )}
    </div>
  );
}
