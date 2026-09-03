'use client';

import { useEffect } from 'react';
import { useApp } from '@/lib/store';

/** Writes the theme choice onto <html>; 'system' removes the attribute so the
 *  prefers-color-scheme block in globals.css takes over. */
export default function ThemeSync() {
  const theme = useApp((s) => s.theme);
  useEffect(() => {
    const root = document.documentElement;
    if (theme === 'system') root.removeAttribute('data-theme');
    else root.dataset.theme = theme;
  }, [theme]);
  return null;
}
