import { useState, memo } from 'react';
import {
  Download, Sun, Grid3x3, Lightbulb, RotateCcw,
  Camera, MoveVertical, Gauge,
} from 'lucide-react';
import { useAppStore } from '../../../state/appStore';
import { RibbonButton, RibbonSmallButton, RibbonGroup, RibbonButtonStack, ThemeSelector } from './RibbonComponents';
import { WireframeIcon, SolidIcon, PointsIcon, HiddenLineIcon, SectionBoxIcon, OrbitIcon, WalkIcon, PerspectiveIcon, OrthoIcon } from './RibbonIcons';
import { useViewerActions } from './useViewerActions';
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
          Home
        </button>
        <button
          className={`ribbon-tab ${activeTab === 'view' ? 'active' : ''}`}
          onClick={() => setActiveTab('view')}
        >
          View
        </button>
        <button
          className={`ribbon-tab ${activeTab === 'tools' ? 'active' : ''}`}
          onClick={() => setActiveTab('tools')}
        >
          Tools
        </button>
      </div>

      {/* Content area */}
      <div className="ribbon-content-container">
        {/* Home tab */}
        <div className={`ribbon-content ${activeTab === 'home' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label="File">
              <RibbonButton
                icon={<Download size={20} />}
                label="Import"
                onClick={actions.handleImport}
                tooltip="Import 3D file"
              />
            </RibbonGroup>

            <RibbonGroup label="Render Mode">
              <RibbonButtonStack>
                <RibbonSmallButton icon={<SolidIcon />} label="Solid" onClick={() => setRenderMode('solid')} active={renderMode === 'solid'} />
                <RibbonSmallButton icon={<WireframeIcon />} label="Wireframe" onClick={() => setRenderMode('wireframe')} active={renderMode === 'wireframe'} />
                <RibbonSmallButton icon={<PointsIcon />} label="Points" onClick={() => setRenderMode('points')} active={renderMode === 'points'} />
                <RibbonSmallButton icon={<HiddenLineIcon />} label="Hidden Line" onClick={() => setRenderMode('hiddenline')} active={renderMode === 'hiddenline'} />
              </RibbonButtonStack>
            </RibbonGroup>

            <RibbonGroup label="Display">
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Grid3x3 size={14} />} label="Grid" onClick={() => setShowGrid(!showGrid)} active={showGrid} />
                <RibbonSmallButton icon={<Lightbulb size={14} />} label="Ambient" onClick={() => setAmbientLightEnabled(!ambientLightEnabled)} active={ambientLightEnabled} />
                <RibbonSmallButton icon={<Sun size={14} />} label="Directional" onClick={() => setDirectionalLightEnabled(!directionalLightEnabled)} active={directionalLightEnabled} />
              </RibbonButtonStack>
            </RibbonGroup>

            <RibbonGroup label="Settings">
              <ThemeSelector currentTheme={uiTheme} onThemeChange={setUITheme} />
            </RibbonGroup>
          </div>
        </div>

        {/* View tab */}
        <div className={`ribbon-content ${activeTab === 'view' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label="Navigation">
              <RibbonButtonStack>
                <RibbonSmallButton icon={<OrbitIcon />} label="Orbit" onClick={() => setCameraMode('orbit')} active={cameraMode === 'orbit'} />
                <RibbonSmallButton icon={<WalkIcon />} label="Walk" onClick={() => setCameraMode('walk')} active={cameraMode === 'walk'} />
              </RibbonButtonStack>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<PerspectiveIcon />} label="Perspective" onClick={() => setProjectionMode('perspective')} active={projectionMode === 'perspective'} />
                <RibbonSmallButton icon={<OrthoIcon />} label="Ortho" onClick={() => setProjectionMode('orthographic')} active={projectionMode === 'orthographic'} />
              </RibbonButtonStack>
              {cameraMode === 'walk' && (
                <div className="ribbon-slider-group">
                  <label className="ribbon-slider-row">
                    <span className="ribbon-slider-label">Speed</span>
                    <input type="range" min="0.5" max="50" step="0.5" value={walkSpeed} onChange={(e) => setWalkSpeed(Number(e.target.value))} className="ribbon-slider" />
                    <span className="ribbon-slider-value">{walkSpeed}</span>
                  </label>
                </div>
              )}
            </RibbonGroup>

            <RibbonGroup label="Section Box">
              <RibbonButton
                icon={<SectionBoxIcon />}
                label="Section"
                onClick={() => setSectionBoxEnabled(!sectionBoxEnabled)}
                active={sectionBoxEnabled}
                tooltip="Toggle section box"
              />
              <RibbonButton
                icon={<RotateCcw size={20} />}
                label="Reset"
                onClick={() => {
                  window.dispatchEvent(new CustomEvent('open3d:sectionbox-reset'));
                }}
                disabled={!sectionBoxEnabled}
                tooltip="Reset section box to model bounds"
              />
            </RibbonGroup>

            <RibbonGroup label="Camera">
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Camera size={14} />} label="Front" onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'front' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label="Back" onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'back' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label="Left" onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'left' }))} />
              </RibbonButtonStack>
              <RibbonButtonStack>
                <RibbonSmallButton icon={<Camera size={14} />} label="Right" onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'right' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label="Top" onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'top' }))} />
                <RibbonSmallButton icon={<Camera size={14} />} label="Bottom" onClick={() => window.dispatchEvent(new CustomEvent('open3d:camera', { detail: 'bottom' }))} />
              </RibbonButtonStack>
            </RibbonGroup>
          </div>
        </div>

        {/* Tools tab */}
        <div className={`ribbon-content ${activeTab === 'tools' ? 'active' : ''}`}>
          <div className="ribbon-groups">
            <RibbonGroup label="Measure">
              <RibbonButton
                icon={<MoveVertical size={20} />}
                label="Distance"
                onClick={() => {}}
                disabled
                tooltip="Measure distance (coming soon)"
              />
              <RibbonButton
                icon={<Gauge size={20} />}
                label="Angle"
                onClick={() => {}}
                disabled
                tooltip="Measure angle (coming soon)"
              />
            </RibbonGroup>

            <RibbonGroup label="Info">
              <RibbonButton
                icon={<Camera size={20} />}
                label="Statistics"
                onClick={() => window.dispatchEvent(new CustomEvent('open3d:show-stats'))}
                tooltip="Show model statistics"
              />
            </RibbonGroup>
          </div>
        </div>
      </div>
    </div>
    </>
  );
});
