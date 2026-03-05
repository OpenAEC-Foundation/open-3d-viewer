import { useEffect, useRef, useCallback } from 'react';
import * as THREE from 'three';
import { useTranslation } from '../../i18n/useTranslation';
import { useAppStore } from '../../state/appStore';

interface ViewCubeProps {
  cameraRef: React.RefObject<THREE.Camera | null>;
}

const SIZE = 70;
const HALF = SIZE / 2;
const EDGE_W = 10;

type Dir3 = [number, number, number];
type ZonePos = 'center' | 'top' | 'bottom' | 'left' | 'right' | 'tl' | 'tr' | 'bl' | 'br';

const FACE_NORMAL: Record<string, Dir3> = {
  top: [0, 0, 1], bottom: [0, 0, -1],
  right: [1, 0, 0], left: [-1, 0, 0],
  front: [0, -1, 0], back: [0, 1, 0],
};

// For each face, maps div edge position to the world direction of the adjacent face.
// div "top" (y=0) = CSS -Y → World +Y (back), div "bottom" (y=SIZE) = CSS +Y → World -Y (front)
// for faces rotated only around Y. Front/back use rotateX so their Y maps differently.
const FACE_ADJ: Record<string, Record<string, Dir3>> = {
  top:    { top: [0,1,0],  bottom: [0,-1,0], left: [-1,0,0], right: [1,0,0]  },
  bottom: { top: [0,1,0],  bottom: [0,-1,0], left: [1,0,0],  right: [-1,0,0] },
  right:  { top: [0,1,0],  bottom: [0,-1,0], left: [0,0,1],  right: [0,0,-1] },
  left:   { top: [0,1,0],  bottom: [0,-1,0], left: [0,0,-1], right: [0,0,1]  },
  front:  { top: [0,0,1],  bottom: [0,0,-1], left: [-1,0,0], right: [1,0,0]  },
  back:   { top: [0,0,-1], bottom: [0,0,1],  left: [-1,0,0], right: [1,0,0]  },
};

// Reverse lookups
const DIR_TO_FACE: Record<string, string> = {};
for (const [key, dir] of Object.entries(FACE_NORMAL)) {
  DIR_TO_FACE[dir.join(',')] = key;
}

const DIR_TO_POS: Record<string, Record<string, string>> = {};
for (const [faceKey, adj] of Object.entries(FACE_ADJ)) {
  const map: Record<string, string> = {};
  for (const [pos, dir] of Object.entries(adj)) {
    map[(dir as Dir3).join(',')] = pos;
  }
  DIR_TO_POS[faceKey] = map;
}

const FACES: { key: string; preset: string; transform: string; textRotate?: string }[] = [
  { key: 'top',    preset: 'top',    transform: `translateZ(${HALF}px)` },
  { key: 'bottom', preset: 'bottom', transform: `rotateY(180deg) translateZ(${HALF}px)` },
  { key: 'right',  preset: 'right',  transform: `rotateY(90deg) translateZ(${HALF}px)`, textRotate: 'rotate(-90deg)' },
  { key: 'left',   preset: 'left',   transform: `rotateY(-90deg) translateZ(${HALF}px)`, textRotate: 'rotate(90deg)' },
  { key: 'front',  preset: 'front',  transform: `rotateX(-90deg) translateZ(${HALF}px)` },
  { key: 'back',   preset: 'back',   transform: `rotateX(90deg) translateZ(${HALF}px)` },
];

function zoneRect(pos: ZonePos) {
  switch (pos) {
    case 'center': return { top: EDGE_W, left: EDGE_W, width: SIZE - 2 * EDGE_W, height: SIZE - 2 * EDGE_W };
    case 'top':    return { top: 0, left: EDGE_W, width: SIZE - 2 * EDGE_W, height: EDGE_W };
    case 'bottom': return { top: SIZE - EDGE_W, left: EDGE_W, width: SIZE - 2 * EDGE_W, height: EDGE_W };
    case 'left':   return { top: EDGE_W, left: 0, width: EDGE_W, height: SIZE - 2 * EDGE_W };
    case 'right':  return { top: EDGE_W, left: SIZE - EDGE_W, width: EDGE_W, height: SIZE - 2 * EDGE_W };
    case 'tl':     return { top: 0, left: 0, width: EDGE_W, height: EDGE_W };
    case 'tr':     return { top: 0, left: SIZE - EDGE_W, width: EDGE_W, height: EDGE_W };
    case 'bl':     return { top: SIZE - EDGE_W, left: 0, width: EDGE_W, height: EDGE_W };
    case 'br':     return { top: SIZE - EDGE_W, left: SIZE - EDGE_W, width: EDGE_W, height: EDGE_W };
  }
}

