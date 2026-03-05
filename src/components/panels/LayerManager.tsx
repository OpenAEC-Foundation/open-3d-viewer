import { Eye, EyeOff } from 'lucide-react';
import { useAppStore } from '../../state/appStore';
import { formatPoints } from '../../utils/format';
import { useTranslation } from '../../i18n/useTranslation';

export function LayerManager() {
  const { t } = useTranslation('panels');
  const layers = useAppStore((s) => s.layers);
  const setLayerVisible = useAppStore((s) => s.setLayerVisible);
  const setMultipleLayersVisible = useAppStore((s) => s.setMultipleLayersVisible);

  const allVisible = layers.length > 0 && layers.every((l) => l.visible);

  const toggleAll = () => {
    const newVisible = !allVisible;
    setMultipleLayersVisible(layers.map((l) => ({ name: l.name, visible: newVisible })));
  };

  if (layers.length === 0) {
    return (
      <div className="p-3 text-xs text-cad-text-muted italic">
        {t('layers.noLayers')}
      </div>
    );
  }

  const totalTriangles = layers.reduce((sum, l) => sum + l.triangles, 0);
  const visibleTriangles = layers.filter((l) => l.visible).reduce((sum, l) => sum + l.triangles, 0);

  return (
    <div className="flex flex-col h-full text-xs">
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-3 py-1.5 border-b border-cad-border bg-cad-surface">
        <button
          onClick={toggleAll}
          className="flex items-center gap-1 px-2 py-0.5 rounded hover:bg-cad-hover text-cad-text-dim hover:text-cad-text transition-colors"
          title={allVisible ? t('layers.hideAll') : t('layers.showAll')}
        >
          {allVisible ? <EyeOff size={12} /> : <Eye size={12} />}
          <span>{allVisible ? t('layers.hideAll') : t('layers.showAll')}</span>
        </button>
        <div className="flex-1" />
        <span className="text-cad-text-muted font-mono">
          {layers.filter((l) => l.visible).length}/{layers.length}
        </span>
      </div>

      {/* Stats */}
      <div className="px-3 py-1 text-cad-text-muted border-b border-cad-border bg-cad-surface/50">
        {t('layers.trianglesVisible', { visible: formatPoints(visibleTriangles), total: formatPoints(totalTriangles) })}
      </div>

      {/* Layer list */}
      <div className="flex-1 overflow-y-auto">
        {layers.map((layer) => (
          <div
            key={layer.name}
            className={`flex items-center gap-2 px-3 py-1 hover:bg-cad-hover cursor-pointer border-b border-cad-border/30 ${
              !layer.visible ? 'opacity-50' : ''
            }`}
            onClick={() => setLayerVisible(layer.name, !layer.visible)}
          >
            <button
              className={`flex-shrink-0 w-4 h-4 flex items-center justify-center rounded ${
                layer.visible ? 'text-cad-text' : 'text-cad-text-muted'
              }`}
              title={layer.visible ? t('layers.hide') : t('layers.show')}
            >
              {layer.visible ? <Eye size={12} /> : <EyeOff size={12} />}
            </button>
            <span className={`truncate flex-1 ${layer.visible ? 'text-cad-text' : 'text-cad-text-muted line-through'}`}>
              {layer.name}
            </span>
            <span className="text-cad-text-muted flex-shrink-0 font-mono text-[10px]">
              {formatPoints(layer.triangles)}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
