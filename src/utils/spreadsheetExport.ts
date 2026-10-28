import { strToU8, zipSync } from 'fflate';

/**
 * Minimal spreadsheet export without extra dependencies: a real .xlsx
 * (SpreadsheetML zipped with fflate) and a CSV fallback.
 */

export type Cell = string | number | { value: number; decimals: number } | null;

export interface Sheet {
  name: string;
  /** First row is the header (bold) */
  rows: Cell[][];
  /** Column widths in characters */
  widths?: number[];
}

const MAX_DECIMALS = 4;

function escapeXml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

function columnName(index: number): string {
  let name = '';
  for (let n = index + 1; n > 0; n = Math.floor((n - 1) / 26)) {
    name = String.fromCharCode(65 + ((n - 1) % 26)) + name;
  }
  return name;
}

/** Style index: 0 = default, 1 = bold header, 2.. = number with 0..4 decimals */
function cellXml(cell: Cell, ref: string, header: boolean): string {
  if (cell === null || cell === '') return '';
  if (typeof cell === 'string') {
    return `<c r="${ref}" t="inlineStr"${header ? ' s="1"' : ''}><is><t xml:space="preserve">${escapeXml(cell)}</t></is></c>`;
  }
  const value = typeof cell === 'number' ? cell : cell.value;
  const decimals = typeof cell === 'number' ? 2 : Math.min(MAX_DECIMALS, Math.max(0, cell.decimals));
  if (!Number.isFinite(value)) return '';
  return `<c r="${ref}" s="${2 + decimals}"><v>${value}</v></c>`;
}

function sheetXml(sheet: Sheet): string {
  const cols = sheet.widths
    ? `<cols>${sheet.widths.map((w, i) => `<col min="${i + 1}" max="${i + 1}" width="${w}" customWidth="1"/>`).join('')}</cols>`
    : '';
  const rows = sheet.rows.map((row, r) =>
    `<row r="${r + 1}">${row.map((cell, c) => cellXml(cell, `${columnName(c)}${r + 1}`, r === 0)).join('')}</row>`,
  ).join('');
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>${cols}<sheetData>${rows}</sheetData></worksheet>`;
}

function stylesXml(): string {
  const formats = Array.from({ length: MAX_DECIMALS + 1 }, (_, d) =>
    `<numFmt numFmtId="${164 + d}" formatCode="${d === 0 ? '#,##0' : `#,##0.${'0'.repeat(d)}`}"/>`).join('');
  const xfs = Array.from({ length: MAX_DECIMALS + 1 }, (_, d) =>
    `<xf numFmtId="${164 + d}" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>`).join('');
  return `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="${MAX_DECIMALS + 1}">${formats}</numFmts><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="${MAX_DECIMALS + 3}"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>${xfs}</cellXfs></styleSheet>`;
}

/** Build an .xlsx workbook. */
export function buildXlsx(sheets: Sheet[]): Uint8Array {
  const safeName = (n: string, i: number) => escapeXml(n.replace(/[[\]*?:/\\]/g, ' ').slice(0, 31) || `Sheet${i + 1}`);
  const files: Record<string, Uint8Array> = {
    '[Content_Types].xml': strToU8(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>${sheets.map((_, i) => `<Override PartName="/xl/worksheets/sheet${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`).join('')}</Types>`),
    '_rels/.rels': strToU8(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>`),
    'xl/workbook.xml': strToU8(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>${sheets.map((s, i) => `<sheet name="${safeName(s.name, i)}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`).join('')}</sheets></workbook>`),
    'xl/_rels/workbook.xml.rels': strToU8(`<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${sheets.map((_, i) => `<Relationship Id="rId${i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${i + 1}.xml"/>`).join('')}<Relationship Id="rId${sheets.length + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>`),
    'xl/styles.xml': strToU8(stylesXml()),
  };
  sheets.forEach((s, i) => { files[`xl/worksheets/sheet${i + 1}.xml`] = strToU8(sheetXml(s)); });
  return zipSync(files);
}

/**
 * CSV with a semicolon separator and a BOM, so Excel with a Dutch locale opens
 * it directly. Numbers use the given decimal separator.
 */
export function buildCsv(sheet: Sheet, decimalSeparator = ','): Uint8Array {
  const text = sheet.rows.map((row) => row.map((cell) => {
    if (cell === null) return '';
    if (typeof cell === 'string') return /[;"\n]/.test(cell) ? `"${cell.replace(/"/g, '""')}"` : cell;
    const value = typeof cell === 'number' ? cell : cell.value;
    const decimals = typeof cell === 'number' ? 2 : cell.decimals;
    return value.toFixed(decimals).replace('.', decimalSeparator);
  }).join(';')).join('\r\n');
  return strToU8('﻿' + text);
}

/**
 * Save bytes to a file: a save dialog in the desktop app, a download in the browser.
 */
export async function saveFile(bytes: Uint8Array, fileName: string, filter: { name: string; extensions: string[] }): Promise<boolean> {
  if ((window as any).__TAURI_INTERNALS__) {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({ defaultPath: fileName, filters: [filter] });
      if (!path) return false;
      const { writeFile } = await import('@tauri-apps/plugin-fs');
      await writeFile(path, bytes);
      return true;
    } catch (err) {
      console.warn('Saving via the desktop dialog failed, falling back to a download', err);
    }
  }
  const blob = new Blob([bytes as BlobPart], { type: 'application/octet-stream' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = fileName;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return true;
}
