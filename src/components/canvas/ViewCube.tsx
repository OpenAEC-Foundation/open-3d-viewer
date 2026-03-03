import { useEffect, useRef } from 'react';
import * as THREE from 'three';

interface ViewCubeProps {
  cameraRef: React.RefObject<THREE.Camera | null>;
}

const SIZE = 70;
const HALF = SIZE / 2;

const FACES = [
  { label: 'Voor',   preset: 'front',  transform: `translateZ(${HALF}px)` },
  { label: 'Achter', preset: 'back',   transform: `rotateY(180deg) translateZ(${HALF}px)` },
  { label: 'Rechts', preset: 'right',  transform: `rotateY(90deg) translateZ(${HALF}px)` },
  { label: 'Links',  preset: 'left',   transform: `rotateY(-90deg) translateZ(${HALF}px)` },
  { label: 'Boven',  preset: 'top',    transform: `rotateX(-90deg) translateZ(${HALF}px)` },
  { label: 'Onder',  preset: 'bottom', transform: `rotateX(90deg) translateZ(${HALF}px)` },
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

      // Convert Three.js rotation matrix to CSS matrix3d
      // Y-axis is flipped between Three.js (Y-up) and CSS (Y-down)
      cube.style.transform =
        `matrix3d(${e[0]},${-e[1]},${e[2]},0,${-e[4]},${e[5]},${-e[6]},0,${e[8]},${-e[9]},${e[10]},0,0,0,0,1)`;
    };

    update();
    return () => cancelAnimationFrame(rafRef.current);
  }, [cameraRef]);

  const snap = (preset: string) => {
    window.dispatchEvent(new CustomEvent('open3d:camera', { detail: preset }));
  };

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
    borderRadius: 2,
    userSelect: 'none',
    pointerEvents: 'auto',
    transition: 'background 0.15s, border-color 0.15s, color 0.15s',
  });

  const handleEnter = (e: React.MouseEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.style.background = 'rgba(35, 60, 105, 0.95)';
    el.style.borderColor = 'rgba(90, 140, 210, 0.6)';
    el.style.color = 'white';
  };

  const handleLeave = (e: React.MouseEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.style.background = 'rgba(20, 30, 50, 0.82)';
    el.style.borderColor = 'rgba(70, 110, 170, 0.3)';
    el.style.color = 'rgba(180, 195, 220, 0.9)';
  };

  // Axis label positions (in 3D space, relative to cube center)
  const axisLabels = [
    { label: 'X', color: '#e05555', x: HALF + 14, y: 0, z: 0 },
    { label: 'Y', color: '#55b855', x: 0, y: -(HALF + 14), z: 0 },
    { label: 'Z', color: '#5580e0', x: 0, y: 0, z: HALF + 14 },
  ];

  return (
    <div
      style={{
        position: 'absolute',
        top: 8,
        right: 8,
        width: SIZE + 44,
        height: SIZE + 44,
        perspective: 250,
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
        {/* 6 clickable faces */}
        {FACES.map((face) => (
          <div
            key={face.preset}
            onClick={() => snap(face.preset)}
            style={faceStyle(face.transform)}
            onMouseEnter={handleEnter}
            onMouseLeave={handleLeave}
          >
            {face.label}
          </div>
        ))}

        {/* Axis labels */}
        {axisLabels.map((a) => (
          <span
            key={a.label}
            style={{
              position: 'absolute',
              left: SIZE / 2 - 5 + a.x,
              top: SIZE / 2 - 6 + a.y,
              transform: `translateZ(${a.z}px)`,
              color: a.color,
              fontSize: 11,
              fontWeight: 700,
              pointerEvents: 'none',
              textShadow: '0 0 4px rgba(0,0,0,0.8)',
            }}
          >
            {a.label}
          </span>
        ))}
      </div>
    </div>
  );
}