function cornerFromEdges(a: string, b: string): ZonePos {
  let v: string, h: string;
  if (a === 'top' || a === 'bottom') { v = a; h = b; } else { v = b; h = a; }
  return ((v === 'top' ? 't' : 'b') + (h === 'left' ? 'l' : 'r')) as ZonePos;
}

function getHighlightZones(faceKey: string, x: number, y: number): Map<string, ZonePos> {
  const zones = new Map<string, ZonePos>();
  const adj = FACE_ADJ[faceKey];
  const normalStr = FACE_NORMAL[faceKey].join(',');

  const onTop = y < EDGE_W;
  const onBottom = y > SIZE - EDGE_W;
  const onLeft = x < EDGE_W;
  const onRight = x > SIZE - EDGE_W;

  if ((onTop || onBottom) && (onLeft || onRight)) {
    // Corner — highlight on 3 faces
    const vPos = onTop ? 'top' : 'bottom';
    const hPos = onLeft ? 'left' : 'right';
    const vDir = adj[vPos] as Dir3;
    const hDir = adj[hPos] as Dir3;

    zones.set(faceKey, cornerFromEdges(vPos, hPos));

    const vFace = DIR_TO_FACE[vDir.join(',')];
    if (vFace) {
      const p1 = DIR_TO_POS[vFace][normalStr];
      const p2 = DIR_TO_POS[vFace][hDir.join(',')];
      if (p1 && p2) zones.set(vFace, cornerFromEdges(p1, p2));
    }

    const hFace = DIR_TO_FACE[hDir.join(',')];
    if (hFace) {
      const p1 = DIR_TO_POS[hFace][normalStr];
      const p2 = DIR_TO_POS[hFace][vDir.join(',')];
      if (p1 && p2) zones.set(hFace, cornerFromEdges(p1, p2));
    }
  } else if (onTop || onBottom) {
    // Edge — highlight on 2 faces
    const vPos = onTop ? 'top' : 'bottom';
    const vDir = adj[vPos] as Dir3;
    zones.set(faceKey, vPos as ZonePos);

    const adjFace = DIR_TO_FACE[vDir.join(',')];
    if (adjFace) {
      const pos = DIR_TO_POS[adjFace][normalStr];
      if (pos) zones.set(adjFace, pos as ZonePos);
    }
  } else if (onLeft || onRight) {
    // Edge — highlight on 2 faces
    const hPos = onLeft ? 'left' : 'right';
    const hDir = adj[hPos] as Dir3;
    zones.set(faceKey, hPos as ZonePos);

    const adjFace = DIR_TO_FACE[hDir.join(',')];
    if (adjFace) {
      const pos = DIR_TO_POS[adjFace][normalStr];
      if (pos) zones.set(adjFace, pos as ZonePos);
    }
  } else {
    // Center — highlight only this face
    zones.set(faceKey, 'center');
  }

  return zones;
}

function getSnapPreset(faceKey: string, x: number, y: number): string {
  const adj = FACE_ADJ[faceKey];
  const n = FACE_NORMAL[faceKey];

  const onTop = y < EDGE_W;
  const onBottom = y > SIZE - EDGE_W;
  const onLeft = x < EDGE_W;
  const onRight = x > SIZE - EDGE_W;

  if ((onTop || onBottom) && (onLeft || onRight)) {
    const vDir = adj[onTop ? 'top' : 'bottom'] as Dir3;
    const hDir = adj[onLeft ? 'left' : 'right'] as Dir3;
    return `dir:${n[0]+vDir[0]+hDir[0]},${n[1]+vDir[1]+hDir[1]},${n[2]+vDir[2]+hDir[2]}`;
  }
  if (onTop || onBottom) {
    const d = adj[onTop ? 'top' : 'bottom'] as Dir3;
    return `dir:${n[0]+d[0]},${n[1]+d[1]},${n[2]+d[2]}`;
  }
  if (onLeft || onRight) {
    const d = adj[onLeft ? 'left' : 'right'] as Dir3;
    return `dir:${n[0]+d[0]},${n[1]+d[1]},${n[2]+d[2]}`;
  }
  return faceKey;
}

