import { useState, memo } from 'react';
import {
  Download, Sun, Grid3x3, Lightbulb, RotateCcw,
  Camera, MoveVertical, Gauge,
} from 'lucide-react';
import { useAppStore } from '../../../state/appStore';
import { RibbonButton, RibbonSmallButton, RibbonGroup, RibbonButtonStack, ThemeSelector } from './RibbonComponents';
import { WireframeIcon, SolidIcon, PointsIcon, HiddenLineIcon, SectionBoxIcon, OrbitIcon, WalkIcon, PerspectiveIcon, OrthoIcon } from './RibbonIcons';
import { useViewerActions } from './useViewerActions';
import { useTranslation } from '../../../i18n/useTranslation';
import './Ribbon.css';

type RibbonTab = 'home' | 'view' | 'tools';

export const Ribbon = memo(function Ribbon() {
  const [activeTab, setActiveTab] = useState<RibbonTab>('home');
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
  const uiTheme = useAppStore((s) => s.uiTheme);
  const setUITheme = useAppStore((s) => s.setUITheme);

  const actions = useViewerActions();
  const { t } = useTranslation('ribbon');

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
    <div className="ribbon-container">
      {/* Tab bar */}
      <div className="ribbon-tabs">
        <button
          className={`ribbon-tab ${activeTab === 'home' ? 'active' : ''}`}
          onClick={() => setActiveTab('home')}
        >
          {t('tabs.home')}
        </button>
        <button
          className={`ribbon-tab ${activeTab === 'view' ? 'active' : ''}`}
          onClick={() => setActiveTab('view')}
        >
          {t('tabs.view')}
        </button>
        <button
          className={`ribbon-tab ${activeTab === 'tools' ? 'active' : ''}`}
          onClick={() => setActiveTab('tools')}
        >
          {t('tabs.tools')}
        </button>
      </div>

      {/* Content area */}
      <div className="ribbon-content-container">
        {/* Home tab */}
        <div className={`ribbon-content ${activeTab === 'home' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label={t('groups.file')}>
              <RibbonButton
                icon={<Download size={20} />}
                label={t('buttons.import')}
                onClick={actions.handleImport}
                tooltip={t('buttons.importTooltip')}
              />
            </RibbonGroup>

            <RibbonGroup label={t('groups.renderMode')}>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<SolidIcon />} label={t('buttons.solid')} onClick={() => setRenderMode('solid')} active={renderMode === 'solid'} />
                <RibbonSmallButton icon={<WireframeIcon />} label={t('buttons.wireframe')} onClick={() => setRenderMode('wireframe')} active={renderMode === 'wireframe'} />
                <RibbonSmallButton icon={<PointsIcon />} label={t('buttons.points')} onClick={() => setRenderMode('points')} active={renderMode === 'points'} />
                <RibbonSmallButton icon={<HiddenLineIcon />} label={t('buttons.hiddenLine')} onClick={() => setRenderMode('hiddenline')} active={renderMode === 'hiddenline'} />
              </RibbonButtonStack>
            </RibbonGroup>

            <RibbonGroup label={t('groups.display')}>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Grid3x3 size={14} />} label={t('buttons.grid')} onClick={() => setShowGrid(!showGrid)} active={showGrid} />
                <RibbonSmallButton icon={<Lightbulb size={14} />} label={t('buttons.ambient')} onClick={() => setAmbientLightEnabled(!ambientLightEnabled)} active={ambientLightEnabled} />
                <RibbonSmallButton icon={<Sun size={14} />} label={t('buttons.directional')} onClick={() => setDirectionalLightEnabled(!directionalLightEnabled)} active={directionalLightEnabled} />
              </RibbonButtonStack>
            </RibbonGroup>

            <RibbonGroup label={t('groups.settings')}>
              <ThemeSelector currentTheme={uiTheme} onThemeChange={setUITheme} />
            </RibbonGroup>
          </div>
        </div>

        {/* View tab */}
        <div className={`ribbon-content ${activeTab === 'view' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label={t('groups.navigation')}>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<OrbitIcon />} label={t('buttons.orbit')} onClick={() => setCameraMode('orbit')} active={cameraMode === 'orbit'} />
                <RibbonSmallButton icon={<WalkIcon />} label={t('buttons.walk')} onClick={() => setCameraMode('walk')} active={cameraMode === 'walk'} />
              </RibbonButtonStack>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<PerspectiveIcon />} label={t('buttons.perspective')} onClick={() => setProjectionMode('perspective')} active={projectionMode === 'perspective'} />
                <RibbonSmallButton icon={<OrthoIcon />} label={t('buttons.ortho')} onClick={() => setProjectionMode('orthographic')} active={projectionMode === 'orthographic'} />
              </RibbonButtonStack>
              {cameraMode === 'walk' && (
                <div className="ribbon-slider-group">
                  <label className="ribbon-slider-row">
                    <span className="ribbon-slider-label">{t('buttons.speed')}</span>
                    <input type="range" min="0.5" max="50" step="0.5" value={walkSpeed} onChange={(e) => setWalkSpeed(Number(e.target.value))} className="ribbon-slider" />
                    <span className="ribbon-slider-value">{walkSpeed}</span>
                  </label>
                </div>
              )}
            </RibbonGroup>

            <RibbonGroup label={t('groups.sectionBox')}>
              <RibbonButton
                icon={<SectionBoxIcon />}
                label={t('buttons.section')}
                onClick={() => setSectionBoxEnabled(!sectionBoxEnabled)}
                active={sectionBoxEnabled}
                tooltip={t('buttons.sectionTooltip')}
              />
              <RibbonButton
                icon={<RotateCcw size={20} />}
                label={t('buttons.reset')}
                onClick={() => {
                  window.dispatchEvent(new CustomEvent('open3d:sectionbox-reset'));
                }}
                disabled={!sectionBoxEnabled}
                tooltip={t('buttons.resetSectionTooltip')}
              />
            </RibbonGroup>

            <RibbonGroup label={t('groups.camera')}>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Camera size={14} />} label={t('buttons.front')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'front' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label={t('buttons.back')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'back' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label={t('buttons.left')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'left' }))} />
              </RibbonButtonStack>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Camera size={14} />} label={t('buttons.right')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'right' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label={t('buttons.top')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'top' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label={t('buttons.bottom')} onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'bottom' }))} />
              </RibbonButtonStack>
            </RibbonGroup>
          </div>
        </div>

        {/* Tools tab */}
        <div className={`ribbon-content ${activeTab === 'tools' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label={t('groups.measure')}>
              <RibbonButton
                icon={<MoveVertical size={20} />}
                label={t('buttons.distance')}
                onClick={() => {}}
                disabled
                tooltip={t('buttons.distanceTooltip')}
              />
              <RibbonButton
                icon={<Gauge size={20} />}
                label={t('buttons.angle')}
                onClick={() => {}}
                disabled
                tooltip={t('buttons.angleTooltip')}
              />
            </RibbonGroup>

            <RibbonGroup label={t('groups.info')}>
              <RibbonButton
                icon={<Camera size={20} />}
                label={t('buttons.statistics')}
                onClick={() => window.dispatchEvent(new CustomEvent('open3d:show-stats'))}
                tooltip={t('buttons.statisticsTooltip')}
              />
            </RibbonGroup>
          </div>
        </div>
      </div>
    </div>
    </>
  );
});
