'use client';

import { useEffect, useRef } from 'react';
import { hash32, rng } from '@/lib/scene/vector';
import styles from './Onboarding.module.css';

/**
 * The field, out of focus, behind the setup screen — so the first thing the app
 * shows is the thing it is about to become.
 *
 * Drawn rather than borrowed: onboarding runs before there is a corpus, so the
 * real canvas would have nothing in it on a first run, which is the run this
 * screen exists for. Deterministic from one seed, so it is the same field every
 * time rather than a new shape on each mount.
 *
 * Purely decorative: aria-hidden, never hit-tested, and it holds no state the
 * rest of the app can read.
 */

const SEED = 'parallax-onboarding-field';
const NODES = 46;
/** Roughly the app's own ratio of linked to isolated notes. */
const LINK_CHANCE = 0.62;

interface Node {
  x: number;
  y: number;
  w: number;
  h: number;
}

export default function Backdrop() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let frame = 0;

    const draw = () => {
      const parent = canvas.parentElement;
      if (!parent) return;
      const { width, height } = parent.getBoundingClientRect();
      if (width === 0 || height === 0) return;

      // Cap the backing store: this is blurred to illegibility anyway, so a
      // full device-pixel buffer is memory spent on detail nobody can see.
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, height);

      // Read the live tokens, so the field follows the theme the user picks on
      // this very screen rather than baking one palette in.
      const css = getComputedStyle(document.documentElement);
      const ink = css.getPropertyValue('--title').trim() || '#9dacaf';
      const line = css.getPropertyValue('--edge-fact').trim() || '#5a6b6e';

      const r = rng(hash32(SEED));
      const nodes: Node[] = [];
      // Golden-angle scatter, the same spiral placement falls back on when
      // nothing is similar enough to pull a note anywhere in particular.
      const golden = Math.PI * (3 - Math.sqrt(5));
      // Spread so the outermost notes clear the card. A field sized to the
      // centre of the stage is a field entirely behind the panel sitting on it,
      // which is no backdrop at all.
      const reach = Math.max(width, height) * 0.68;
      const radius = reach / Math.sqrt(NODES);
      for (let i = 0; i < NODES; i++) {
        const a = i * golden + r() * 0.35;
        const d = radius * Math.sqrt(i + 1);
        nodes.push({
          x: width / 2 + Math.cos(a) * d * 1.35,
          y: height / 2 + Math.sin(a) * d * 0.92,
          w: 34 + r() * 66,
          h: 5 + r() * 4,
        });
      }

      // Edges first, so a line never sits on top of the title it connects.
      ctx.strokeStyle = line;
      ctx.lineWidth = 1;
      ctx.globalAlpha = 0.5;
      for (let i = 1; i < nodes.length; i++) {
        if (r() > LINK_CHANCE) continue;
        const a = nodes[i]!;
        const b = nodes[Math.floor(r() * i)]!;
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.stroke();
      }

      // Titles as bars: at this blur a real word would only be mush anyway, and
      // a bar keeps the field reading as text rather than as dots.
      ctx.fillStyle = ink;
      for (const n of nodes) {
        ctx.globalAlpha = 0.42 + r() * 0.4;
        ctx.fillRect(n.x - n.w / 2, n.y - n.h / 2, n.w, n.h);
        // A second, shorter bar under about half of them — a wrapped title.
        if (r() > 0.5) {
          const w = n.w * (0.4 + r() * 0.35);
          ctx.fillRect(n.x - n.w / 2, n.y + n.h, w, n.h * 0.82);
        }
      }
      ctx.globalAlpha = 1;
    };

    draw();

    // Redraw on resize and on a theme change, both of which invalidate it.
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(draw);
    });
    if (canvas.parentElement) observer.observe(canvas.parentElement);

    const themed = new MutationObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(draw);
    });
    themed.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme', 'class'],
    });

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      themed.disconnect();
    };
  }, []);

  return <canvas ref={ref} className={styles.backdrop} aria-hidden />;
}
