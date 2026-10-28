import { useEffect, useState } from 'react';
import { useAppStore } from '../../state/appStore';
import { useTranslation } from '../../i18n/useTranslation';
import { DENSITY_PRESETS, densityFor, presetForMaterial } from '../../engine/measure/measureObjects';
import { formatDensity } from '../../engine/measure/measureFormat';
import { FloatingWindow } from './FloatingWindow';

/**
 * Densities per material for weight measurements. Materials found in the
 * loaded models are listed; each uses a matching preset automatically unless
 * the user picks another preset or types a value. A weight stated in the file
 * (e.g. an IFC NetWeight) always wins.
 */
export function MaterialsDialog() {
  const { t } = useTranslation('ribbon');
  const setOpen = useAppStore((s) => s.setMaterialsDialogOpen);
  const weightSettings = useAppStore((s) => s.weightSettings);
  const setWeightSettings = useAppStore((s) => s.setWeightSettings);
  const setMaterialDensity = useAppStore((s) => s.setMaterialDensity);
  const models = useAppStore((s) => s.models);
  const [materials, setMaterials] = useState<string[]>([]);

  // Ask the viewer for the materials in the scene (again when models change)
  useEffect(() => {
    window.dispatchEvent(new CustomEvent('open3d:measure-materials', { detail: (names: string[]) => setMaterials(names) }));
  }, [models]);

  const presetName = (id: string) => t(`measure.materials.presets.${id}`);

  return (
    <FloatingWindow title={t('measure.materials.title')} onClose={() => setOpen(false)} width={560} height={420}>
      <div className="materials-intro">{t('measure.materials.intro')}</div>
      <label className="materials-default">
        <span>{t('measure.materials.defaultDensity')}</span>
        <input
          type="number"
          min={0}
          step={10}
          value={weightSettings.defaultDensity}
          onChange={(e) => setWeightSettings({ defaultDensity: Math.max(0, Number(e.target.value) || 0) })}
        />
        <span className="dim">kg/m³</span>
      </label>

      {materials.length === 0 ? (
        <div className="takeoff-empty">{t('measure.materials.none')}</div>
      ) : (
        <table className="takeoff-table">
          <thead>
            <tr>
              <th>{t('measure.takeoff.columns.material')}</th>
              <th>{t('measure.materials.preset')}</th>
              <th className="num">{t('measure.materials.density')}</th>
            </tr>
          </thead>
          <tbody>
            {materials.map((name) => {
              const custom = weightSettings.materialDensities[name];
              const auto = presetForMaterial(name);
              const current = densityFor(name, weightSettings);
              const matchingPreset = custom !== undefined ? DENSITY_PRESETS.find((p) => p.density === custom) : undefined;
              const selectValue = custom === undefined ? 'auto' : matchingPreset ? matchingPreset.id : 'custom';
              return (
                <tr key={name}>
                  <td title={name}>{name}</td>
                  <td>
                    <select
                      className="materials-select"
                      value={selectValue}
                      onChange={(e) => {
                        const v = e.target.value;
                        if (v === 'auto') setMaterialDensity(name, null);
                        else if (v === 'custom') setMaterialDensity(name, current.density);
                        else setMaterialDensity(name, DENSITY_PRESETS.find((p) => p.id === v)!.density);
                      }}
                    >
                      <option value="auto">
                        {auto
                          ? `${t('measure.materials.auto')}: ${presetName(auto.id)}`
                          : `${t('measure.materials.auto')}: ${t('measure.materials.useDefault')}`}
                      </option>
                      {DENSITY_PRESETS.map((p) => (
                        <option key={p.id} value={p.id}>{presetName(p.id)} ({formatDensity(p.density)})</option>
                      ))}
                      <option value="custom">{t('measure.materials.custom')}</option>
                    </select>
                  </td>
                  <td className="num">
                    <input
                      type="number"
                      min={0}
                      step={10}
                      className="materials-density"
                      value={current.density}
                      onChange={(e) => setMaterialDensity(name, Math.max(0, Number(e.target.value) || 0))}
                    />
                    <span className="dim"> kg/m³</span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </FloatingWindow>
  );
}
