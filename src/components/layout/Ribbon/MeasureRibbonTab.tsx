import { Undo2, Trash2, Save, Table2, Settings2 } from 'lucide-react';
import { useAppStore } from '../../../state/appStore';
import { TOOL_CATEGORY, type MeasureTool } from '../../../state/slices/measureSlice';
import type { MeasureCategory, UnitSettings } from '../../../engine/measure/measureFormat';
import { formatValue, UNIT_LABELS } from '../../../engine/measure/measureFormat';
import { useTranslation } from '../../../i18n/useTranslation';
import {
  RibbonButton, RibbonSmallButton, RibbonGroup, RibbonButtonStack,
  RibbonSplitButton, RibbonPanelButton, type RibbonMenuItem,
} from './RibbonComponents';
import {
  MeasureDistanceIcon, MeasurePerpendicularIcon, MeasurePointIcon,
  MeasureAreaIcon, MeasurePolygonIcon, MeasureVolumeIcon, MeasureWeightIcon,
  MeasureAngleIcon, MeasureCountIcon, MultiSelectIcon, MeasureUnitsIcon, MeasureComponentsIcon,
  SnapVertexIcon, SnapMidpointIcon, SnapEdgeIcon, SnapFaceIcon,
} from './RibbonIcons';

type Method = Exclude<MeasureTool, 'off'>;

const METHOD_ICONS: Record<Method, () => React.ReactElement> = {
  perpendicular: MeasurePerpendicularIcon,
  distance: MeasureDistanceIcon,
  point: MeasurePointIcon,
  area: MeasureAreaIcon,
  polygon: MeasurePolygonIcon,
  areaTotal: MeasureVolumeIcon,
  volume: MeasureVolumeIcon,
  weight: MeasureWeightIcon,
  angle: MeasureAngleIcon,
  angle3: MeasureAngleIcon,
  count: MeasureCountIcon,
  countEntity: SnapVertexIcon,
};

const CATEGORY_METHODS: { category: MeasureCategory; icon: () => React.ReactElement; methods: Method[] }[] = [
  { category: 'length', icon: MeasureDistanceIcon, methods: ['perpendicular', 'distance', 'point'] },
  { category: 'area', icon: MeasureAreaIcon, methods: ['area', 'polygon', 'areaTotal'] },
  { category: 'volume', icon: MeasureVolumeIcon, methods: ['volume'] },
  { category: 'weight', icon: MeasureWeightIcon, methods: ['weight'] },
  { category: 'angle', icon: MeasureAngleIcon, methods: ['angle', 'angle3'] },
  { category: 'count', icon: MeasureCountIcon, methods: ['count', 'countEntity'] },
];

