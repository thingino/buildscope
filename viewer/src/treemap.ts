// Squarified treemap layout (Bruls, Huizing, van Wijk). Pure geometry:
// weighted items in, pixel rects out. No rendering here.

export interface TreemapItem<T> {
  weight: number;
  data: T;
}

export interface TreemapRect<T> {
  x: number;
  y: number;
  w: number;
  h: number;
  data: T;
}

export function squarify<T>(
  items: TreemapItem<T>[],
  x: number,
  y: number,
  w: number,
  h: number
): TreemapRect<T>[] {
  const out: TreemapRect<T>[] = [];
  const total = items.reduce((a, i) => a + i.weight, 0);
  if (total <= 0 || w <= 0 || h <= 0) return out;
  const area = w * h;
  let scaled = items
    .filter((i) => i.weight > 0)
    .map((i) => ({ area: (i.weight / total) * area, data: i.data }))
    .sort((a, b) => b.area - a.area);

  let cx = x,
    cy = y,
    cw = w,
    ch = h;

  while (scaled.length > 0) {
    const side = Math.min(cw, ch);
    if (side <= 0) break;

    // Worst aspect ratio of a candidate row laid along `side`.
    const rowWorst = (row: { area: number }[]) => {
      const sum = row.reduce((a, r) => a + r.area, 0);
      const thickness = sum / side;
      let m = 0;
      for (const r of row) {
        const len = r.area / thickness;
        const aspect = Math.max(len / thickness, thickness / len);
        if (aspect > m) m = aspect;
      }
      return m;
    };

    // Grow the row while the worst aspect ratio does not degrade.
    let row = [scaled[0]];
    let best = rowWorst(row);
    let i = 1;
    while (i < scaled.length) {
      const candidate = [...row, scaled[i]];
      const cand = rowWorst(candidate);
      if (cand <= best) {
        row = candidate;
        best = cand;
        i++;
      } else {
        break;
      }
    }

    const sum = row.reduce((a, r) => a + r.area, 0);
    const thickness = sum / side;
    if (cw >= ch) {
      let ry = cy;
      for (const r of row) {
        const len = r.area / thickness;
        out.push({ x: cx, y: ry, w: thickness, h: len, data: r.data });
        ry += len;
      }
      cx += thickness;
      cw -= thickness;
    } else {
      let rx = cx;
      for (const r of row) {
        const len = r.area / thickness;
        out.push({ x: rx, y: cy, w: len, h: thickness, data: r.data });
        rx += len;
      }
      cy += thickness;
      ch -= thickness;
    }
    scaled = scaled.slice(row.length);
  }
  return out;
}
