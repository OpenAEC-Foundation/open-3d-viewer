import { memo } from 'react';
import {
  Download, Sun, Grid3x3, Lightbulb, RotateCcw,
  MoveVertical, Gauge, Home,
} from 'lucide-react';
import { useAppStore } from '../../../state/appStore';
import { RibbonButton, RibbonSmallButton, RibbonGroup, RibbonButtonStack, ThemeSelector } from './RibbonComponents';
import { WireframeIcon, SolidIcon, PointsIcon, HiddenLineIcon, SectionBoxIcon, OrbitIcon, WalkIcon, PerspectiveIcon, OrthoIcon } from './RibbonIcons';
import { useViewerActions } from './useViewerActions';
import './Ribbon.css';

export const Ribbon = memo(function Ribbon() {
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
  const sectionCapsEnabled = useAppStore((s) => s.sectionCapsEnabled);
  const setSectionCapsEnabled = useAppStore((s) => s.setSectionCapsEnabled);
  const uiTheme = useAppStore((s) => s.uiTheme);
  const setUITheme = useAppStore((s) => s.setUITheme);
  const clearModels = useAppStore((s) => s.clearModels);
  const measureMode = useAppStore((s) => s.measureMode);
  const setMeasureMode = useAppStore((s) => s.setMeasureMode);
  const clearMeasurements = useAppStore((s) => s.clearMeasurements);

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
      {/* Tab label */}
      <div className="ribbon-tabs">
        <button className="ribbon-tab active">Home</button>
      </div>

      <div className="ribbon-content-container">
        <div className="ribbon-content active">
          <div className="ribbon-groups">
            <RibbonGroup label="File">
              <RibbonButton
                icon={<Home size={20} />}
                label="Start"
                onClick={() => clearModels()}
                tooltip="Terug naar startscherm"
              />
              <RibbonButton
                icon={<Download size={20} />}
                label="Import"
                onClick={actions.handleImport}
                tooltip="Import 3D file"
              />
            </RibbonGroup>

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
              <RibbonButton
                icon={<SectionBoxIcon />}
                label="Caps"
                onClick={() => setSectionCapsEnabled(!sectionCapsEnabled)}
                active={sectionCapsEnabled}
                disabled={!sectionBoxEnabled}
                tooltip="Toon zwarte doorsnede-vulling"
              />
            </RibbonGroup>

            <RibbonGroup label="Measure">
              <RibbonButton
                icon={<MoveVertical size={20} />}
                label="Point"
                onClick={() => setMeasureMode(measureMode === 'point' ? 'off' : 'point')}
                active={measureMode === 'point'}
                tooltip="Punt-tot-punt afstand meten"
              />
              <RibbonButton
                icon={<Gauge size={20} />}
                label="Face"
                onClick={() => setMeasureMode(measureMode === 'face' ? 'off' : 'face')}
                active={measureMode === 'face'}
                tooltip="Vlak-tot-vlak afstand meten"
              />
              <RibbonButton
                icon={<RotateCcw size={20} />}
                label="Wissen"
                onClick={() => {
                  clearMeasurements();
                  window.dispatchEvent(new CustomEvent('open3d:clear-measurements'));
                }}
                tooltip="Alle metingen wissen"
              />
            </RibbonGroup>

            <RibbonGroup label="Settings">
              <ThemeSelector currentTheme={uiTheme} onThemeChange={setUITheme} />
            </RibbonGroup>
          </div>
        </div>
      </div>
    </div>
    </>
  );
});
