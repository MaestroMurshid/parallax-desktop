/**
 * The tidy pass — user-invoked, never automatic.
 *
 * §5.1 freezes positions because a layout that re-solves on every insert means
 * nothing is ever where you left it, and spatial memory is the thing the canvas
 * is for. That argument is about the app moving your notes behind your back. It
 * says nothing about you asking for a tidy: you chose it, you watched it happen,
 * and you can put a note back by dragging it.
 *
 * All it does is take up slack between notes that are already linked. Every move
 * is rejected if it would put two titles on top of each other, so the one
 * guarantee placement makes — you can always read every title — survives.
 */

export interface RelaxNode {
  id: string;
  x: number;
  y: number;
  halfW: number;
  halfH: number;
}

export interface RelaxOptions {
  iterations: number;
  /** Fraction of the way to the linked centroid, per pass. Small: this is a
   *  settling motion, not a re-solve. */
  pull: number;
  /** How firmly an overlapping pair pushes apart. */
  push: number;
  padX: number;
  padY: number;
}

export const DEFAULT_RELAX: RelaxOptions = {
  iterations: 300,
  pull: 0.1,
  push: 0.3,
  padX: 30,
  padY: 26,
};

function collides(a: RelaxNode, b: RelaxNode, padX: number, padY: number): boolean {
  return (
    Math.abs(a.x - b.x) < a.halfW + b.halfW + padX &&
    Math.abs(a.y - b.y) < a.halfH + b.halfH + padY
  );
}

/**
 * Returns the new position of every node that moved. Input is not mutated.
 * `links` is the drawn graph — only what the canvas actually renders pulls,
 * because the point is to shorten the lines you can see.
 */
export function relaxLayout(
  nodes: readonly RelaxNode[],
  links: ReadonlyMap<string, readonly string[]>,
  opts: RelaxOptions = DEFAULT_RELAX,
): Map<string, { x: number; y: number }> {
  const field: RelaxNode[] = nodes.map((n) => ({ ...n }));
  const byId = new Map(field.map((n) => [n.id, n]));
  const { padX, padY, pull, push } = opts;

  for (let pass = 0; pass < opts.iterations; pass++) {
    // Toward what you are linked to, but only if the seat is free.
    for (const n of field) {
      const neighbours = links.get(n.id);
      if (!neighbours?.length) continue;

      let tx = 0;
      let ty = 0;
      let count = 0;
      for (const id of neighbours) {
        const m = byId.get(id);
        if (!m) continue;
        tx += m.x;
        ty += m.y;
        count++;
      }
      if (count === 0) continue;

      const ox = n.x;
      const oy = n.y;
      n.x = ox + (tx / count - ox) * pull;
      n.y = oy + (ty / count - oy) * pull;

      // Reject rather than resolve: a move that would overlap simply does not
      // happen, which is why this can never produce an unreadable map.
      for (const m of field) {
        if (m !== n && collides(n, m, padX, padY)) {
          n.x = ox;
          n.y = oy;
          break;
        }
      }
    }

    // Anything already too close — including notes that started that way —
    // eases apart along its shallower axis, so titles separate sideways.
    for (let i = 0; i < field.length; i++) {
      for (let j = i + 1; j < field.length; j++) {
        const a = field[i]!;
        const b = field[j]!;
        const dx = a.x - b.x;
        const dy = a.y - b.y;
        const overX = a.halfW + b.halfW + padX - Math.abs(dx);
        const overY = a.halfH + b.halfH + padY - Math.abs(dy);
        if (overX <= 0 || overY <= 0) continue;

        if (overX < overY) {
          const shift = (Math.sign(dx) || 1) * overX * push;
          a.x += shift / 2;
          b.x -= shift / 2;
        } else {
          const shift = (Math.sign(dy) || 1) * overY * push;
          a.y += shift / 2;
          b.y -= shift / 2;
        }
      }
    }
  }

  const moved = new Map<string, { x: number; y: number }>();
  for (const n of field) {
    const before = nodes.find((o) => o.id === n.id)!;
    if (Math.abs(before.x - n.x) > 0.5 || Math.abs(before.y - n.y) > 0.5) {
      moved.set(n.id, { x: n.x, y: n.y });
    }
  }
  return moved;
}
