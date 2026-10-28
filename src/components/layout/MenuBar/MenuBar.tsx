import { useState, useEffect, memo, useCallback } from 'react';
import { Settings } from 'lucide-react';
import { useTranslation } from '../../../i18n/useTranslation';
import '../../FeedbackDialog/FeedbackDialog.css';

type Platform = 'windows' | 'linux' | 'macos';

function getPlatform(): Platform {
  const userAgent = navigator.userAgent.toLowerCase();
  if (userAgent.includes('win')) return 'windows';
  if (userAgent.includes('mac')) return 'macos';
  return 'linux';
}

const isTauri = !!(window as any).__TAURI_INTERNALS__;

/** Lazy-load Tauri window API only when running in Tauri */
async function getTauriWindow() {
  if (!isTauri) return null;
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    return getCurrentWindow();
  } catch {
    return null;
  }
}

function WindowsControls({
  onMinimize,
  onMaximize,
  onClose,
  isMaximized
}: {
  onMinimize: () => void;
  onMaximize: () => void;
  onClose: () => void;
  isMaximized: boolean;
}) {
  const { t } = useTranslation('menuBar');
  return (
    <div className="flex items-center h-full">
      <button
        onClick={onMinimize}
        className="w-[46px] h-full flex items-center justify-center hover:bg-[var(--theme-hover-strong)] transition-colors cursor-default"
        title={t('minimize')}
      >
        <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
          <rect width="10" height="1" />
        </svg>
      </button>
      <button
        onClick={onMaximize}
        className="w-[46px] h-full flex items-center justify-center hover:bg-[var(--theme-hover-strong)] transition-colors cursor-default"
        title={isMaximized ? t('restoreDown') : t('maximize')}
      >
        {isMaximized ? (
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
            <rect x="2" y="0.5" width="7" height="7" />
            <polyline points="0.5,2.5 0.5,9.5 7.5,9.5 7.5,7.5" />
          </svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
            <rect x="0.5" y="0.5" width="9" height="9" />
          </svg>
        )}
      </button>
      <button
        onClick={onClose}
        className="w-[46px] h-full flex items-center justify-center hover:bg-[#e81123] transition-colors group cursor-default"
        title={t('close')}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor" strokeWidth="1.2" className="group-hover:stroke-white">
          <line x1="0" y1="0" x2="10" y2="10" />
          <line x1="10" y1="0" x2="0" y2="10" />
        </svg>
      </button>
    </div>
  );
}

function LinuxControls({
  onMinimize,
  onMaximize,
  onClose,
  isMaximized
}: {
  onMinimize: () => void;
  onMaximize: () => void;
  onClose: () => void;
  isMaximized: boolean;
}) {
  const { t } = useTranslation('menuBar');
  return (
    <div className="flex items-center h-full gap-2 px-3">
      <button
        onClick={onMinimize}
        className="w-3 h-3 rounded-full bg-[#f5c211] hover:bg-[#d9a900] transition-colors flex items-center justify-center group cursor-default"
        title={t('minimize')}
      >
        <svg width="6" height="1" viewBox="0 0 6 1" className="opacity-0 group-hover:opacity-100 transition-opacity">
          <rect width="6" height="1" fill="#000" fillOpacity="0.6" />
        </svg>
      </button>
      <button
        onClick={onMaximize}
        className="w-3 h-3 rounded-full bg-[#2ecc71] hover:bg-[#27ae60] transition-colors flex items-center justify-center group cursor-default"
        title={isMaximized ? t('restore') : t('maximize')}
      >
        {isMaximized ? (
          <svg width="6" height="6" viewBox="0 0 6 6" className="opacity-0 group-hover:opacity-100 transition-opacity">
            <rect x="0.5" y="2" width="3" height="3" fill="none" stroke="#000" strokeOpacity="0.6" strokeWidth="1" />
            <polyline points="2,2 2,0.5 5.5,0.5 5.5,4 4,4" fill="none" stroke="#000" strokeOpacity="0.6" strokeWidth="1" />
          </svg>
        ) : (
          <svg width="6" height="6" viewBox="0 0 6 6" className="opacity-0 group-hover:opacity-100 transition-opacity">
            <rect x="0.5" y="0.5" width="5" height="5" fill="none" stroke="#000" strokeOpacity="0.6" strokeWidth="1" />
          </svg>
        )}
      </button>
      <button
        onClick={onClose}
        className="w-3 h-3 rounded-full bg-[#e95420] hover:bg-[#c44117] transition-colors flex items-center justify-center group cursor-default"
        title={t('close')}
      >
        <svg width="6" height="6" viewBox="0 0 6 6" className="opacity-0 group-hover:opacity-100 transition-opacity">
          <line x1="1" y1="1" x2="5" y2="5" stroke="#000" strokeOpacity="0.6" strokeWidth="1" />
          <line x1="5" y1="1" x2="1" y2="5" stroke="#000" strokeOpacity="0.6" strokeWidth="1" />
        </svg>
      </button>
    </div>
  );
}

