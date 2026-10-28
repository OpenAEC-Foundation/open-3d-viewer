import { useAppStore } from '../../state/appStore';
import { useTranslation } from '../../i18n/useTranslation';
import { formatNumber, formatValue, type MeasureCategory } from '../../engine/measure/measureFormat';
import './measure.css';

const TOTAL_CATEGORIES: MeasureCategory[] = ['length', 'area', 'volume', 'weight', 'count'];
/** Tools that take two picks; the hint changes after the first one */
const TWO_PICK = new Set(['distance', 'perpendicular', 'angle']);

/**
 * Overlay at the top of the viewport: what to click next for the active tool,
 * and — with multiple selection or counting — the running totals.
 */
export function MeasureHud() {
  const { t } = useTranslation('ribbon');
  const tool = useAppStore((s) => s.measureTool);
  const stage = useAppStore((s) => s.measureStage);
  const multi = useAppStore((s) => s.measureMulti);
  const measurements = useAppStore((s) => s.measurements);
  const units = useAppStore((s) => s.measureUnits);

  let hint = '';
  if (tool !== 'off') {
    if (TWO_PICK.has(tool)) hint = t(`measure.methods.${tool}.${stage > 0 ? 'hintSecond' : 'hint'}`);
    else if (tool === 'angle3') hint = t(`measure.methods.angle3.hint${Math.min(stage, 2)}`);
    else hint = t(`measure.methods.${tool}.hint`, { n: stage });
    hint += ` — ${t('measure.hints.escape')}`;
  }

  // Totals of the current (unsaved) selection
  const current = measurements.filter((m) => !m.saved && m.method !== 'point');
  const totals = TOTAL_CATEGORIES.map((category) => {
    const items = current.filter((m) => m.category === category);
    return { category, count: items.length, total: items.reduce((s, m) => s + m.value, 0) };
  }).filter((x) => x.count > 0 && (multi || x.category === 'count'));

  if (!hint && totals.length === 0) return null;

  return (
    <div className="measure-hud">
      {hint && <div className="measure-hint">{hint}</div>}
      {totals.length > 0 && (
        <div className="measure-totals">
          {totals.map(({ category, count, total }) => (
            <span key={category} className="measure-total">
              <span className="measure-total-label">
                {category === 'count' ? t('measure.label.countTotal') : `${t(`measure.categories.${category}`)} (${count}×)`}
              </span>
              <span className="measure-total-value">
                {category === 'count' ? formatNumber(total, 0) : formatValue(category, total, units)}
              </span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
