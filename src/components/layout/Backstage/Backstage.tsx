import { memo, useEffect, useState } from 'react';
import { FolderOpen, Settings, MessageSquare, Info, LogOut, ArrowLeft } from 'lucide-react';
import { useTranslation } from '../../../i18n/useTranslation';
import './Backstage.css';

type Panel = 'none' | 'about';

const isTauri = !!(window as any).__TAURI_INTERNALS__;

async function closeWindow() {
  if (!isTauri) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow().close();
}

/** File menu (OpenAEC Backstage): full-height overlay below the title bar, Escape closes it. */
export const Backstage = memo(function Backstage({
  onClose,
  onOpen,
  onPreferences,
  onFeedback,
}: {
  onClose: () => void;
  onOpen: () => void;
  onPreferences: () => void;
  onFeedback: () => void;
}) {
  const { t } = useTranslation(['backstage', 'app']);
  const [panel, setPanel] = useState<Panel>('none');

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const run = (fn: () => void) => () => {
    onClose();
    fn();
  };

  return (
    <div className="backstage-overlay" role="dialog" aria-label={t('backstage:file')}>
      <nav className="backstage-sidebar">
        <button className="backstage-back" onClick={onClose} title={t('backstage:close')}>
          <ArrowLeft size={18} />
        </button>
        <button className="backstage-item" onClick={run(onOpen)} title={t('backstage:openHint')}>
          <FolderOpen size={16} />
          <span>{t('backstage:open')}</span>
          <span className="backstage-shortcut">Ctrl+O</span>
        </button>
        <div className="backstage-divider" />
        <button className="backstage-item" onClick={run(onPreferences)}>
          <Settings size={16} />
          <span>{t('backstage:preferences')}</span>
          <span className="backstage-shortcut">Ctrl+,</span>
        </button>
        <button className="backstage-item" onClick={run(onFeedback)}>
          <MessageSquare size={16} />
          <span>{t('backstage:feedback')}</span>
        </button>
        <button className={`backstage-item ${panel === 'about' ? 'active' : ''}`} onClick={() => setPanel('about')}>
          <Info size={16} />
          <span>{t('backstage:about')}</span>
        </button>
        <div className="backstage-spacer" />
        {isTauri && (
          <button className="backstage-item" onClick={() => void closeWindow()}>
            <LogOut size={16} />
            <span>{t('backstage:exit')}</span>
            <span className="backstage-shortcut">Alt+F4</span>
          </button>
        )}
      </nav>
      <section className="backstage-content">
        {panel === 'about' && (
          <div className="backstage-about">
            <h2>{t('backstage:aboutTitle')}</h2>
            <p className="backstage-version">
              {t('app:version', { version: __APP_VERSION__ })} · {t('app:build', { build: __APP_BUILD__ })}
            </p>
            <p>{t('backstage:aboutBody')}</p>
            <p>{t('backstage:builtOn')}</p>
            <dl>
              <dt>{t('backstage:licence')}</dt>
              <dd>LGPL-3.0-or-later</dd>
              <dt>{t('backstage:source')}</dt>
              <dd>github.com/OpenAEC-Foundation/open-3d-viewer</dd>
            </dl>
          </div>
        )}
      </section>
    </div>
  );
});