function WindowControls() {
  const [isMaximized, setIsMaximized] = useState(false);
  const [platform] = useState<Platform>(getPlatform);
  const [appWindow, setAppWindow] = useState<Awaited<ReturnType<typeof getTauriWindow>>>(null);

  useEffect(() => {
    getTauriWindow().then(setAppWindow);
  }, []);

  useEffect(() => {
    if (!appWindow) return;

    let cancelled = false;
    appWindow.isMaximized().then((m) => { if (!cancelled) setIsMaximized(m); });

    const unlisten = appWindow.onResized(() => {
      appWindow.isMaximized().then((m) => { if (!cancelled) setIsMaximized(m); });
    });

    return () => {
      cancelled = true;
      unlisten.then((fn) => fn());
    };
  }, [appWindow]);

  if (!appWindow) return null;

  const handleMinimize = () => appWindow.minimize();
  const handleMaximize = () => appWindow.toggleMaximize();
  const handleClose = () => appWindow.close();

  const controlProps = {
    onMinimize: handleMinimize,
    onMaximize: handleMaximize,
    onClose: handleClose,
    isMaximized,
  };

  if (platform === 'linux') {
    return <LinuxControls {...controlProps} />;
  }

  return <WindowsControls {...controlProps} />;
}

export const MenuBar = memo(function MenuBar({ onSettingsClick, onFeedbackClick }: { onSettingsClick: () => void; onFeedbackClick: () => void }) {
  const { t } = useTranslation(['menuBar', 'app', 'feedback']);
  const handleDoubleClick = useCallback(() => {
    getTauriWindow().then((w) => w?.toggleMaximize());
  }, []);

  return (
    <div className="h-8 bg-[var(--theme-bg-lighter)] flex items-center select-none relative text-cad-text">
      {/* Logo + Quick Access */}
      <div className="flex items-center gap-2 pr-3">
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" className="w-5 h-5 ml-3">
          <defs>
            <linearGradient id="menu-bg" x1="0" y1="0" x2="0.3" y2="1">
              <stop offset="0%" stopColor="#F59E0B"/>
              <stop offset="100%" stopColor="#D97706"/>
            </linearGradient>
          </defs>
          <rect width="512" height="512" rx="90" fill="url(#menu-bg)"/>
          <text x="256" y="256" textAnchor="middle" dominantBaseline="central" fontFamily="Space Grotesk,system-ui,sans-serif" fontSize="300" fontWeight="700" fill="#36363E" letterSpacing="-12">3D</text>
        </svg>
        <div className="w-px h-4 bg-cad-border" />
        <button
          className="flex items-center justify-center w-6 h-6 hover:bg-cad-hover text-cad-text-dim hover:text-cad-text transition-colors cursor-default"
          title={t('menuBar:settings')}
          onClick={onSettingsClick}
        >
          <Settings size={14} />
        </button>
      </div>

      {/* Draggable area */}
      <div
        data-tauri-drag-region
        className="flex-1 h-full cursor-default"
        onDoubleClick={handleDoubleClick}
      />

      {/* App title - fixed center */}
      <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
        <span className="text-[12px] font-medium text-[var(--theme-accent)]" style={{ fontFamily: 'var(--font-heading)' }}>
          {t('app:title')}
        </span>
        <span className="text-[var(--theme-text-muted)] text-[10px] ml-1.5">
          {t('app:version')}
        </span>
      </div>

      {/* Send Feedback + Window controls */}
      <button className="send-feedback-btn" onClick={onFeedbackClick}>
        {t('feedback:sendFeedback')}
      </button>
      <WindowControls />
    </div>
  );
});
