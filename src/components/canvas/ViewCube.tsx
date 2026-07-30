import { useEffect, useRef, useCallback } from 'react';
import * as THREE from 'three';

interface ViewCubeProps {
  cameraRef: React.RefObject<THREE.Camera | null>;
}

const SIZE = 70;
const HALF = SIZE / 2;
const EDGE_W = 10; // clickable edge thickness
const CORNER_S = 14; // corner hit size

// Outer container is bigger to fit the rotation arrows
const OUTER = SIZE + 60;
const ARROW_R = (SIZE / 2) + 18; // radius from center for arrows

const FACES = [
  { label: 'Voor',   preset: 'front',  transform: `translateZ(${HALF}px)` },
  { label: 'Achter', preset: 'back',   transform: `rotateY(180deg) translateZ(${HALF}px)` },
  { label: 'Rechts', preset: 'right',  transform: `rotateY(90deg) translateZ(${HALF}px)` },
  { label: 'Links',  preset: 'left',   transform: `rotateY(-90deg) translateZ(${HALF}px)` },
  { label: 'Onder',  preset: 'top',    transform: `rotateX(-90deg) translateZ(${HALF}px)` },
  { label: 'Boven',  preset: 'bottom', transform: `rotateX(90deg) translateZ(${HALF}px)` },
];

// 8 corners — small cubes at each vertex
const CORNERS: { preset: string; x: number; y: number; z: number }[] = [
  { preset: 'front-top-right',    x:  1, y:  1, z:  1 },
  { preset: 'front-top-left',     x: -1, y:  1, z:  1 },
  { preset: 'front-bottom-right', x:  1, y: -1, z:  1 },
  { preset: 'front-bottom-left',  x: -1, y: -1, z:  1 },
  { preset: 'back-top-right',     x:  1, y:  1, z: -1 },
  { preset: 'back-top-left',      x: -1, y:  1, z: -1 },
  { preset: 'back-bottom-right',  x:  1, y: -1, z: -1 },
  { preset: 'back-bottom-left',   x: -1, y: -1, z: -1 },
];

// Rotation arrow definitions: angle around center, rotation direction
const ARROWS: { angle: number; dx: number; dy: number; label: string }[] = [
  { angle: 0,   dx:  1, dy:  0, label: 'Draai rechts' },  // right
  { angle: 90,  dx:  0, dy:  1, label: 'Draai omlaag' },  // down
  { angle: 180, dx: -1, dy:  0, label: 'Draai links' },   // left
  { angle: 270, dx:  0, dy: -1, label: 'Draai omhoog' },  // up
];

