import { useEffect, useRef, useState } from 'react';
import './measure.css';

interface FloatingWindowProps {
  title: string;
  onClose: () => void;
  width: number;
  height: number;
  children: React.ReactNode;
  footer?: React.ReactNode;
}

/**
 * Non-modal window that floats over the app: the model stays usable, so you
 * can keep measuring while it is open. Drag it by its title bar; resize it
 * from the bottom-right corner.
 */
export function FloatingWindow({ title, onClose, width, height, children, footer }: FloatingWindowProps) {
  const [pos, setPos] = useState(() => ({
    x: Math.max(16, window.innerWidth - width - 40),
    y: Math.max(16, Math.min(140, window.innerHeight - height - 16)),
  }));
  const dragStart = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const move = (e: MouseEvent) => {
      if (!dragStart.current) return;
      setPos({
        x: Math.min(Math.max(0, e.clientX - dragStart.current.x), window.innerWidth - 80),
        y: Math.min(Math.max(0, e.clientY - dragStart.current.y), window.innerHeight - 30),
      });
    };
    const up = () => { dragStart.current = null; };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
    return () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
  }, []);

  return (
    <div className="floating-window" style={{ left: pos.x, top: pos.y, width, height }}>
      <div
        className="floating-window-header"
        onMouseDown={(e) => {
          if ((e.target as HTMLElement).closest('button')) return;
          dragStart.current = { x: e.clientX - pos.x, y: e.clientY - pos.y };
        }}
      >
        <span className="floating-window-title">{title}</span>
        <button className="floating-window-close" onClick={onClose}>&times;</button>
      </div>
      <div className="floating-window-body">{children}</div>
      {footer && <div className="floating-window-footer">{footer}</div>}
    </div>
  );
}
