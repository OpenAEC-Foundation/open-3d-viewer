import { memo } from 'react';
import { useAppStore } from '../../../state/appStore';
import { formatPoints } from '../../../utils/format';
import { useTranslation } from '../../../i18n/useTranslation';

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

  return (
    <div className="h-6 bg-cad-surface border-t border-cad-border flex items-center px-3 text-xs text-cad-text-dim gap-6">
      {/* Active file */}
      {activeModel && (
        <div className="flex items-center gap-2">
          <span className="text-cad-text font-mono">{activeModel.fileName}</span>
        </div>
      )}

      {/* Models count */}
      <div className="flex items-center gap-2">
        <span>{t('models')}:</span>
        <span className="text-cad-text font-mono">{models.length}</span>
      </div>

      {/* Total vertices */}
      <div className="flex items-center gap-2">
        <span>{t('vertices')}:</span>
        <span className="text-cad-text font-mono">{formatPoints(totalVertices)}</span>
      </div>

      {/* Total triangles */}
      <div className="flex items-center gap-2">
        <span>{t('triangles')}:</span>
        <span className="text-cad-text font-mono">{formatPoints(totalTriangles)}</span>
      </div>

      {/* Selected object */}
      {selectedObject && (
        <div className="flex items-center gap-2">
          <span>{t('selected')}:</span>
          <span className="text-blue-400 font-mono">{selectedObject.name}</span>
        </div>
      )}

      {/* Spacer */}
      <div className="flex-1" />

      {/* Error status */}
      {errors.length > 0 && (
        <div className="flex items-center gap-2 text-red-400">
          {errors.map((f) => (
            <span key={f.id} className="font-mono truncate max-w-[300px]" title={f.error ?? ''}>
              {f.fileName}: {f.error}
            </span>
          ))}
        </div>
      )}

      {/* Loading status */}
      {loading.length > 0 && (
        <div className="flex items-center gap-2 text-yellow-400">
          <span>{t('loading')}</span>
          {loading.map((f) => (
            <span key={f.id} className="font-mono">
              {f.fileName}: {(f.progress * 100).toFixed(0)}%
            </span>
          ))}
        </div>
      )}

      {/* FPS */}
      <div className="flex items-center gap-2">
        <span>{t('fps')}:</span>
        <span className="text-cad-text font-mono">{fps}</span>
      </div>

      {/* Renderer type */}
      <div className="flex items-center gap-1">
        <span className={rendererType === 'webgpu' ? 'text-green-400' : 'text-cad-text'}>
          {rendererType.toUpperCase()}
        </span>
      </div>
    </div>
  );
});
