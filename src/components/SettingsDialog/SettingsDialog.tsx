import { useState, useRef, useEffect } from 'react';
import { useAppStore, type UITheme } from '../../state/appStore';
import { useTranslation } from '../../i18n/useTranslation';
import { SUPPORTED_LANGUAGES } from '../../i18n/config';
import './SettingsDialog.css';

const THEME_KEYS: { value: UITheme; key: string; swatches: string[] }[] = [
  { value: 'forge', key: 'forge', swatches: ['#36363E', '#44444C', '#D97706', '#FAFAF9'] },
  { value: 'light', key: 'light', swatches: ['#FAFAF9', '#FFFFFF', '#D97706', '#36363E'] },
  { value: 'openaec', key: 'openaec', swatches: ['#27272A', '#36363E', '#D97706', '#FAFAF9'] },
  { value: 'blueprint', key: 'blueprint', swatches: ['#0F1B2D', '#1A2C45', '#60A5FA', '#E0E7FF'] },
  { value: 'contrast', key: 'contrast', swatches: ['#000000', '#0A0A0A', '#FFD700', '#FFFFFF'] },
];

interface SettingsDialogProps {
  onClose: () => void;
}

export function SettingsDialog({ onClose }: SettingsDialogProps) {
  const { t, i18n, language } = useTranslation('settings');
  const uiTheme = useAppStore((s) => s.uiTheme);
  const setUITheme = useAppStore((s) => s.setUITheme);

  const dialogRef = useRef<HTMLDivElement>(null);
  const [offset, setOffset] = useState<{ x: number; y: number } | null>(null);
  const dragging = useRef(false);
  const dragStart = useRef({ x: 0, y: 0 });

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      setOffset({
        x: e.clientX - dragStart.current.x,
        y: e.clientY - dragStart.current.y,
      });
    };
    const handleMouseUp = () => {
      dragging.current = false;
    };
    window.addEventListener('mousemove', handleMouseMove);
    window.addEventListener('mouseup', handleMouseUp);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, []);

  const handleHeaderMouseDown = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('.settings-close')) return;
    dragging.current = true;
    dragStart.current = {
      x: e.clientX - (offset?.x ?? 0),
      y: e.clientY - (offset?.y ?? 0),
    };
  };

  const handleThemeChange = (theme: UITheme) => {
    setUITheme(theme);
    document.documentElement.setAttribute('data-theme', theme);
  };

  return (
    <div className="settings-dialog-overlay">
      <div
        ref={dialogRef}
        className="settings-dialog"
        style={offset ? { transform: `translate(${offset.x}px, ${offset.y}px)` } : undefined}
      >
        <div className="settings-header" onMouseDown={handleHeaderMouseDown}>
          <h2 className="settings-title">{t('title')}</h2>
          <button className="settings-close" onClick={onClose}>&times;</button>
        </div>

        <div className="settings-body">
          <div className="settings-sidebar">
            <button className="settings-tab active">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
              </svg>
              <span>{t('general')}</span>
            </button>
          </div>

          <div className="settings-content">
            <div className="settings-section">
              <h3 className="settings-section-title">{t('theme')}</h3>
              <div className="settings-theme-table">
                {THEME_KEYS.map(theme => (
                  <button
                    key={theme.value}
                    className={`settings-theme-row${uiTheme === theme.value ? ' active' : ''}`}
                    onClick={() => handleThemeChange(theme.value)}
                  >
                    <span className="settings-theme-row-swatches">
                      {theme.swatches.map((color, i) => (
                        <span key={i} className="settings-theme-row-swatch" style={{ background: color }} />
                      ))}
                    </span>
                    <span className="settings-theme-row-name">{t(`themes.${theme.key}`)}</span>
                  </button>
                ))}
              </div>
            </div>

            <div className="settings-section">
              <h3 className="settings-section-title">{t('language')}</h3>
              <select
                className="settings-language-select"
                value={SUPPORTED_LANGUAGES.find(l => language === l.code || language?.startsWith(l.code + '-'))?.code ?? 'en'}
                onChange={(e) => i18n.changeLanguage(e.target.value)}
              >
                {SUPPORTED_LANGUAGES.map(lang => (
                  <option key={lang.code} value={lang.code}>{lang.label}</option>
                ))}
              </select>
            </div>
          </div>
        </div>

        <div className="settings-footer">
          <button className="settings-footer-close" onClick={onClose}>{t('close')}</button>
        </div>
      </div>
    </div>
  );
}
