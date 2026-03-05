import { useCallback, useSyncExternalStore } from 'react';
import i18next from './config';

/**
 * Reactive i18n hook — re-renders the component when the language changes.
 *
 * @param ns — namespace(s) to use (default: 'app')
 * @returns { t, i18n, language }
 */
export function useTranslation(ns: string | string[] = 'app') {
  const subscribe = useCallback((onStoreChange: () => void) => {
    i18next.on('languageChanged', onStoreChange);
    return () => i18next.off('languageChanged', onStoreChange);
  }, []);

  const language = useSyncExternalStore(
    subscribe,
    () => i18next.language,
    () => i18next.language,
  );

  const t = useCallback(
    (key: string, options?: Record<string, unknown>) => {
      return i18next.t(key, { ns, ...options });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [language, ns],
  );

  return { t, i18n: i18next, language };
}

/**
 * Non-reactive translation for use outside React components (loaders, utilities).
 * Always uses the current i18next language at call time.
 */
export function t(key: string, options?: Record<string, unknown>): string {
  return i18next.t(key, options) as string;
}
