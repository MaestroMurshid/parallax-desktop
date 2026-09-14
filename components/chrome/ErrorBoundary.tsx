'use client';

import { Component, type ErrorInfo, type ReactNode } from 'react';

interface State {
  error: Error | null;
}

/**
 * Without this, one throw while rendering unmounts the whole tree and leaves a
 * blank window with no way back but quitting. The corpus lives in Rust, so a
 * reload loses nothing that was saved.
 */
export default class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('render failed', error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <main
        role="alert"
        style={{
          minHeight: '100vh',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 12,
          padding: 24,
          background: 'var(--surface)',
          color: 'var(--title)',
          fontFamily: 'var(--font-sans)',
          textAlign: 'center',
        }}
      >
        <p style={{ margin: 0 }}>Something on this screen broke. Your notes are saved.</p>
        <p style={{ margin: 0, color: 'var(--meta)', fontSize: 13 }}>{error.message}</p>
        <button
          type="button"
          onClick={() => window.location.reload()}
          style={{
            font: 'inherit',
            color: 'var(--title)',
            background: 'transparent',
            border: '1px solid var(--edge-control)',
            borderRadius: 6,
            padding: '5px 12px',
            cursor: 'pointer',
          }}
        >
          reload
        </button>
      </main>
    );
  }
}
