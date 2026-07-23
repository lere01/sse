// Pure statistical aggregation for SSE expansion-order series.
// Mirrors the CLI runner's estimators: energy from the expansion order,
// Geyer initial-positive-sequence autocorrelation, and split-chain R-hat.
// Everything here is deterministic and unit-tested under Node.

/** Mean of an array. */
export function mean(values) {
  let sum = 0;
  for (const value of values) sum += value;
  return sum / values.length;
}

/** Sample variance (n - 1 denominator). */
export function sampleVariance(values, valueMean = mean(values)) {
  if (values.length < 2) return 0;
  let sum = 0;
  for (const value of values) {
    const difference = value - valueMean;
    sum += difference * difference;
  }
  return sum / (values.length - 1);
}

/**
 * Thermodynamics from one chain's expansion orders.
 * energy = shift - <n> / beta; C = <n^2> - <n>^2 - <n>.
 */
export function chainThermodynamics(orders, beta, energyShift, numSites) {
  const orderMean = mean(orders);
  let squares = 0;
  for (const order of orders) squares += order * order;
  const secondMoment = squares / orders.length;
  const energy = energyShift - orderMean / beta;
  const heatCapacity = secondMoment - orderMean * orderMean - orderMean;
  return {
    energy,
    energyPerSite: energy / numSites,
    heatCapacity,
    heatCapacityPerSite: heatCapacity / numSites,
    meanExpansionOrder: orderMean,
    samples: orders.length,
  };
}

/** Mean and between-value standard error across independent chain means. */
export function combine(values) {
  const combinedMean = mean(values);
  if (values.length < 2) return { mean: combinedMean, standardError: null };
  const variance = sampleVariance(values, combinedMean);
  return {
    mean: combinedMean,
    standardError: Math.sqrt(variance / values.length),
  };
}

/**
 * Geyer initial-positive-sequence integrated autocorrelation time.
 *
 * This is a line-for-line port of qslib-quantum-variational's estimator
 * ("geyer_initial_positive_sequence_common_n_tau_floor_1"): covariances are
 * normalized by the common sample count, consecutive-lag pairs are summed
 * while positive, tau starts at -1 and accumulates 2 * pair / gamma0, and
 * the result is floored at one with ESS = N / tau.
 */
export function autocorrelation(series, maxLag = Math.min(Math.floor(series.length / 2), 10_000)) {
  const count = series.length;
  if (count < 2) return { tau: 1, ess: count };
  const seriesMean = mean(series);
  let gamma0 = 0;
  for (const value of series) {
    const difference = value - seriesMean;
    gamma0 += difference * difference;
  }
  gamma0 /= count;
  if (gamma0 === 0) return { tau: 1, ess: count };
  const covarianceAt = (lag) => {
    let sum = 0;
    for (let i = 0; i < count - lag; i += 1) {
      sum += (series[i] - seriesMean) * (series[i + lag] - seriesMean);
    }
    return sum / count;
  };
  let tau = -1;
  let lag = 0;
  const limit = Math.min(Math.max(maxLag, 1), count - 1);
  while (lag < limit) {
    const pair = covarianceAt(lag) + covarianceAt(lag + 1);
    if (pair <= 0) break;
    tau += (2 * pair) / gamma0;
    lag += 2;
  }
  const floored = Math.max(tau, 1);
  return { tau: floored, ess: count / floored };
}

/**
 * Split-chain potential scale reduction over expansion orders.
 * Each chain contributes its first and last halves as separate sequences;
 * returns null when undefined (fewer than two chains or degenerate data).
 */
export function splitRHat(chains) {
  if (chains.length < 2) return null;
  const halfLength = Math.min(
    ...chains.map((orders) => Math.floor(orders.length / 2)),
  );
  if (halfLength < 2) return null;
  const sequences = [];
  for (const orders of chains) {
    sequences.push(orders.slice(0, halfLength));
    sequences.push(orders.slice(orders.length - halfLength));
  }
  const means = sequences.map((sequence) => mean(sequence));
  const variances = sequences.map((sequence, index) =>
    sampleVariance(sequence, means[index]),
  );
  const sequenceCount = sequences.length;
  const meanOfMeans = mean(means);
  let betweenSum = 0;
  for (const value of means) {
    const difference = value - meanOfMeans;
    betweenSum += difference * difference;
  }
  const between = (halfLength * betweenSum) / (sequenceCount - 1);
  const within = mean(variances);
  if (within <= Number.EPSILON) return null;
  const estimated =
    ((halfLength - 1) / halfLength) * within + between / halfLength;
  return Math.sqrt(estimated / within);
}

/**
 * Running per-chain energy-per-site trace for the convergence plot,
 * downsampled to at most `points` samples.
 */
export function runningEnergyTrace(orders, beta, energyShift, numSites, points = 240) {
  const stride = Math.max(1, Math.ceil(orders.length / points));
  const trace = [];
  let sum = 0;
  for (let i = 0; i < orders.length; i += 1) {
    sum += orders[i];
    if ((i + 1) % stride === 0 || i === orders.length - 1) {
      const runningMean = sum / (i + 1);
      trace.push({
        measurement: i + 1,
        energyPerSite: (energyShift - runningMean / beta) / numSites,
      });
    }
  }
  return trace;
}
