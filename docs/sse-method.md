# The SSE method

The stochastic series expansion starts from the partition function

\[
Z = \operatorname{Tr}(e^{-\beta H})
  = \sum_{n=0}^{\infty}\frac{\beta^n}{n!}
    \operatorname{Tr}[(-H)^n].
\]

For the built-in models, local shifts permit the decomposition

\[
H = E_{\mathrm{shift}} - \sum_a B_a
\]

with non-negative sampled local matrix elements. The scalar shift factors out
of normalized observables, while its contribution is restored in physical
energy estimators.

## Fixed-length representation

The implementation stores a padded operator string of length \(M\). A sampled
configuration contains \(n\) non-identity operators and \(M-n\) identity
positions. The sampler grows \(M\) automatically when fewer than approximately
25 percent empty positions remain, with a minimum empty-headroom target.

Automatic growth prevents simple cutoff saturation, but users should still
inspect the realized expansion order and verify that pathological growth does
not signal an unsuitable simulation regime.

## Diagonal update

A diagonal sweep walks through the operator string while propagating the basis
state. At identity positions it proposes insertion of a randomly selected
diagonal term. At diagonal positions it proposes removal. The Metropolis ratios
include the local matrix element, inverse temperature, number of eligible
diagonal terms, current expansion order, and padded string length.

Off-diagonal operators encountered during the sweep update the propagated
basis state. A completed sweep must return to the initial imaginary-time
boundary state.

## TFIM cluster update

The TFIM update constructs linked world-line vertices. Connected components
are independently flipped with probability one half. Matched single-site
constant and spin-flip vertices are toggled when their incoming and outgoing
cluster decisions differ. The implemented breakup is valid only for the
built-in TFIM local model.

## Rydberg updates

Occupation-dependent diagonal weights prevent direct reuse of the uncorrected
TFIM cluster rule.

The production Rydberg update makes local world-line proposals on each site.
It either toggles a pair of transverse partner vertices or flips the entire
imaginary-time boundary spin. Proposed changes receive a Metropolis correction
from the ratio of propagated local weights.

The global Rydberg reference update proposes a trace-preserving cluster change
and then applies one global Metropolis correction. Its acceptance can become
poor for larger or strongly interacting systems. It is retained primarily for
small-system comparison.

## Thermodynamic estimators

For expansion order \(n\), the instantaneous physical energy estimator is

\[
E = E_{\mathrm{shift}} - \frac{n}{\beta}.
\]

The heat capacity in units with \(k_B=1\) is estimated from

\[
C = \langle n^2\rangle - \langle n\rangle^2 - \langle n\rangle.
\]

These formulas do not remove Monte Carlo autocorrelation. Statistical analysis
must account for correlated measurements and independent-chain variation.
