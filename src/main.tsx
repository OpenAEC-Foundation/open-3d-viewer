import React from 'react';
import ReactDOM from 'react-dom/client';
import './i18n/config';
import { loadPreferences } from './state/appStore';
import App from './App';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/inter/700.css';
import '@fontsource/space-grotesk/500.css';
import '@fontsource/space-grotesk/700.css';
import '@fontsource/jetbrains-mono/400.css';
import './styles/globals.css';

loadPreferences();

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error('[ErrorBoundary]', error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div style={{ padding: 40, color: 'var(--theme-text)', background: 'var(--theme-bg)', minHeight: '100vh', fontFamily: 'var(--font-mono)' }}>
          <h1 style={{ color: 'var(--theme-danger-color)' }}>Runtime Error</h1>
          <pre style={{ whiteSpace: 'pre-wrap', fontSize: 14, color: 'var(--theme-text)' }}>
            {this.state.error.message}
          </pre>
          <pre style={{ whiteSpace: 'pre-wrap', fontSize: 12, color: 'var(--theme-text-muted)', marginTop: 16 }}>
            {this.state.error.stack}
          </pre>
          <button
            onClick={() => this.setState({ error: null })}
            style={{ marginTop: 20, padding: '8px 16px', background: 'var(--theme-btn-primary-bg)', color: 'var(--theme-btn-primary-text)', border: 'none', borderRadius: 8, fontWeight: 600, cursor: 'pointer' }}
          >
            Try Again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
