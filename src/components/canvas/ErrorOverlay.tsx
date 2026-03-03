import { useEffect, useState } from 'react';
import { X, AlertTriangle } from 'lucide-react';
import { useAppStore } from '../../state/appStore';

interface ErrorToast {
  id: string;
  fileName: string;
  message: string;
  timestamp: number;
}

export function ErrorOverlay() {
  const [toasts, setToasts] = useState<ErrorToast[]>([]);
  const loadingFiles = useAppStore((s) => s.loadingFiles);
  const models = useAppStore((s) => s.models);

  // Watch for new errors in loading files
  useEffect(() => {
    const errors = loadingFiles.filter((f) => f.error);
    for (const err of errors) {
      setToasts((prev) => {
        // Don't add duplicate
        if (prev.some((t) => t.id === err.id)) return prev;
        return [...prev, {
          id: err.id,
          fileName: err.fileName,
          message: err.error!,
          timestamp: Date.now(),
        }];
      });
    }
  }, [loadingFiles]);

  // Watch for model errors
  useEffect(() => {
    const errorModels = models.filter((m) => m.loadingPhase.startsWith('Error:'));
    for (const m of errorModels) {
      setToasts((prev) => {
        if (prev.some((t) => t.id === m.id)) return prev;
        return [...prev, {
          id: m.id,
          fileName: m.fileName,
          message: m.loadingPhase.replace('Error: ', ''),
          timestamp: Date.now(),
        }];
      });
    }
  }, [models]);

  // Auto-dismiss after 15 seconds
  useEffect(() => {
    if (toasts.length === 0) return;
    const timer = setInterval(() => {
      const now = Date.now();
      setToasts((prev) => prev.filter((t) => now - t.timestamp < 15000));
    }, 1000);
    return () => clearInterval(timer);
  }, [toasts.length]);

  const dismiss = (id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
    // Also clean up from store
    useAppStore.getState().removeLoadingFile(id);
  };

  if (toasts.length === 0) return null;

  return (
    <div className="absolute top-3 right-3 z-50 flex flex-col gap-2 max-w-[420px]">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className="bg-red-900/90 border border-red-600/50 rounded-lg shadow-lg backdrop-blur-sm p-3 animate-in slide-in-from-right"
        >
          <div className="flex items-start gap-2">
            <AlertTriangle size={16} className="text-red-400 mt-0.5 flex-shrink-0" />
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium text-red-200 truncate">
                {toast.fileName}
              </div>
              <div className="text-xs text-red-300/80 mt-1 whitespace-pre-line break-words">
                {toast.message}
              </div>
            </div>
            <button
              onClick={() => dismiss(toast.id)}
              className="text-red-400 hover:text-red-200 flex-shrink-0"
            >
              <X size={14} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
