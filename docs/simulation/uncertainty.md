# Uncertainty and autocorrelation

Successive Monte Carlo measurements are generally correlated. The independent
sample formula underestimates uncertainty when applied directly to a correlated
time series.

For a stationary observable with normalized autocorrelation \(\rho(t)\), the
integrated autocorrelation time is commonly written

\[
\tau_{\mathrm{int}}=\frac{1}{2}+\sum_{t=1}^{\infty}\rho(t).
\]

An approximate effective sample count is

\[
N_{\mathrm{eff}}\approx\frac{N}{2\tau_{\mathrm{int}}}.
\]

Configured runs truncate the correlation sum at the first non-positive or
non-finite estimate, with a maximum lag of 10,000. This positive-window method
is a diagnostic estimator, not a universal optimal-window procedure.

## Between-chain uncertainty

The configured runner computes the sample standard deviation of independent
chain energy-density means and divides by the square root of the chain count.
This estimator is unavailable for a single chain and meaningful only when
chains are independently seeded and individually long enough to equilibrate
and explore their stationary distribution.

The summary also reports split \(\hat R\) for the expansion-order series. Values
above 1.01 generate a warning. This is a useful agreement diagnostic, not proof
of equilibration or ergodicity.

## Reporting

Report at least:

- Number of independent chains
- Measurements per chain
- Sweeps between measurements
- Thermalization sweeps
- Mean and named uncertainty estimator
- Autocorrelation estimate or blocking procedure
- Acceptance statistics for corrected proposals

Do not quote more significant digits than the uncertainty supports.
