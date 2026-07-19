# Rydberg model

The implemented Hamiltonian is

\[
H = -\frac{\Omega}{2}\sum_i\sigma_i^x
    -\Delta\sum_i n_i
    +\sum_{i<j}\frac{C_6}{r_{ij}^6}n_i n_j.
\]

Here \(n_i=(1+\sigma_i^z)/2\). The implementation parameter names are
`omega`, `detuning`, and `c6` for \(\Omega\), \(\Delta\), and \(C_6\).

`omega` must be finite and non-negative. `detuning` and `c6` may have either
sign. All site pairs are included, so model construction and diagonal updates
scale with the number of pairs. Coincident custom coordinates are invalid
because \(r_{ij}^{-6}\) would be undefined.

## Local decomposition

Each site receives matched constant and spin-flip terms of amplitude
\(\Omega/2\). The onsite occupation term uses a shift

\[
C_i=\max(-\Delta,0)
\]

so that \(C_i+\Delta n_i\) is non-negative. Each pair uses interaction
\(V_{ij}=C_6/r_{ij}^6\) and shift

\[
C_{ij}=\max(V_{ij},0)
\]

so that \(C_{ij}-V_{ij}n_i n_j\) is non-negative.

## Updates

The recommended `local` update proposes site-local changes and evaluates their
full propagated weight ratio. The `global` update constructs a larger cluster
proposal and applies one global correction. Compare them on small systems when
validating a new physical regime, but do not assume comparable acceptance or
mixing at larger size.

## Long-range geometry

Periodic chain and rectangular distances use the minimum-image convention. The
current interaction includes one term for every unordered site pair evaluated
at that distance. It does not perform an Ewald sum or add interactions with
multiple periodic images.
