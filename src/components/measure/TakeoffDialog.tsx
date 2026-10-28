import { useMemo, useState } from 'react';
import { Trash2, FileSpreadsheet, FileText } from 'lucide-react';
import { useAppStore } from '../../state/appStore';
import type { TakeoffRow } from '../../state/slices/measureSlice';
import { useTranslation } from '../../i18n/useTranslation';
import {
  formatCoordinate, formatNumber, formatValue, toDisplay,
  type MeasureCategory, type UnitSettings,
} from '../../engine/measure/measureFormat';
import { buildCsv, buildXlsx, saveFile, type Cell, type Sheet } from '../../utils/spreadsheetExport';
import { FloatingWindow } from './FloatingWindow';

/** Categories whose values can be added up */
const SUMMABLE: MeasureCategory[] = ['length', 'area', 'volume', 'weight', 'count'];

function rowValueText(row: TakeoffRow, units: UnitSettings): string {
  if (row.method === 'point' && row.detail) {
    const [x, y, z] = row.detail.split('|').map(Number);
    return `${formatCoordinate(x, units)}; ${formatCoordinate(y, units)}; ${formatCoordinate(z, units)} ${units.length.unit}`;
  }
  return formatValue(row.category, row.value, units);
}

/** Sum per category of the given rows (summable categories only) */
function totals(rows: TakeoffRow[]): { category: MeasureCategory; count: number; total: number }[] {
  const map = new Map<MeasureCategory, { count: number; total: number }>();
  for (const r of rows) {
    if (!SUMMABLE.includes(r.category) || r.method === 'point') continue;
    const entry = map.get(r.category) ?? { count: 0, total: 0 };
    entry.count += r.category === 'count' ? r.value : 1;
    entry.total += r.value;
    map.set(r.category, entry);
  }
  return SUMMABLE.filter((c) => map.has(c)).map((c) => ({ category: c, ...map.get(c)! }));
}

/**
 * Takeoff (uittrekstaat): saved measurements in a table with names and
 * groups, exportable to Excel. Kept for this session only.
 */
