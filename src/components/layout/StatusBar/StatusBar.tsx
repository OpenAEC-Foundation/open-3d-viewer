import { Fragment, memo, type ReactNode } from 'react';
import { useAppStore } from '../../../state/appStore';
import { formatPoints } from '../../../utils/format';
import { useTranslation } from '../../../i18n/useTranslation';
import './StatusBar.css';

function Item({ label, value, className = '', valueClass = 'mono', title }: {
  label?: string; value: ReactNode; className?: string; valueClass?: string; title?: string;
}) {
  return (
    <div className={`status-item ${className}`} title={title}>
      {label && <span className="status-item-label">{label}:</span>}
      <span className={`status-item-value ${valueClass}`}>{value}</span>
    </div>
  );
}

/** Joins items with the 1 × 14 px status separator. */
function withSeparators(items: ReactNode[]) {
  return items.filter(Boolean).map((it, i) => (
    <Fragment key={i}>
      {i > 0 && <div className="status-separator" />}
      {it}
    </Fragment>
  ));
}

export const StatusBar = memo(function StatusBar() {
  const { t } = useTranslation('statusBar');
  const models = useAppStore((s) => s.models);
  const rendererType = useAppStore((s) => s.rendererType);
  const fps = useAppStore((s) => s.fps);
  const loadingFiles = useAppStore((s) => s.loadingFiles);
  const selectedObject = useAppStore((s) => s.selectedObject);

  const totalVertices = models.reduce((sum, m) => sum + m.vertices, 0);
  const totalTriangles = models.reduce((sum, m) => sum + m.triangles, 0);
  const activeModel = models.find((m) => m.id === useAppStore.getState().activeModelId);

  // Separate loading from errors
  const loading = loadingFiles.filter((f) => !f.error);
  const errors = loadingFiles.filter((f) => f.error);

  const left = withSeparators([
    activeModel && <Item value={activeModel.fileName} />,
    <Item label={t('models')} value={models.length} />,
    <Item label={t('vertices')} value={formatPoints(totalVertices)} />,
    <Item label={t('triangles')} value={formatPoints(totalTriangles)} />,
    selectedObject && <Item label={t('selected')} value={selectedObject.name} valueClass="mono accent" />,
  ]);

  const right = withSeparators([
    ...errors.map((f) => (
      <Item className="is-error" value={`${f.fileName}: ${f.error}`} title={f.error ?? ''} />
    )),
    ...loading.map((f) => (
      <Item className="is-loading" label={t('loading')} value={`${f.fileName}: ${(f.progress * 100).toFixed(0)}%`} />
    )),
    <Item label={t('fps')} value={fps} />,
    <Item value={rendererType.toUpperCase()} valueClass={rendererType === 'webgpu' ? 'mono success' : 'mono'} />,
    // version number on the right (OpenAEC status bar), the build tag on hover
    <Item value={`v${__APP_VERSION__}`} title={`${__APP_VERSION__}+${__APP_BUILD__}`} />,
  ]);

  return (
    <div className="status-bar">
      <div className="status-bar-left">{left}</div>
      <div className="status-bar-right">{right}</div>
    </div>
  );
});