export function MeasureRibbonTab() {
  const { t } = useTranslation('ribbon');
  const tool = useAppStore((s) => s.measureTool);
  const setTool = useAppStore((s) => s.setMeasureTool);
  const lastMethod = useAppStore((s) => s.measureLastMethod);
  const snap = useAppStore((s) => s.measureSnap);
  const setSnap = useAppStore((s) => s.setMeasureSnap);
  const multi = useAppStore((s) => s.measureMulti);
  const setMulti = useAppStore((s) => s.setMeasureMulti);
  const showComponents = useAppStore((s) => s.measureShowComponents);
  const setShowComponents = useAppStore((s) => s.setMeasureShowComponents);
  const measurements = useAppStore((s) => s.measurements);
  const takeoffRows = useAppStore((s) => s.takeoffRows);
  const setTakeoffOpen = useAppStore((s) => s.setTakeoffOpen);
  const setMaterialsDialogOpen = useAppStore((s) => s.setMaterialsDialogOpen);

  const activeCategory = tool === 'off' ? null : TOOL_CATEGORY[tool];
  const unsaved = measurements.filter((m) => !m.saved).length;

  const methodItem = (m: Method): RibbonMenuItem => {
    const Icon = METHOD_ICONS[m];
    return {
      id: m,
      label: t(`measure.methods.${m}.label`),
      description: t(`measure.methods.${m}.description`),
      icon: <Icon />,
      active: tool === m,
      onClick: () => setTool(m),
    };
  };

  const handleSave = () => {
    const state = useAppStore.getState();
    const group = t('measure.takeoff.defaultGroup', { n: state.takeoffGroupCounter + 1 });
    const ids = state.saveMeasurementsToTakeoff(group);
    if (ids.length > 0) window.dispatchEvent(new CustomEvent('open3d:measure-saved', { detail: ids }));
    setTakeoffOpen(true);
  };

  return (
    <div className="ribbon-groups">
      <RibbonGroup label={t('groups.measureTools')}>
        {CATEGORY_METHODS.map(({ category, icon: Icon, methods }) => {
          const current = activeCategory === category ? (tool as Method) : lastMethod[category];
          const items = methods.map(methodItem);
          if (category === 'weight') {
            items.push({
              id: 'materials',
              label: t('measure.materials.menu'),
              description: t('measure.materials.menuDescription'),
              icon: <Settings2 size={16} />,
              separator: true,
              onClick: () => setMaterialsDialogOpen(true),
            });
          }
          return (
            <RibbonSplitButton
              key={category}
              icon={<Icon />}
              label={t(`measure.categories.${category}`)}
              tooltip={`${t(`measure.categories.${category}`)}: ${t(`measure.methods.${current}.label`)}`}
              active={activeCategory === category}
              onClick={() => setTool(activeCategory === category ? 'off' : lastMethod[category])}
              items={items}
            />
          );
        })}
      </RibbonGroup>

      <RibbonGroup label={t('groups.measureSelection')}>
        <RibbonButton
          icon={<MultiSelectIcon />}
          label={t('measure.multi')}
          onClick={() => setMulti(!multi)}
          active={multi}
          tooltip={t('measure.multiTooltip')}
        />
      </RibbonGroup>

      <RibbonGroup label={t('groups.measureSnap')}>
        <RibbonButtonStack>
          <RibbonSmallButton icon={<SnapVertexIcon />} label={t('measure.snap.vertex')} onClick={() => setSnap({ vertex: !snap.vertex })} active={snap.vertex} />
          <RibbonSmallButton icon={<SnapMidpointIcon />} label={t('measure.snap.midpoint')} onClick={() => setSnap({ midpoint: !snap.midpoint })} active={snap.midpoint} />
        </RibbonButtonStack>
        <RibbonButtonStack>
          <RibbonSmallButton icon={<SnapEdgeIcon />} label={t('measure.snap.edge')} onClick={() => setSnap({ edge: !snap.edge })} active={snap.edge} />
          <RibbonSmallButton icon={<SnapFaceIcon />} label={t('measure.snap.face')} onClick={() => setSnap({ face: !snap.face })} active={snap.face} />
        </RibbonButtonStack>
      </RibbonGroup>

      <RibbonGroup label={t('groups.measureDisplay')}>
        <RibbonPanelButton icon={<MeasureUnitsIcon />} label={t('measure.units.button')} tooltip={t('measure.units.tooltip')}>
          <MeasureUnitsPanel />
        </RibbonPanelButton>
        <RibbonButtonStack>
          <RibbonSmallButton icon={<MeasureComponentsIcon />} label={t('measure.components')} onClick={() => setShowComponents(!showComponents)} active={showComponents} />
        </RibbonButtonStack>
      </RibbonGroup>

      <RibbonGroup label={t('groups.measureManage')}>
        <RibbonButtonStack>
          <RibbonSmallButton
            icon={<Undo2 size={14} />}
            label={t('measure.removeLast')}
            onClick={() => window.dispatchEvent(new CustomEvent('open3d:measure-undo'))}
            disabled={measurements.length === 0}
          />
          <RibbonSmallButton
            icon={<Trash2 size={14} />}
            label={t('measure.clear')}
            onClick={() => window.dispatchEvent(new CustomEvent('open3d:measure-clear'))}
            disabled={measurements.length === 0}
          />
        </RibbonButtonStack>
        <RibbonButton
          icon={<Save size={20} />}
          label={t('measure.save')}
          onClick={handleSave}
          disabled={unsaved === 0}
          tooltip={t('measure.saveTooltip')}
        />
        <RibbonButton
          icon={<Table2 size={20} />}
          label={t('measure.takeoff.button')}
          onClick={() => setTakeoffOpen(true)}
          tooltip={t('measure.takeoff.tooltip', { n: takeoffRows.length })}
        />
      </RibbonGroup>
    </div>
  );
}

// ============================================================================
// Units & decimals
// ============================================================================

const DECIMALS = [0, 1, 2, 3, 4];

function MeasureUnitsPanel() {
  const { t } = useTranslation('ribbon');
  const units = useAppStore((s) => s.measureUnits);
  const setUnits = useAppStore((s) => s.setMeasureUnits);

  const rows: { key: keyof UnitSettings; options: string[]; sample: number }[] = [
    { key: 'length', options: ['mm', 'cm', 'm'], sample: 1.23456 },
    { key: 'area', options: ['mm2', 'cm2', 'm2'], sample: 12.3456 },
    { key: 'volume', options: ['cm3', 'dm3', 'm3'], sample: 1.23456 },
    { key: 'weight', options: ['kg', 't'], sample: 1234.56 },
    { key: 'angle', options: [], sample: Math.PI / 7 },
  ];

  return (
    <div style={{ minWidth: 280 }}>
      <div className="ribbon-panel-title">{t('measure.units.title')}</div>
      <div className="ribbon-panel-grid">
        <span className="head" />
        <span className="head">{t('measure.units.unit')}</span>
        <span className="head">{t('measure.units.decimals')}</span>
        {rows.map(({ key, options, sample }) => {
          const current = units[key] as { unit?: string; decimals: number };
          return (
            <div key={key} style={{ display: 'contents' }}>
              <span>{t(`measure.categories.${key}`)}</span>
              {options.length > 0 ? (
                <select
                  value={current.unit}
                  onChange={(e) => setUnits({ [key]: { ...current, unit: e.target.value } } as Partial<UnitSettings>)}
                >
                  {options.map((o) => <option key={o} value={o}>{UNIT_LABELS[o]}</option>)}
                </select>
              ) : (
                <span style={{ color: 'var(--theme-text-dim)' }}>°</span>
              )}
              <select
                value={current.decimals}
                onChange={(e) => setUnits({ [key]: { ...current, decimals: Number(e.target.value) } } as Partial<UnitSettings>)}
                title={formatValue(key as MeasureCategory, sample, units)}
              >
                {DECIMALS.map((d) => <option key={d} value={d}>{d}</option>)}
              </select>
            </div>
          );
        })}
      </div>
      <div className="ribbon-panel-preview">
        {t('measure.units.example')}: {formatValue('length', 1.23456, units)} · {formatValue('area', 12.3456, units)} · {formatValue('angle', Math.PI / 7, units)}
      </div>
    </div>
  );
}
