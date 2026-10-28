import { useEffect, useState } from 'react';
import { Check } from 'lucide-react';
import { useAppStore } from '../../state/appStore';
import { useTranslation } from '../../i18n/useTranslation';
import type { FileLoadingEntry } from '../../state/slices/loadingSlice';
import './LoadingOverlay.css';

/** Loading steps by progress: the loaders report 0–0.2 reading, up to 0.7 converting, then the scene. */
const STEPS = [
  { key: 'stepRead', until: 0.2 },
  { key: 'stepConvert', until: 0.7 },
  { key: 'stepScene', until: 1.01 },
] as const;

/** Seconds without progress after which the bar shows that work goes on (long conversions). */
const BUSY_AFTER = 1.5;

function stepIndex(progress: number): number {
  const i = STEPS.findIndex((s) => progress < s.until);
  return i < 0 ? STEPS.length - 1 : i;
}

function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

function extension(fileName: string): string {
  const dot = fileName.lastIndexOf('.');
  return dot > 0 ? fileName.slice(dot + 1).toUpperCase() : '3D';
}

/**
 * Loading card over the viewport while files load: format badge, file name, elapsed time, the
 * three steps (read, convert, build the scene) and a progress bar that keeps moving during long
 * steps without progress reports (a Revit conversion is one step of up to a minute).
 */
export function LoadingOverlay() {
  const { t } = useTranslation('loader');
  const loading = useAppStore((s) => s.loadingFiles).filter((f) => !f.error);
  const [now, setNow] = useState(() => Date.now());

  // tick for the elapsed time and the busy detection
  useEffect(() => {
    if (loading.length === 0) return;
    const id = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(id);
  }, [loading.length]);

  if (loading.length === 0) return null;
  const [first, ...rest] = loading;

  return (
    <div className="loading-overlay" role="status" aria-live="polite">
      <div className="loading-card">
        <LoadingRow entry={first} now={now} t={t} detailed />
        {rest.length > 0 && (
          <div className="loading-more">
            {rest.map((f) => (
              <LoadingRow key={f.id} entry={f} now={now} t={t} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function LoadingRow({ entry, now, t, detailed = false }: {
  entry: FileLoadingEntry;
  now: number;
  t: (key: string, vars?: Record<string, unknown>) => string;
  detailed?: boolean;
}) {
  const progress = Math.min(Math.max(entry.progress, 0), 1);
  const current = stepIndex(progress);
  const busy = (now - entry.updatedAt) / 1000 > BUSY_AFTER && progress < 1;
  const ext = extension(entry.fileName);
  const slow = busy && ext === 'RVT' && current === 1;

  return (
    <div className={`loading-row ${detailed ? 'is-detailed' : 'is-compact'}`}>
      <div className="loading-head">
        <span className="loading-badge">{ext}</span>
        <span className="loading-name" title={entry.fileName}>{entry.fileName}</span>
        <span className="loading-time">{formatElapsed(now - entry.startedAt)}</span>
      </div>

      {detailed && (
        <ol className="loading-steps">
          {STEPS.map((s, i) => (
            <li key={s.key} className={i < current ? 'is-done' : i === current ? 'is-active' : 'is-pending'}>
              <span className="loading-step-dot">{i < current ? <Check size={10} strokeWidth={3} /> : i + 1}</span>
              <span className="loading-step-label">{t(s.key)}</span>
            </li>
          ))}
        </ol>
      )}

      <div className={`loading-bar ${busy ? 'is-busy' : ''}`}>
        <div className="loading-bar-fill" style={{ width: `${(progress * 100).toFixed(1)}%` }} />
      </div>

      <div className="loading-foot">
        <span className="loading-phase">{entry.phase}</span>
        <span className="loading-percent">{Math.round(progress * 100)}%</span>
      </div>
      {detailed && slow && <div className="loading-hint">{t('hintSlowRvt')}</div>}
    </div>
  );
}
