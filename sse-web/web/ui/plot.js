// Hand-rolled SVG convergence plot: one faint running-mean trace per
// chain, a shaded combined mean +/- SE band, and an optional beta-ladder
// inset. No chart library; the data volume is tiny and the rendering
// contract (theme-aware, animated by data arrival only) is easier to keep
// by drawing directly.

const SVG = "http://www.w3.org/2000/svg";

function element(name, attributes) {
  const node = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) {
    node.setAttribute(key, value);
  }
  return node;
}

function extent(values, pad = 0.12) {
  let low = Math.min(...values);
  let high = Math.max(...values);
  if (!Number.isFinite(low) || !Number.isFinite(high)) return [0, 1];
  if (high - low < 1e-12) {
    low -= 0.5;
    high += 0.5;
  }
  const span = high - low;
  return [low - pad * span, high + pad * span];
}

/**
 * Draws the convergence plot.
 * traces: per chain, [{measurement, energyPerSite}, ...]
 * band: {mean, standardError} | null
 */
export function renderConvergence(svg, traces, band) {
  svg.replaceChildren();
  const width = svg.clientWidth || 560;
  const height = svg.clientHeight || 220;
  const margin = { top: 12, right: 14, bottom: 26, left: 58 };
  const innerWidth = width - margin.left - margin.right;
  const innerHeight = height - margin.top - margin.bottom;
  if (!traces.length || !traces[0].length) return;

  const allX = traces.flatMap((trace) => trace.map((p) => p.measurement));
  const allY = traces.flatMap((trace) => trace.map((p) => p.energyPerSite));
  const [minX, maxX] = [Math.min(...allX), Math.max(...allX)];
  const [minY, maxY] = extent(allY);
  const scaleX = (x) =>
    margin.left + ((x - minX) / Math.max(maxX - minX, 1)) * innerWidth;
  const scaleY = (y) =>
    margin.top + (1 - (y - minY) / (maxY - minY)) * innerHeight;

  // Axes: minimal - a baseline, three y ticks with labels.
  const axes = element("g", { class: "plot-axes" });
  const tickCount = 3;
  for (let i = 0; i <= tickCount; i += 1) {
    const value = minY + ((maxY - minY) * i) / tickCount;
    const y = scaleY(value);
    axes.append(
      element("line", {
        x1: margin.left,
        y1: y,
        x2: width - margin.right,
        y2: y,
        class: "plot-grid",
      }),
    );
    const label = element("text", {
      x: margin.left - 8,
      y: y + 3.5,
      class: "plot-tick",
      "text-anchor": "end",
    });
    label.textContent = value.toFixed(4);
    axes.append(label);
  }
  const xLabel = element("text", {
    x: margin.left + innerWidth / 2,
    y: height - 6,
    class: "plot-tick",
    "text-anchor": "middle",
  });
  xLabel.textContent = "measurements per chain";
  axes.append(xLabel);
  svg.append(axes);

  // Combined mean +/- SE band across the plotted range.
  if (band && band.standardError !== null) {
    const top = scaleY(band.mean + band.standardError);
    const bottom = scaleY(band.mean - band.standardError);
    svg.append(
      element("rect", {
        x: margin.left,
        y: Math.min(top, bottom),
        width: innerWidth,
        height: Math.max(Math.abs(bottom - top), 1),
        class: "plot-band",
      }),
    );
    svg.append(
      element("line", {
        x1: margin.left,
        y1: scaleY(band.mean),
        x2: width - margin.right,
        y2: scaleY(band.mean),
        class: "plot-mean",
      }),
    );
  }

  traces.forEach((trace, index) => {
    const points = trace
      .map((p) => `${scaleX(p.measurement).toFixed(1)},${scaleY(p.energyPerSite).toFixed(1)}`)
      .join(" ");
    svg.append(
      element("polyline", {
        points,
        class: `plot-trace trace-${index % 8}`,
        fill: "none",
      }),
    );
  });
}

/**
 * Draws the beta-ladder inset: energy per site against beta with error
 * bars, communicating ground-state convergence.
 * points: [{beta, energyPerSite, standardError}]
 */
export function renderLadder(svg, points) {
  svg.replaceChildren();
  if (points.length < 2) return;
  const width = svg.clientWidth || 240;
  const height = svg.clientHeight || 130;
  const margin = { top: 10, right: 12, bottom: 22, left: 46 };
  const innerWidth = width - margin.left - margin.right;
  const innerHeight = height - margin.top - margin.bottom;
  const betas = points.map((p) => p.beta);
  const values = points.flatMap((p) => [
    p.energyPerSite - (p.standardError ?? 0),
    p.energyPerSite + (p.standardError ?? 0),
  ]);
  const [minX, maxX] = [Math.min(...betas), Math.max(...betas)];
  const [minY, maxY] = extent(values, 0.25);
  const scaleX = (x) =>
    margin.left + ((x - minX) / Math.max(maxX - minX, 1e-9)) * innerWidth;
  const scaleY = (y) =>
    margin.top + (1 - (y - minY) / (maxY - minY)) * innerHeight;

  const line = points
    .map((p) => `${scaleX(p.beta).toFixed(1)},${scaleY(p.energyPerSite).toFixed(1)}`)
    .join(" ");
  svg.append(element("polyline", { points: line, class: "plot-mean", fill: "none" }));
  for (const point of points) {
    const x = scaleX(point.beta);
    if (point.standardError !== null) {
      svg.append(
        element("line", {
          x1: x,
          y1: scaleY(point.energyPerSite - point.standardError),
          x2: x,
          y2: scaleY(point.energyPerSite + point.standardError),
          class: "plot-errorbar",
        }),
      );
    }
    svg.append(
      element("circle", {
        cx: x,
        cy: scaleY(point.energyPerSite),
        r: 3.2,
        class: "plot-point",
      }),
    );
    const label = element("text", {
      x,
      y: height - 6,
      class: "plot-tick",
      "text-anchor": "middle",
    });
    label.textContent = `β=${point.beta}`;
    svg.append(label);
  }
}