export function ViewCube({ cameraRef }: ViewCubeProps) {
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

  const snap = useCallback((preset: string) => {
    window.dispatchEvent(new CustomEvent('open3d:camera', { detail: preset }));
  }, []);

  const nudgeView = useCallback((dx: number, dy: number) => {
    window.dispatchEvent(new CustomEvent('open3d:nudge-camera', { detail: { dx, dy } }));
  }, []);

  const faceStyle = (transform: string): React.CSSProperties => ({
    position: 'absolute',
    width: SIZE,
    height: SIZE,
    transform,
    backfaceVisibility: 'hidden',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    cursor: 'pointer',
    fontSize: 10,
    fontWeight: 600,
    letterSpacing: 0.5,
    textTransform: 'uppercase',
    color: 'rgba(180, 195, 220, 0.9)',
    background: 'rgba(20, 30, 50, 0.82)',
    border: '1px solid rgba(70, 110, 170, 0.3)',
    borderRadius: SIZE * 0.2,
    userSelect: 'none',
    pointerEvents: 'auto',
  });

  const cornerStyle = (x: number, y: number, z: number): React.CSSProperties => ({
    position: 'absolute',
    width: CORNER_S,
    height: CORNER_S,
    left: SIZE / 2 + x * (HALF) - CORNER_S / 2,
    top: SIZE / 2 - y * (HALF) - CORNER_S / 2,
    transform: `translateZ(${z * HALF}px)`,
    cursor: 'pointer',
    pointerEvents: 'auto',
    background: 'transparent',
    borderRadius: 2,
    zIndex: 10,
  });

  const handleEnter = (e: React.MouseEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.style.background = 'rgba(60, 130, 220, 0.7)';
  };

  const handleLeave = (e: React.MouseEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.style.background = el.dataset.bg || 'transparent';
  };

  const handleFaceEnter = (e: React.MouseEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.style.background = 'rgba(35, 60, 105, 0.95)';
    el.style.borderColor = 'rgba(90, 140, 210, 0.6)';
    el.style.color = 'white';
  };

  const handleFaceLeave = (e: React.MouseEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.style.background = 'rgba(20, 30, 50, 0.82)';
    el.style.borderColor = 'rgba(70, 110, 170, 0.3)';
    el.style.color = 'rgba(180, 195, 220, 0.9)';
  };

  return (
    <div
      style={{
        position: 'absolute',
        top: 4,
        right: 4,
        width: OUTER,
        height: OUTER,
        zIndex: 10,
        pointerEvents: 'none',
      }}
    >
      {/* Rotation arrows around the cube */}
      {ARROWS.map((arrow) => {
        const rad = (arrow.angle * Math.PI) / 180;
        const cx = OUTER / 2 + Math.cos(rad) * ARROW_R;
        const cy = OUTER / 2 + Math.sin(rad) * ARROW_R;
        // Curved arrow SVG pointing in the rotation direction
        const isHorizontal = arrow.dy === 0;
        return (
          <button
            key={arrow.angle}
            title={arrow.label}
            onClick={() => nudgeView(arrow.dx, arrow.dy)}
            onMouseEnter={(e) => {
              (e.currentTarget as HTMLElement).style.opacity = '1';
              (e.currentTarget as HTMLElement).style.background = 'rgba(60, 130, 220, 0.25)';
            }}
            onMouseLeave={(e) => {
              (e.currentTarget as HTMLElement).style.opacity = '0.5';
              (e.currentTarget as HTMLElement).style.background = 'transparent';
            }}
            style={{
              position: 'absolute',
              left: cx - 11,
              top: cy - 11,
              width: 22,
              height: 22,
              border: 'none',
              background: 'transparent',
              borderRadius: '50%',
              cursor: 'pointer',
              pointerEvents: 'auto',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              opacity: 0.5,
              transition: 'opacity 0.15s, background 0.15s',
              padding: 0,
            }}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="rgba(140, 170, 220, 0.9)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"
              style={{ transform: `rotate(${arrow.angle}deg)` }}
            >
              {isHorizontal ? (
                // Curved arrow for horizontal rotation
                <>
                  <path d="M9 4l5 0" />
                  <path d="M14 4l-3 -3" />
                  <path d="M14 4l-3 3" />
                </>
              ) : (
                // Curved arrow for vertical rotation
                <>
                  <path d="M4 9l0 5" />
                  <path d="M4 14l-3 -3" />
                  <path d="M4 14l3 -3" />
                </>
              )}
            </svg>
          </button>
        );
      })}

      {/* 3D cube container with perspective */}
      <div
        style={{
          position: 'absolute',
          left: (OUTER - SIZE) / 2,
          top: (OUTER - SIZE) / 2,
          width: SIZE,
          height: SIZE,
          perspective: 250,
          perspectiveOrigin: '50% 50%',
          pointerEvents: 'none',
        }}
      >
        <div
          ref={cubeRef}
          style={{
            width: SIZE,
            height: SIZE,
            position: 'absolute',
            transformStyle: 'preserve-3d',
          }}
        >
          {/* 6 clickable faces */}
          {FACES.map((face) => (
            <div
              key={face.preset}
              onClick={() => snap(face.preset)}
              style={faceStyle(face.transform)}
              onMouseEnter={handleFaceEnter}
              onMouseLeave={handleFaceLeave}
            >
              {face.label}
            </div>
          ))}

          {/* 8 corner hotspots — rendered on front & back face planes */}
          {CORNERS.map((c) => (
            <div
              key={c.preset}
              data-bg="transparent"
              onClick={() => snap(c.preset)}
              style={cornerStyle(c.x, c.y, c.z)}
              onMouseEnter={handleEnter}
              onMouseLeave={handleLeave}
              title={c.preset}
            />
          ))}

          {/* 4 horizontal edge strips on front face */}
          {(['top', 'bottom', 'left', 'right'] as const).map((side) => {
            const preset = `front-${side}`;
            const style: React.CSSProperties = {
              position: 'absolute',
              transform: `translateZ(${HALF}px)`,
              cursor: 'pointer',
              pointerEvents: 'auto',
              background: 'transparent',
              zIndex: 5,
              ...(side === 'top'    ? { left: CORNER_S - HALF, top: -HALF, width: SIZE - 2 * CORNER_S, height: EDGE_W } : {}),
              ...(side === 'bottom' ? { left: CORNER_S - HALF, top: HALF - EDGE_W, width: SIZE - 2 * CORNER_S, height: EDGE_W } : {}),
              ...(side === 'left'   ? { left: -HALF, top: CORNER_S - HALF, width: EDGE_W, height: SIZE - 2 * CORNER_S } : {}),
              ...(side === 'right'  ? { left: HALF - EDGE_W, top: CORNER_S - HALF, width: EDGE_W, height: SIZE - 2 * CORNER_S } : {}),
            };
            return (
              <div
                key={preset}
                data-bg="transparent"
                onClick={() => snap(preset)}
                style={style}
                onMouseEnter={handleEnter}
                onMouseLeave={handleLeave}
                title={preset}
              />
            );
          })}

          {/* 4 horizontal edge strips on back face */}
          {(['top', 'bottom', 'left', 'right'] as const).map((side) => {
            const preset = `back-${side}`;
            const style: React.CSSProperties = {
              position: 'absolute',
              transform: `rotateY(180deg) translateZ(${HALF}px)`,
              cursor: 'pointer',
              pointerEvents: 'auto',
              background: 'transparent',
              zIndex: 5,
              backfaceVisibility: 'hidden',
              ...(side === 'top'    ? { left: CORNER_S - HALF, top: -HALF, width: SIZE - 2 * CORNER_S, height: EDGE_W } : {}),
              ...(side === 'bottom' ? { left: CORNER_S - HALF, top: HALF - EDGE_W, width: SIZE - 2 * CORNER_S, height: EDGE_W } : {}),
              ...(side === 'left'   ? { left: -HALF, top: CORNER_S - HALF, width: EDGE_W, height: SIZE - 2 * CORNER_S } : {}),
              ...(side === 'right'  ? { left: HALF - EDGE_W, top: CORNER_S - HALF, width: EDGE_W, height: SIZE - 2 * CORNER_S } : {}),
            };
            return (
              <div
                key={preset}
                data-bg="transparent"
                onClick={() => snap(preset)}
                style={style}
                onMouseEnter={handleEnter}
                onMouseLeave={handleLeave}
                title={preset}
              />
            );
          })}

          {/* 4 edge strips on top face (front/back/left/right edges of top) */}
          {(['front', 'back', 'left', 'right'] as const).map((side) => {
            const preset = `top-${side}`;
            const style: React.CSSProperties = {
              position: 'absolute',
              transform: `rotateX(-90deg) translateZ(${HALF}px)`,
              cursor: 'pointer',
              pointerEvents: 'auto',
              background: 'transparent',
              zIndex: 5,
              backfaceVisibility: 'hidden',
              ...(side === 'front'  ? { left: CORNER_S - HALF, top: HALF - EDGE_W, width: SIZE - 2 * CORNER_S, height: EDGE_W } : {}),
              ...(side === 'back'   ? { left: CORNER_S - HALF, top: -HALF, width: SIZE - 2 * CORNER_S, height: EDGE_W } : {}),
              ...(side === 'left'   ? { left: -HALF, top: CORNER_S - HALF, width: EDGE_W, height: SIZE - 2 * CORNER_S } : {}),
              ...(side === 'right'  ? { left: HALF - EDGE_W, top: CORNER_S - HALF, width: EDGE_W, height: SIZE - 2 * CORNER_S } : {}),
            };
            return (
              <div
                key={preset}
                data-bg="transparent"
                onClick={() => snap(preset)}
                style={style}
                onMouseEnter={handleEnter}
                onMouseLeave={handleLeave}
                title={preset}
              />
            );
          })}

          {/* 4 edge strips on bottom face */}
          {(['front', 'back', 'left', 'right'] as const).map((side) => {
            const preset = `bottom-${side}`;
            const style: React.CSSProperties = {
              position: 'absolute',
              transform: `rotateX(90deg) translateZ(${HALF}px)`,
              cursor: 'pointer',
              pointerEvents: 'auto',
              background: 'transparent',
              zIndex: 5,
              backfaceVisibility: 'hidden',
              ...(side === 'front'  ? { left: CORNER_S - HALF, top: -HALF, width: SIZE - 2 * CORNER_S, height: EDGE_W } : {}),
              ...(side === 'back'   ? { left: CORNER_S - HALF, top: HALF - EDGE_W, width: SIZE - 2 * CORNER_S, height: EDGE_W } : {}),
              ...(side === 'left'   ? { left: -HALF, top: CORNER_S - HALF, width: EDGE_W, height: SIZE - 2 * CORNER_S } : {}),
              ...(side === 'right'  ? { left: HALF - EDGE_W, top: CORNER_S - HALF, width: EDGE_W, height: SIZE - 2 * CORNER_S } : {}),
            };
            return (
              <div
                key={preset}
                data-bg="transparent"
                onClick={() => snap(preset)}
                style={style}
                onMouseEnter={handleEnter}
                onMouseLeave={handleLeave}
                title={preset}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}
