import i18next from 'i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

// English
import enApp from './locales/en/app.json';
import enMenuBar from './locales/en/menuBar.json';
import enRibbon from './locales/en/ribbon.json';
import enPanels from './locales/en/panels.json';
import enStatusBar from './locales/en/statusBar.json';
import enViewCube from './locales/en/viewCube.json';
import enWelcome from './locales/en/welcome.json';
import enSettings from './locales/en/settings.json';
import enLoader from './locales/en/loader.json';
import enFileFilter from './locales/en/fileFilter.json';
import enFeedback from './locales/en/feedback.json';

// Dutch
import nlApp from './locales/nl/app.json';
import nlMenuBar from './locales/nl/menuBar.json';
import nlRibbon from './locales/nl/ribbon.json';
import nlPanels from './locales/nl/panels.json';
import nlStatusBar from './locales/nl/statusBar.json';
import nlViewCube from './locales/nl/viewCube.json';
import nlWelcome from './locales/nl/welcome.json';
import nlSettings from './locales/nl/settings.json';
import nlLoader from './locales/nl/loader.json';
import nlFileFilter from './locales/nl/fileFilter.json';
import nlFeedback from './locales/nl/feedback.json';

export const SUPPORTED_LANGUAGES = [
  { code: 'en', label: 'English' },
  { code: 'nl', label: 'Nederlands' },
] as const;

export type LanguageCode = (typeof SUPPORTED_LANGUAGES)[number]['code'];

i18next.use(LanguageDetector).init({
  resources: {
    en: {
      app: enApp,
      menuBar: enMenuBar,
      ribbon: enRibbon,
      panels: enPanels,
      statusBar: enStatusBar,
      viewCube: enViewCube,
      welcome: enWelcome,
      settings: enSettings,
      loader: enLoader,
      fileFilter: enFileFilter,
      feedback: enFeedback,
    },
    nl: {
      app: nlApp,
      menuBar: nlMenuBar,
      ribbon: nlRibbon,
      panels: nlPanels,
      statusBar: nlStatusBar,
      viewCube: nlViewCube,
      welcome: nlWelcome,
      settings: nlSettings,
      loader: nlLoader,
      fileFilter: nlFileFilter,
      feedback: nlFeedback,
    },
  },
  fallbackLng: 'en',
  supportedLngs: ['en', 'nl'],
  ns: ['app', 'menuBar', 'ribbon', 'panels', 'statusBar', 'viewCube', 'welcome', 'settings', 'loader', 'fileFilter', 'feedback'],
  defaultNS: 'app',
  interpolation: {
    escapeValue: false,
  },
  detection: {
    order: ['localStorage', 'navigator'],
    caches: ['localStorage'],
    lookupLocalStorage: 'open3d-language',
  },
});

// Persist language changes to Tauri store
import { setPref } from '../state/preferences';
i18next.on('languageChanged', (lng: string) => {
  setPref('language', lng);
});

export default i18next;