export function TakeoffDialog() {
  const { t } = useTranslation('ribbon');
  const rows = useAppStore((s) => s.takeoffRows);
  const units = useAppStore((s) => s.measureUnits);
  const setOpen = useAppStore((s) => s.setTakeoffOpen);
  const updateRow = useAppStore((s) => s.updateTakeoffRow);
  const removeRow = useAppStore((s) => s.removeTakeoffRow);
  const clearTakeoff = useAppStore((s) => s.clearTakeoff);
  const [confirmClear, setConfirmClear] = useState(false);

  const groups = useMemo(() => {
    const map = new Map<string, TakeoffRow[]>();
    for (const r of rows) {
      const list = map.get(r.group);
      if (list) list.push(r); else map.set(r.group, [r]);
    }
    return [...map.entries()];
  }, [rows]);

  const defaultName = (r: TakeoffRow) => r.objectName || t(`measure.methods.${r.method}.label`);
  const methodText = (r: TakeoffRow) => `${t(`measure.categories.${r.category}`)} · ${t(`measure.methods.${r.method}.label`)}`;
  const detailText = (r: TakeoffRow) => {
    if (!r.detail || r.method === 'point') return '';
    if (r.detail === 'curved') return t('measure.label.curved');
    if (r.method === 'countEntity') return t(`measure.snap.${r.detail}`);
    return r.detail;
  };
  const totalText = (category: MeasureCategory, count: number, total: number) =>
    category === 'count'
      ? `${t(`measure.categories.${category}`)}: ${formatNumber(total, 0)}`
      : `${t(`measure.categories.${category}`)}: ${formatValue(category, total, units)} (${count}×)`;

  const buildSheets = (): Sheet[] => {
    const numberCell = (r: TakeoffRow): Cell => {
      if (r.method === 'point') return null;
      const d = toDisplay(r.category, r.value, units);
      return { value: d.value, decimals: d.decimals };
    };
    const measurements: Sheet = {
      name: t('measure.takeoff.sheetMeasurements'),
      widths: [18, 28, 14, 26, 14, 8, 30, 22, 30],
      rows: [
        [
          t('measure.takeoff.columns.group'), t('measure.takeoff.columns.name'), t('measure.takeoff.columns.category'),
          t('measure.takeoff.columns.method'), t('measure.takeoff.columns.value'), t('measure.takeoff.columns.unit'),
          t('measure.takeoff.columns.object'), t('measure.takeoff.columns.material'), t('measure.takeoff.columns.detail'),
        ],
        ...rows.map((r): Cell[] => [
          r.group, r.name || defaultName(r), t(`measure.categories.${r.category}`), t(`measure.methods.${r.method}.label`),
          numberCell(r), r.method === 'point' ? units.length.unit : toDisplay(r.category, r.value, units).unit,
          r.objectName ?? '', r.material ?? '', r.method === 'point' ? rowValueText(r, units) : detailText(r),
        ]),
      ],
    };
    const totalRows: Cell[][] = [];
    for (const [group, groupRows] of groups) {
      for (const tot of totals(groupRows)) {
        const d = toDisplay(tot.category, tot.total, units);
        totalRows.push([group, t(`measure.categories.${tot.category}`), tot.count, { value: d.value, decimals: d.decimals }, d.unit]);
      }
    }
    const summary: Sheet = {
      name: t('measure.takeoff.sheetTotals'),
      widths: [22, 16, 12, 16, 8],
      rows: [
        [
          t('measure.takeoff.columns.group'), t('measure.takeoff.columns.category'), t('measure.takeoff.columns.count'),
          t('measure.takeoff.columns.total'), t('measure.takeoff.columns.unit'),
        ],
        ...totalRows,
      ],
    };
    return [measurements, summary];
  };

  const stamp = new Date().toISOString().slice(0, 10);
  const exportXlsx = () => saveFile(buildXlsx(buildSheets()), `${t('measure.takeoff.fileName')}-${stamp}.xlsx`, { name: 'Excel', extensions: ['xlsx'] });
  const exportCsv = () => {
    const decimalSeparator = formatNumber(1.5, 1).includes(',') ? ',' : '.';
    return saveFile(buildCsv(buildSheets()[0], decimalSeparator), `${t('measure.takeoff.fileName')}-${stamp}.csv`, { name: 'CSV', extensions: ['csv'] });
  };

  const allTotals = totals(rows);

  return (
    <FloatingWindow
      title={t('measure.takeoff.title')}
      onClose={() => setOpen(false)}
      width={760}
      height={440}
      footer={
        <>
          <div className="takeoff-totals">
            {allTotals.map((tot) => <span key={tot.category}>{totalText(tot.category, tot.count, tot.total)}</span>)}
          </div>
          <div className="takeoff-actions">
            {confirmClear ? (
              <>
                <span className="takeoff-confirm">{t('measure.takeoff.confirmClear')}</span>
                <button className="takeoff-btn danger" onClick={() => { clearTakeoff(); setConfirmClear(false); }}>{t('measure.takeoff.yesClear')}</button>
                <button className="takeoff-btn" onClick={() => setConfirmClear(false)}>{t('measure.takeoff.cancel')}</button>
              </>
            ) : (
              <>
                <button className="takeoff-btn" onClick={() => setConfirmClear(true)} disabled={rows.length === 0}>
                  <Trash2 size={13} /> {t('measure.takeoff.clear')}
                </button>
                <button className="takeoff-btn" onClick={exportCsv} disabled={rows.length === 0}>
                  <FileText size={13} /> CSV
                </button>
                <button className="takeoff-btn primary" onClick={exportXlsx} disabled={rows.length === 0}>
                  <FileSpreadsheet size={13} /> {t('measure.takeoff.exportExcel')}
                </button>
              </>
            )}
          </div>
        </>
      }
    >
      {rows.length === 0 ? (
        <div className="takeoff-empty">{t('measure.takeoff.empty')}</div>
      ) : (
        <table className="takeoff-table">
          <thead>
            <tr>
              <th style={{ width: '26%' }}>{t('measure.takeoff.columns.name')}</th>
              <th>{t('measure.takeoff.columns.method')}</th>
              <th className="num">{t('measure.takeoff.columns.value')}</th>
              <th>{t('measure.takeoff.columns.object')}</th>
              <th>{t('measure.takeoff.columns.material')}</th>
              <th>{t('measure.takeoff.columns.detail')}</th>
              <th style={{ width: 28 }} />
            </tr>
          </thead>
          {groups.map(([group, groupRows]) => (
            <tbody key={groupRows[0].id}>
              <tr className="takeoff-group">
                <td colSpan={7}>
                  <input
                    className="takeoff-group-input"
                    value={group}
                    onChange={(e) => groupRows.forEach((r) => updateRow(r.id, { group: e.target.value }))}
                    title={t('measure.takeoff.renameGroup')}
                  />
                  <span className="takeoff-group-totals">
                    {totals(groupRows).map((tot) => <span key={tot.category}>{totalText(tot.category, tot.count, tot.total)}</span>)}
                  </span>
                </td>
              </tr>
              {groupRows.map((r) => (
                <tr key={r.id}>
                  <td>
                    <input
                      className="takeoff-name-input"
                      value={r.name}
                      placeholder={defaultName(r)}
                      onChange={(e) => updateRow(r.id, { name: e.target.value })}
                    />
                  </td>
                  <td className="dim">{methodText(r)}</td>
                  <td className="num">{rowValueText(r, units)}</td>
                  <td className="dim">{r.objectName ?? ''}</td>
                  <td className="dim">{r.material ?? ''}</td>
                  <td className="dim">{detailText(r)}</td>
                  <td>
                    <button className="takeoff-row-delete" onClick={() => removeRow(r.id)} title={t('measure.delete')}>&times;</button>
                  </td>
                </tr>
              ))}
            </tbody>
          ))}
        </table>
      )}
    </FloatingWindow>
  );
}
