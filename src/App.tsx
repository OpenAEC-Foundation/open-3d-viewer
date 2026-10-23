import { useEffect, useRef, useState } from 'react';
import { PanelRightOpen, PanelRightClose, PanelLeftOpen, PanelLeftClose } from 'lucide-react';
import { MenuBar } from './components/layout/MenuBar/MenuBar';
import { Ribbon } from './components/layout/Ribbon/Ribbon';
import { StatusBar } from './components/layout/StatusBar/StatusBar';
import { Viewer3D } from './components/canvas/Viewer3D';
import { PropertiesPanel } from './components/panels/PropertiesPanel';
import { LayerManager } from './components/panels/LayerManager';
import { SettingsDialog } from './components/SettingsDialog/SettingsDialog';
import { FeedbackDialog } from './components/FeedbackDialog/FeedbackDialog';
import { useAppStore } from './state/appStore';
import { useTranslation } from './i18n/useTranslation';

function App() {
  // Apply theme on mount
  const uiTheme = useAppStore((s) => s.uiTheme);
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', uiTheme);
  }, [uiTheme]);

  const rightPanelOpen = useAppStore((s) => s.rightPanelOpen);
  const toggleRightPanel = useAppStore((s) => s.toggleRightPanel);
  const [showSettings, setShowSettings] = useState(false);
  const [showFeedback, setShowFeedback] = useState(false);
  const [leftPanelOpen, setLeftPanelOpen] = useState(true);
  const { t } = useTranslation('panels');

  // Right panel resizing
  const [rightPanelWidth, setRightPanelWidth] = useState(256);
  const isResizingRight = useRef(false);

  // Left panel resizing
  const [leftPanelWidth, setLeftPanelWidth] = useState(240);
  const isResizingLeft = useRef(false);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (isResizingRight.current) {
        const newWidth = Math.max(180, Math.min(500, window.innerWidth - e.clientX));
        setRightPanelWidth(newWidth);
      }
      if (isResizingLeft.current) {
        const newWidth = Math.max(180, Math.min(400, e.clientX));
        setLeftPanelWidth(newWidth);
      }
    };
    const handleMouseUp = () => {
      if (isResizingRight.current || isResizingLeft.current) {
        isResizingRight.current = false;
        isResizingLeft.current = false;
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
      }
    };
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, []);

  // Block browser shortcuts in production
  useEffect(() => {
    if (import.meta.env.DEV) return;

    const handler = (e: KeyboardEvent) => {
      if (e.key === 'F5' || (e.ctrlKey && e.key === 'r')) e.preventDefault();
      if (e.ctrlKey && e.shiftKey && e.key === 'I') e.preventDefault();
      if (e.ctrlKey && e.key === 'u') e.preventDefault();
      if (e.key === 'F7') e.preventDefault();
    };

    const contextHandler = (e: MouseEvent) => e.preventDefault();

    window.addEventListener('keydown', handler, true);
    window.addEventListener('contextmenu', contextHandler, true);
    return () => {
      window.removeEventListener('keydown', handler, true);
      window.removeEventListener('contextmenu', contextHandler, true);
    };
  }, []);

  return (
    <div className="flex flex-col h-full w-full bg-cad-bg text-cad-text no-select">
      {/* Menu Bar */}
      <MenuBar onSettingsClick={() => setShowSettings(true)} onFeedbackClick={() => setShowFeedback(true)} />

      {/* Ribbon */}
      <Ribbon onPreferences={() => setShowSettings(true)} onFeedback={() => setShowFeedback(true)} />

      {/* Main Content Area */}
      <div className="flex flex-1 overflow-hidden">
        {/* Left Panel - Layers */}
        {leftPanelOpen ? (
          <div
            className="bg-cad-surface border-r border-cad-border flex flex-col overflow-hidden relative"
            style={{ width: leftPanelWidth, minWidth: 180, maxWidth: 400 }}
          >
            {/* Panel header */}
            <div className="flex items-center justify-between px-3 h-7 min-h-[28px] select-none border-b border-cad-border bg-cad-surface">
              <span className="text-xs font-semibold text-cad-text">{t('layers.title')}</span>
              <button
                type="button"
                onClick={() => setLeftPanelOpen(false)}
                className="flex items-center justify-center w-5 h-5 rounded hover:bg-cad-hover text-cad-text-dim hover:text-cad-text transition-colors"
                title={t('layers.collapseLeft')}
              >
                <PanelLeftClose size={14} />
              </button>
            </div>
            <div className="flex-1 overflow-y-auto">
              <LayerManager />
            </div>
            {/* Resize handle */}
            <div
              className="absolute top-0 right-0 w-px h-full cursor-col-resize hover:bg-cad-accent z-10"
              onMouseDown={(e) => {
                e.preventDefault();
                isResizingLeft.current = true;
                document.body.style.cursor = 'col-resize';
                document.body.style.userSelect = 'none';
              }}
            />
          </div>
        ) : (
          <div className="flex flex-col bg-cad-surface border-r border-cad-border" style={{ width: 28 }}>
            <button
              type="button"
              onClick={() => setLeftPanelOpen(true)}
              className="flex items-center justify-center w-full h-7 hover:bg-cad-hover text-cad-text-dim hover:text-cad-text transition-colors"
              title={t('layers.expandLayers')}
            >
              <PanelLeftOpen size={16} />
            </button>
          </div>
        )}

        {/* Center - 3D Viewer */}
        <div className="flex-1 flex flex-col overflow-hidden relative">
          <div className="flex-1 relative overflow-hidden">
            <Viewer3D />
          </div>
        </div>

        {/* Right Panel - Properties */}
        {rightPanelOpen ? (
          <div
            className="bg-cad-surface border-l border-cad-border flex flex-col overflow-hidden relative"
            style={{ width: rightPanelWidth, minWidth: 180, maxWidth: 500 }}
          >
            {/* Resize handle */}
            <div
              className="absolute top-0 left-0 w-px h-full cursor-col-resize hover:bg-cad-accent z-10"
              onMouseDown={(e) => {
                e.preventDefault();
                isResizingRight.current = true;
                document.body.style.cursor = 'col-resize';
                document.body.style.userSelect = 'none';
              }}
            />
            {/* Panel header */}
            <div className="flex items-center justify-between px-3 h-7 min-h-[28px] select-none border-b border-cad-border bg-cad-surface">
              <span className="text-xs font-semibold text-cad-text">{t('properties.title')}</span>
              <button
                type="button"
                onClick={toggleRightPanel}
                className="flex items-center justify-center w-5 h-5 rounded hover:bg-cad-hover text-cad-text-dim hover:text-cad-text transition-colors"
                title={t('properties.collapseRight')}
              >
                <PanelRightClose size={14} />
              </button>
            </div>
            <div className="flex-1 overflow-y-auto">
              <PropertiesPanel />
            </div>
          </div>
        ) : (
          <div className="flex flex-col bg-cad-surface border-l border-cad-border" style={{ width: 28 }}>
            <button
              type="button"
              onClick={toggleRightPanel}
              className="flex items-center justify-center w-full h-7 hover:bg-cad-hover text-cad-text-dim hover:text-cad-text transition-colors"
              title={t('properties.expandRight')}
            >
              <PanelRightOpen size={16} />
            </button>
          </div>
        )}
      </div>

      {/* Bottom Status Bar */}
      <StatusBar />

      {/* Settings Dialog */}
      {showSettings && (
        <SettingsDialog onClose={() => setShowSettings(false)} />
      )}

      {/* Feedback Dialog */}
      {showFeedback && (
        <FeedbackDialog onClose={() => setShowFeedback(false)} />
      )}
    </div>
  );
}

export default App;
