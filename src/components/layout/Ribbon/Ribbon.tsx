import { useState, useEffect, memo } from 'react';
import {
  Download, Sun, Grid3x3, Lightbulb, RotateCcw,
  Camera, MoveVertical, Gauge, Crosshair,
} from 'lucide-react';
import { useAppStore } from '../../../state/appStore';
import { RibbonButton, RibbonSmallButton, RibbonGroup, RibbonButtonStack, ThemeSelector } from './RibbonComponents';
import { WireframeIcon, SolidIcon, PointsIcon, HiddenLineIcon, SectionBoxIcon, OrbitIcon, WalkIcon, PerspectiveIcon, OrthoIcon } from './RibbonIcons';
import { useViewerActions } from './useViewerActions';
import { useTranslation } from '../../../i18n/useTranslation';
import { Backstage } from '../Backstage/Backstage';
import './Ribbon.css';

type RibbonTab = 'home' | 'view' | 'tools';

export const Ribbon = memo(function Ribbon({ onPreferences, onFeedback }: { onPreferences: () => void; onFeedback: () => void }) {
  const [activeTab, setActiveTab] = useState<RibbonTab>('home');
  const [backstageOpen, setBackstageOpen] = useState(false);
  const renderMode = useAppStore((s) => s.renderMode);
  const setRenderMode = useAppStore((s) => s.setRenderMode);
  const showGrid = useAppStore((s) => s.showGrid);
  const setShowGrid = useAppStore((s) => s.setShowGrid);
  const ambientLightEnabled = useAppStore((s) => s.ambientLightEnabled);
  const setAmbientLightEnabled = useAppStore((s) => s.setAmbientLightEnabled);
  const directionalLightEnabled = useAppStore((s) => s.directionalLightEnabled);
  const setDirectionalLightEnabled = useAppStore((s) => s.setDirectionalLightEnabled);
  const cameraMode = useAppStore((s) => s.cameraMode);
  const setCameraMode = useAppStore((s) => s.setCameraMode);
  const walkSpeed = useAppStore((s) => s.walkSpeed);
  const setWalkSpeed = useAppStore((s) => s.setWalkSpeed);
  const projectionMode = useAppStore((s) => s.projectionMode);
  const setProjectionMode = useAppStore((s) => s.setProjectionMode);
  const sectionBoxEnabled = useAppStore((s) => s.sectionBoxEnabled);
  const setSectionBoxEnabled = useAppStore((s) => s.setSectionBoxEnabled);
  const selectedObject = useAppStore((s) => s.selectedObject);
  const uiTheme = useAppStore((s) => s.uiTheme);
  const setUITheme = useAppStore((s) => s.setUITheme);

  const actions = useViewerActions();
  const { t } = useTranslation(['ribbon', 'backstage']);

  // OpenAEC shortcuts: Ctrl+O open, Ctrl+, preferences
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey) return;
      if (e.key === 'o' || e.key === 'O') {
        e.preventDefault();
        setBackstageOpen(false);
        void actions.handleOpen();
      } else if (e.key === ',') {
        e.preventDefault();
        setBackstageOpen(false);
        onPreferences();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [actions.handleOpen, onPreferences]);

  return (
    <>
    {/* Hidden file input for browser mode */}
    <input
      ref={actions.fileInputRef}
      type="file"
      accept={actions.FILE_INPUT_ACCEPT}
      multiple
      style={{ display: 'none' }}
      onChange={actions.handleFileInputChange}
    />
    {backstageOpen && (
      <Backstage
        onClose={() => setBackstageOpen(false)}
        onOpen={() => void actions.handleOpen()}
        onPreferences={onPreferences}
        onFeedback={onFeedback}
      />
    )}
    <div className="ribbon-container">
      {/* Tab bar */}
      <div className="ribbon-tabs">
        <button className="ribbon-tab file" onClick={() => setBackstageOpen(true)}>
          {t('backstage:file')}
        </button>
        <button
          className={`ribbon-tab ${activeTab === 'home' ? 'active' : ''}`}
          onClick={() => setActiveTab('home')}
        >
          {t('ribbon:tabs.home')}
        </button>
        <button
          className={`ribbon-tab ${activeTab === 'view' ? 'active' : ''}`}
          onClick={() => setActiveTab('view')}
        >
          {t('ribbon:tabs.view')}
        </button>
        <button
          className={`ribbon-tab ${activeTab === 'tools' ? 'active' : ''}`}
          onClick={() => setActiveTab('tools')}
        >
          {t('ribbon:tabs.tools')}
        </button>
      </div>

      {/* Content area */}
      <div className="ribbon-content-container">
        {/* Home tab */}
        <div className={`ribbon-content ${activeTab === 'home' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label={t('ribbon:groups.file')}>
              <RibbonButton
                icon={<Download size={20} />}
                label={t('ribbon:buttons.import')}
                onClick={() => void actions.handleImport()}
                tooltip={t('ribbon:buttons.importTooltip')}
              />
            </RibbonGroup>

            <RibbonGroup label={t('ribbon:groups.renderMode')}>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<SolidIcon />} label={t('ribbon:buttons.solid')} onClick={() => setRenderMode('solid')} active={renderMode === 'solid'} />
                <RibbonSmallButton icon={<WireframeIcon />} label={t('ribbon:buttons.wireframe')} onClick={() => setRenderMode('wireframe')} active={renderMode === 'wireframe'} />
                <RibbonSmallButton icon={<PointsIcon />} label={t('ribbon:buttons.points')} onClick={() => setRenderMode('points')} active={renderMode === 'points'} />
                <RibbonSmallButton icon={<HiddenLineIcon />} label={t('ribbon:buttons.hiddenLine')} onClick={() => setRenderMode('hiddenline')} active={renderMode === 'hiddenline'} />
              </RibbonButtonStack>
            </RibbonGroup>

            <RibbonGroup label={t('ribbon:groups.display')}>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Grid3x3 size={14} />} label={t('ribbon:buttons.grid')} onClick={() => setShowGrid(!showGrid)} active={showGrid} />
                <RibbonSmallButton icon={<Lightbulb size={14} />} label={t('ribbon:buttons.ambient')} onClick={() => setAmbientLightEnabled(!ambientLightEnabled)} active={ambientLightEnabled} />
                <RibbonSmallButton icon={<Sun size={14} />} label={t('ribbon:buttons.directional')} onClick={() => setDirectionalLightEnabled(!directionalLightEnabled)} active={directionalLightEnabled} />
              </RibbonButtonStack>
            </RibbonGroup>

            <RibbonGroup label={t('ribbon:groups.settings')}>
              <ThemeSelector currentTheme={uiTheme} onThemeChange={setUITheme} />
            </RibbonGroup>
          </div>
        </div>

        {/* View tab */}
        <div className={`ribbon-content ${activeTab === 'view' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label={t('ribbon:groups.navigation')}>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<OrbitIcon />} label={t('ribbon:buttons.orbit')} onClick={() => setCameraMode('orbit')} active={cameraMode === 'orbit'} />
                <RibbonSmallButton icon={<WalkIcon />} label={t('ribbon:buttons.walk')} onClick={() => setCameraMode('walk')} active={cameraMode === 'walk'} />
              </RibbonButtonStack>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<PerspectiveIcon />} label={t('ribbon:buttons.perspective')} onClick={() => setProjectionMode('perspective')} active={projectionMode === 'perspective'} />
                <RibbonSmallButton icon={<OrthoIcon />} label={t('ribbon:buttons.ortho')} onClick={() => setProjectionMode('orthographic')} active={projectionMode === 'orthographic'} />
              </RibbonButtonStack>
              {cameraMode === 'walk' && (
                <div className="ribbon-slider-group">
                  <label className="ribbon-slider-row">
                    <span className="ribbon-slider-label">{t('ribbon:buttons.speed')}</span>
                    <input type="range" min="0.5" max="50" step="0.5" value={walkSpeed} onChange={(e) => setWalkSpeed(Number(e.target.value))} className="ribbon-slider" />
                    <span className="ribbon-slider-value">{walkSpeed}</span>
                  </label>
                </div>
              )}
            </RibbonGroup>

            <RibbonGroup label={t('ribbon:groups.sectionBox')}>
              <RibbonButton
                icon={<SectionBoxIcon />}
                label={t('ribbon:buttons.section')}
                onClick={() => setSectionBoxEnabled(!sectionBoxEnabled)}
                active={sectionBoxEnabled}
                tooltip={t('ribbon:buttons.sectionTooltip')}
              />
              <RibbonButton
                icon={<RotateCcw size={20} />}
                label={t('ribbon:buttons.reset')}
                onClick={() => {
                  window.dispatchEvent(new CustomEvent('open3d:sectionbox-reset'));
                }}
                disabled={!sectionBoxEnabled}
                tooltip={t('ribbon:buttons.resetSectionTooltip')}
              />
              <RibbonButton
                icon={<Crosshair size={20} />}
                label={t('ribbon:buttons.fitSelection')}
                onClick={() => {
                  window.dispatchEvent(new CustomEvent('open3d:sectionbox-fit-selection'));
                }}
                disabled={!selectedObject}
                tooltip={t('ribbon:buttons.fitSelectionTooltip')}
              />
            </RibbonGroup>

            <RibbonGroup label={t('ribbon:groups.camera')}>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Camera size={14} />} label={t('ribbon:buttons.front')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'front' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label={t('ribbon:buttons.back')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'back' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label={t('ribbon:buttons.left')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'left' }))} />
              </RibbonButtonStack>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Camera size={14} />} label={t('ribbon:buttons.right')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'right' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label={t('ribbon:buttons.top')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'top' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label={t('ribbon:buttons.bottom')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'bottom' }))} />
              </RibbonButtonStack>
            </RibbonGroup>
          </div>
        </div>

        {/* Tools tab */}
        <div className={`ribbon-content ${activeTab === 'tools' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label={t('ribbon:groups.measure')}>
              <RibbonButton
                icon={<MoveVertical size={20} />}
                label={t('ribbon:buttons.distance')}
                onClick={() => {}}
                disabled
                tooltip={t('ribbon:buttons.distanceTooltip')}
              />
              <RibbonButton
                icon={<Gauge size={20} />}
                label={t('ribbon:buttons.angle')}
                onClick={() => {}}
                disabled
                tooltip={t('ribbon:buttons.angleTooltip')}
              />
            </RibbonGroup>

            <RibbonGroup label={t('ribbon:groups.info')}>
              <RibbonButton
                icon={<Camera size={20} />}
                label={t('ribbon:buttons.statistics')}
                onClick={() => window.dispatchEvent(new CustomEvent('open3d:show-stats'))}
                tooltip={t('ribbon:buttons.statisticsTooltip')}
              />
            </RibbonGroup>
          </div>
        </div>
      </div>
    </div>
    </>
  );
});
