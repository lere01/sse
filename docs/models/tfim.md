# Transverse-field Ising model

The implemented Hamiltonian is

\[
H = -J\sum_{\langle i,j\rangle}\sigma_i^z\sigma_j^z
    -h\sum_i\sigma_i^x.
\]

The built-in local model requires finite, non-negative \(J\) and \(h\). Thus it
currently represents a ferromagnetic Ising interaction in this sign
convention. Every supplied pair contributes one bond term.

## Local decomposition

For a bond \((i,j)\), the diagonal SSE operator is

\[
B_{ij}=J(1+\sigma_i^z\sigma_j^z).
\]

For each site, the matched transverse vertices are

\[
B_i^{\mathrm{diag}}=hI,
\qquad
B_i^{\mathrm{offdiag}}=h\sigma_i^x.
\]

The physical energy shift is

\[
E_{\mathrm{shift}}=J N_b+hN,
\]

where \(N_b\) is the number of supplied unordered bonds and \(N\) is the site
count.

## Update

One TFIM sweep consists of:

1. A diagonal insertion and removal sweep.
2. A linked-cluster sweep over space-imaginary-time vertices.

The matched single-site vertices have equal weight, allowing cluster changes
without a Metropolis correction for the supported TFIM model.

## Low-temperature interpretation

Increasing \(\beta\) suppresses excited-state contributions. A calculation is
ground-state-like only after observables have converged as \(\beta\) increases
for the finite lattice being studied. Near a small finite-size gap, the
required \(\beta\) may be much larger than an estimate based only on local
energy scales.
