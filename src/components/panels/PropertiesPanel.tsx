import { X } from 'lucide-react';
import { useAppStore } from '../../state/appStore';
import { formatPoints } from '../../utils/format';
import { useTranslation } from '../../i18n/useTranslation';

export function PropertiesPanel() {
  const { t } = useTranslation('panels');
  const models = useAppStore((s) => s.models);
  const activeModelId = useAppStore((s) => s.activeModelId);
  const setActiveModelId = useAppStore((s) => s.setActiveModelId);
  const removeModel = useAppStore((s) => s.removeModel);
  const selectedObject = useAppStore((s) => s.selectedObject);
  const activeModel = models.find((m) => m.id === activeModelId);

  const handleRemoveModel = (id: string) => {
    // Remove from 3D scene
    window.dispatchEvent(new CustomEvent('open3d:remove-model', { detail: { id } }));
    // Remove from store
    removeModel(id);
    // Clear layers if no models left
    if (models.length <= 1) {
      useAppStore.getState().setLayers([]);
    }
  };

  return (
    <div className="p-3 text-xs overflow-y-auto h-full">
      {/* Model list */}
      <div className="mb-3">
        <h3 className="text-[10px] font-semibold text-cad-text-muted uppercase tracking-wide mb-2">{t('properties.models')}</h3>
        {models.length === 0 ? (
          <div className="text-cad-text-muted italic py-2">{t('properties.noModels')}</div>
        ) : (
          <div className="flex flex-col gap-1">
            {models.map((m) => (
              <div
                key={m.id}
                className={`flex items-center gap-2 px-2 py-1.5 rounded text-left w-full transition-colors ${
                  m.id === activeModelId
                    ? 'bg-cad-accent/20 text-cad-text'
                    : 'hover:bg-cad-hover text-cad-text-dim'
                }`}
              >
                <button
                  className="truncate flex-1 text-left"
                  onClick={() => setActiveModelId(m.id)}
                >
                  {m.fileName}
                </button>
                <span className="text-cad-text-muted flex-shrink-0">{m.format}</span>
                <button
                  className="flex-shrink-0 w-4 h-4 flex items-center justify-center rounded hover:bg-[var(--theme-hover-strong)] text-cad-text-muted hover:text-[var(--theme-danger-color)] transition-colors"
                  title={t('properties.removeModel')}
                  onClick={(e) => { e.stopPropagation(); handleRemoveModel(m.id); }}
                >
                  <X size={12} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Selected object */}
      {selectedObject && (
        <div className="mb-3">
          <h3 className="text-[10px] font-semibold text-cad-text-muted uppercase tracking-wide mb-2">{t('properties.selectedObject')}</h3>
          <div className="flex flex-col gap-1">
            <div className="panel-prop-row">
              <span className="panel-prop-label">{t('properties.name')}</span>
              <span className="panel-prop-value text-[var(--theme-accent)] text-left flex-1 truncate">{selectedObject.name}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">{t('properties.triangles')}</span>
              <span className="panel-prop-value">{formatPoints(selectedObject.triangles)}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">{t('properties.size')}</span>
              <span className="panel-prop-value">{selectedObject.sizeX} x {selectedObject.sizeY} x {selectedObject.sizeZ} m</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">{t('properties.center')}</span>
              <span className="panel-prop-value">{selectedObject.centerX}, {selectedObject.centerY}, {selectedObject.centerZ}</span>
            </div>
          </div>
        </div>
      )}

      {/* Active model properties */}
      {activeModel && (
        <div>
          <h3 className="text-[10px] font-semibold text-cad-text-muted uppercase tracking-wide mb-2">{t('properties.title')}</h3>
          <div className="flex flex-col gap-1">
            <div className="panel-prop-row">
              <span className="panel-prop-label">{t('properties.file')}</span>
              <span className="panel-prop-value text-left flex-1 truncate">{activeModel.fileName}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">{t('properties.format')}</span>
              <span className="panel-prop-value">{activeModel.format}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">{t('properties.vertices')}</span>
              <span className="panel-prop-value">{formatPoints(activeModel.vertices)}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">{t('properties.triangles')}</span>
              <span className="panel-prop-value">{formatPoints(activeModel.triangles)}</span>
            </div>
            {activeModel.loadingProgress < 1.0 && (
              <div className="panel-prop-row">
                <span className="panel-prop-label">{t('properties.loading')}</span>
                <span className="panel-prop-value text-[var(--theme-accent)]">
                  {(activeModel.loadingProgress * 100).toFixed(0)}% - {activeModel.loadingPhase}
                </span>
              </div>
            )}

            {/* Bounding box */}
            <h4 className="text-[10px] font-semibold text-cad-text-muted uppercase tracking-wide mt-2 mb-1">{t('properties.bounds')}</h4>
            <div className="panel-prop-row">
              <span className="panel-prop-label">Min X</span>
              <span className="panel-prop-value">{activeModel.bounds.minX.toFixed(2)}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">Max X</span>
              <span className="panel-prop-value">{activeModel.bounds.maxX.toFixed(2)}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">Min Y</span>
              <span className="panel-prop-value">{activeModel.bounds.minY.toFixed(2)}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">Max Y</span>
              <span className="panel-prop-value">{activeModel.bounds.maxY.toFixed(2)}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">Min Z</span>
              <span className="panel-prop-value">{activeModel.bounds.minZ.toFixed(2)}</span>
            </div>
            <div className="panel-prop-row">
              <span className="panel-prop-label">Max Z</span>
              <span className="panel-prop-value">{activeModel.bounds.maxZ.toFixed(2)}</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
