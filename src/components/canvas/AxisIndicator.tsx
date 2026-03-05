import { useEffect, useRef } from 'react';
import * as THREE from 'three';

interface AxisIndicatorProps {
  cameraRef: React.RefObject<THREE.Camera | null>;
}

const AXIS_LEN = 32;

const AXES = [
  { label: 'X', color: '#e05555', dx: 1, dy: 0, dz: 0 },
  { label: 'Y', color: '#55b855', dx: 0, dy: 1, dz: 0 },
  { label: 'Z', color: '#5580e0', dx: 0, dy: 0, dz: 1 },
];

export function AxisIndicator({ cameraRef }: AxisIndicatorProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef<number>(0);

  useEffect(() => {
    const q = new THREE.Quaternion();
    const m = new THREE.Matrix4();
    const v = new THREE.Vector3();

    const update = () => {
      rafRef.current = requestAnimationFrame(update);
      const camera = cameraRef.current;
      const canvas = canvasRef.current;
      if (!camera || !canvas) return;

      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      camera.getWorldQuaternion(q);
      q.invert();
      m.makeRotationFromQuaternion(q);

      const size = canvas.width;
      const cx = size / 2;
      const cy = size / 2;

      ctx.clearRect(0, 0, size, size);

      // Project each axis and sort by depth (draw back-to-front)
      const projected = AXES.map((axis) => {
        v.set(axis.dx, axis.dy, axis.dz).applyMatrix4(m);
        // View space: X=right, Y=up, Z=toward viewer
        // Canvas: X=right, Y=down
        return {
          label: axis.label,
          color: axis.color,
          sx: v.x,
          sy: -v.y,
          depth: v.z,
        };
      });

      projected.sort((a, b) => a.depth - b.depth);

      for (const axis of projected) {
        const ex = cx + axis.sx * AXIS_LEN;
        const ey = cy + axis.sy * AXIS_LEN;
        const opacity = 0.4 + 0.6 * ((axis.depth + 1) / 2);

        // Line
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.lineTo(ex, ey);
        ctx.strokeStyle = axis.color;
        ctx.globalAlpha = opacity;
        ctx.lineWidth = 2;
        ctx.stroke();

        // Arrowhead
        const angle = Math.atan2(axis.sy, axis.sx);
        const headLen = 6;
        ctx.beginPath();
        ctx.moveTo(ex, ey);
        ctx.lineTo(
          ex - headLen * Math.cos(angle - 0.4),
          ey - headLen * Math.sin(angle - 0.4),
        );
        ctx.lineTo(
          ex - headLen * Math.cos(angle + 0.4),
          ey - headLen * Math.sin(angle + 0.4),
        );
        ctx.closePath();
        ctx.fillStyle = axis.color;
        ctx.fill();

        // Label
        const labelDist = AXIS_LEN + 12;
        const lx = cx + axis.sx * labelDist;
        const ly = cy + axis.sy * labelDist;
        ctx.font = '700 11px sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.globalAlpha = opacity;
        ctx.fillStyle = axis.color;
        ctx.fillText(axis.label, lx, ly);
      }

      ctx.globalAlpha = 1;

      // Center dot
      ctx.beginPath();
      ctx.arc(cx, cy, 2, 0, Math.PI * 2);
      ctx.fillStyle = 'rgba(180, 195, 220, 0.7)';
      ctx.fill();
    };

    update();
    return () => cancelAnimationFrame(rafRef.current);
  }, [cameraRef]);

  const SIZE = (AXIS_LEN + 16) * 2;

  return (
    <canvas
      ref={canvasRef}
      width={SIZE}
      height={SIZE}
      style={{
        position: 'absolute',
        bottom: 8,
        left: 8,
        width: SIZE,
        height: SIZE,
        zIndex: 10,
        pointerEvents: 'none',
      }}
    />
  );
}
