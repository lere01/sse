// Live SVG preview of the configured lattice: sites, nearest-neighbour
// bonds, and periodic boundaries drawn as wrap arcs. The preview is the
// primary way the page communicates geometry - it always reflects the
// current form state exactly.

const SVG = "http://www.w3.org/2000/svg";

function element(name, attributes) {
  const node = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) {
    node.setAttribute(key, value);
  }
  return node;
}

/**
 * Renders the lattice into `svg`.
 * geometry: { lx, ly, periodicX, periodicY } with ly = 1 for chains.
 */
export function renderLattice(svg, geometry) {
  const { lx, ly, periodicX, periodicY } = geometry;
  svg.replaceChildren();
  const margin = 26;
  const width = svg.clientWidth || 460;
  const height = svg.clientHeight || 300;
  const spanX = Math.max(lx - 1, 1);
  const spanY = Math.max(ly - 1, 1);
  const step = Math.min(
    (width - 2 * margin) / spanX,
    (height - 2 * margin) / Math.max(spanY, 1),
    56,
  );
  const originX = (width - step * spanX) / 2;
  const originY = (height - step * spanY) / 2;
  const position = (x, y) => [originX + x * step, originY + y * step];

  const bonds = element("g", { class: "lattice-bonds" });
  const wraps = element("g", { class: "lattice-wraps" });
  const sites = element("g", { class: "lattice-sites" });

  for (let y = 0; y < ly; y += 1) {
    for (let x = 0; x < lx; x += 1) {
      const [px, py] = position(x, y);
      if (x + 1 < lx) {
        const [qx, qy] = position(x + 1, y);
        bonds.append(
          element("line", { x1: px, y1: py, x2: qx, y2: qy }),
        );
      }
      if (y + 1 < ly) {
        const [qx, qy] = position(x, y + 1);
        bonds.append(
          element("line", { x1: px, y1: py, x2: qx, y2: qy }),
        );
      }
    }
  }

  // Periodic wraps as arcs bowing outside the lattice.
  if (periodicX && lx > 2) {
    for (let y = 0; y < ly; y += 1) {
      const [ax, ay] = position(0, y);
      const [bx] = position(lx - 1, y);
      const bow = step * 0.55;
      wraps.append(
        element("path", {
          d: `M ${ax} ${ay} C ${ax - bow} ${ay - bow}, ${bx + bow} ${ay - bow}, ${bx} ${ay}`,
        }),
      );
    }
  }
  if (periodicY && ly > 2) {
    for (let x = 0; x < lx; x += 1) {
      const [ax, ay] = position(x, 0);
      const [, by] = position(x, ly - 1);
      const bow = step * 0.55;
      wraps.append(
        element("path", {
          d: `M ${ax} ${ay} C ${ax - bow} ${ay - bow}, ${ax - bow} ${by + bow}, ${ax} ${by}`,
        }),
      );
    }
  }

  for (let y = 0; y < ly; y += 1) {
    for (let x = 0; x < lx; x += 1) {
      const [px, py] = position(x, y);
      sites.append(element("circle", { cx: px, cy: py, r: 5.2 }));
    }
  }

  svg.append(bonds, wraps, sites);
}
