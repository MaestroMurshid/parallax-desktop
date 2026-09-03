import type { Metadata } from 'next';
import { Archivo, IBM_Plex_Mono, Newsreader } from 'next/font/google';
import { APP_NAME, APP_TAGLINE } from '@/lib/constants';
import ThemeSync from '@/components/chrome/ThemeSync';
import './globals.css';

/* Newsreader carries the canvas titles and the question — the two voices that
   are not chrome. Archivo is the chrome. Plex Mono is anything the machine says. */
const newsreader = Newsreader({ subsets: ['latin'], variable: '--font-newsreader', display: 'swap' });
const archivo = Archivo({ subsets: ['latin'], variable: '--font-archivo', display: 'swap' });
const plexMono = IBM_Plex_Mono({
  subsets: ['latin'],
  weight: ['400', '500'],
  variable: '--font-plex-mono',
  display: 'swap',
});

export const metadata: Metadata = {
  title: APP_NAME,
  description: APP_TAGLINE,
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${newsreader.variable} ${archivo.variable} ${plexMono.variable}`}>
      <body>
        <ThemeSync />
        {children}
      </body>
    </html>
  );
}