export function ViewCube({ cameraRef }: ViewCubeProps) {
  const { t } = useTranslation('viewCube');
  const projectionMode = useAppStore((s) => s.projectionMode);
  const cubeRef = useRef<HTMLDivElement>(null);
  const rafRef = useRef<number>(0);

  useEffect(() => {
    const q = new THREE.Quaternion();
    const m = new THREE.Matrix4();

    const update = () => {
      rafRef.current = requestAnimationFrame(update);
      const camera = cameraRef.current;
      const cube = cubeRef.current;
      if (!camera || !cube) return;

      camera.getWorldQuaternion(q);
      q.invert();
      m.makeRotationFromQuaternion(q);
      const e = m.elements;

      cube.style.transform =
        `matrix3d(${e[0]},${-e[1]},${e[2]},0,${-e[4]},${e[5]},${-e[6]},0,${e[8]},${-e[9]},${e[10]},0,0,0,0,1)`;
    };

    update();
    return () => cancelAnimationFrame(rafRef.current);
  }, [cameraRef]);

  const applyHighlights = useCallback((zones: Map<string, ZonePos> | null) => {
    const cube = cubeRef.current;
    if (!cube) return;
    cube.querySelectorAll<HTMLElement>('[data-hl]').forEach(el => {
      const face = el.getAttribute('data-hl')!;
      const pos = zones?.get(face);
      if (pos) {
        const r = zoneRect(pos);
        el.style.top = r.top + 'px';
        el.style.left = r.left + 'px';
        el.style.width = r.width + 'px';
        el.style.height = r.height + 'px';
        el.style.opacity = '1';
      } else {
        el.style.opacity = '0';
      }
    });
  }, []);

  const handleMove = useCallback((faceKey: string, e: React.MouseEvent) => {
    const x = e.nativeEvent.offsetX;
    const y = e.nativeEvent.offsetY;
    applyHighlights(getHighlightZones(faceKey, x, y));
  }, [applyHighlights]);

  const handleLeave = useCallback(() => {
    applyHighlights(null);
  }, [applyHighlights]);

  const handleClick = useCallback((faceKey: string, e: React.MouseEvent) => {
    const x = e.nativeEvent.offsetX;
    const y = e.nativeEvent.offsetY;
    const preset = getSnapPreset(faceKey, x, y);
    window.dispatchEvent(new CustomEvent('open3d:camera', { detail: preset }));
  }, []);

  return (
    <div
      style={{
        position: 'absolute',
        top: 8,
        right: 8,
        width: SIZE + 44,
        height: SIZE + 44,
        perspective: projectionMode === 'orthographic' ? 'none' : 250,
        perspectiveOrigin: '50% 50%',
        zIndex: 10,
        pointerEvents: 'none',
      }}
    >
      <div
        ref={cubeRef}
        style={{
          width: SIZE,
          height: SIZE,
          position: 'absolute',
          left: 22,
          top: 22,
          transformStyle: 'preserve-3d',
        }}
      >
        {FACES.map((face) => (
          <div
            key={face.key}
            onClick={(e) => handleClick(face.key, e)}
            onMouseMove={(e) => handleMove(face.key, e)}
            onMouseLeave={handleLeave}
            style={{
              position: 'absolute',
              width: SIZE,
              height: SIZE,
              transform: face.transform,
              backfaceVisibility: 'hidden',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              fontSize: 10,
              fontWeight: 600,
              letterSpacing: 0.5,
              textTransform: 'uppercase',
              color: 'rgba(180, 195, 220, 0.9)',
              background: 'rgba(20, 30, 50, 0.82)',
              border: '1px solid rgba(70, 110, 170, 0.3)',
              borderRadius: 2,
              userSelect: 'none',
              pointerEvents: 'auto',
              cursor: 'pointer',
            }}
          >
            {/* Highlight overlay */}
            <div
              data-hl={face.key}
              style={{
                position: 'absolute',
                background: 'rgba(60, 120, 200, 0.45)',
                opacity: 0,
                pointerEvents: 'none',
                borderRadius: 1,
              }}
            />
            {/* Text label */}
            <span style={{
              position: 'relative',
              zIndex: 1,
              pointerEvents: 'none',
              ...(face.textRotate ? { display: 'inline-block', transform: face.textRotate } : {}),
            }}>
              {t(face.key)}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
