# Convergence

Convergence should be demonstrated separately for Monte Carlo sampling,
inverse temperature, and system size.

## Thermalization

Measurements made before equilibration are biased. Compare runs started from
different basis states, inspect chain histories when available, and increase
the discarded thermalization interval until reported observables are stable.

The examples use fixed thermalization counts for reproducibility. Those counts
are not universal recommendations.

## Measurement count

Increasing raw measurement count reduces uncertainty only according to the
effective number of independent samples. Strongly correlated sweeps can add
little information. Increase the run length until uncertainties are small
relative to the physical effect being resolved.

## Measurement spacing

`sweeps_per_measurement` controls the number of complete updates between
recorded expansion orders. Additional spacing can reduce stored serial
correlation but does not replace an explicit autocorrelation analysis.

## Independent chains

Use multiple independently seeded chains. Their means provide a simple check
for incomplete equilibration and a conservative uncertainty estimate when each
chain is long compared with its autocorrelation time.

## Inverse temperature

Finite-temperature results require the intended physical \(\beta\). Ground-state
approximations require a sequence of increasing \(\beta\) values until relevant
observables stop changing within uncertainty. The necessary scale depends on
the finite-size excitation gap.

## System size

Thermodynamic conclusions require multiple lattice sizes. Near criticality,
finite-size corrections can dominate statistical error. Boundary conditions
must remain consistent across the scaling sequence.

## Operator-string cutoff

The sampler grows the padded string when headroom becomes small. Record the
initial and final cutoff and compare them with the realized expansion order.
Repeated large growth is not necessarily incorrect, but it affects memory and
can reveal a poor initial estimate.
