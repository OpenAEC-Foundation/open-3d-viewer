import { RotateCcw } from 'lucide-react';
import { useAppStore } from '../../../state/appStore';
import type { ExplodeAxis } from '../../../state/slices';
import { RibbonSmallButton, RibbonGroup, RibbonButtonStack } from './RibbonComponents';
import { ExplodeStoreysIcon, ExplodeElementsIcon } from './RibbonIcons';
import { useTranslation } from '../../../i18n/useTranslation';

const AXES: ExplodeAxis[] = ['x', 'y', 'z'];

/**
 * Exploded view: X/Y/Z sliders that pull the model apart, per building storey
 * (IFC) or per element.
 */
export function ExplodeRibbonGroup() {
  const explodeMode = useAppStore((s) => s.explodeMode);
  const setExplodeMode = useAppStore((s) => s.setExplodeMode);
  const explode = useAppStore((s) => s.explode);
  const setExplode = useAppStore((s) => s.setExplode);
  const resetExplode = useAppStore((s) => s.resetExplode);
  const storeyCount = useAppStore((s) => s.explodeStoreyCount);
  const { t } = useTranslation('ribbon');

  // Without storey data the viewer explodes per element
  const mode = explodeMode === 'storeys' && storeyCount > 0 ? 'storeys' : 'elements';
  const exploded = explode.x > 0 || explode.y > 0 || explode.z > 0;

  return (
    <RibbonGroup label={t('groups.explode')}>
      <RibbonButtonStack>
        <RibbonSmallButton
          icon={<ExplodeStoreysIcon />}
          label={storeyCount > 0 ? t('buttons.explodeStoreysCount', { count: storeyCount }) : t('buttons.explodeStoreys')}
          onClick={() => setExplodeMode('storeys')}
          active={mode === 'storeys'}
          disabled={storeyCount === 0}
        />
        <RibbonSmallButton icon={<ExplodeElementsIcon />} label={t('buttons.explodeElements')} onClick={() => setExplodeMode('elements')} active={mode === 'elements'} />
        <RibbonSmallButton icon={<RotateCcw size={14} />} label={t('buttons.reset')} onClick={resetExplode} disabled={!exploded} />
      </RibbonButtonStack>
      <div className="ribbon-slider-group">
        {AXES.map((axis) => (
          <label key={axis} className="ribbon-slider-row">
            <span className={`ribbon-slider-label axis axis-${axis}`}>{axis.toUpperCase()}</span>
            <input type="range" min="0" max="100" step="1" value={explode[axis]} onChange={(e) => setExplode(axis, Number(e.target.value))} className="ribbon-slider" />
            <span className="ribbon-slider-value">{explode[axis]}%</span>
          </label>
        ))}
      </div>
    </RibbonGroup>
  );
}
