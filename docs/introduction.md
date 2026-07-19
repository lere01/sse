# SSE

`sse` is a finite-temperature stochastic series expansion quantum Monte Carlo
implementation for quantum lattice models. It currently supports
transverse-field Ising and Rydberg Hamiltonians on chains, rectangular
lattices, and custom open two-dimensional geometries.

The project is designed around three goals:

- State every physical and numerical convention explicitly.
- Keep sampled local matrix elements non-negative for supported models.
- Make validation and reproducibility part of the normal workflow.

Version 0.1.0 provides a Rust library and executable examples. A stable
configuration-file interface and result schema are planned for a later
release. The guide documents the software that exists today and labels planned
features as such.

## What the program computes

At inverse temperature \(\beta\), thermal expectation values are defined by

\[
\langle A \rangle_\beta =
\frac{\operatorname{Tr}(A e^{-\beta H})}
     {\operatorname{Tr}(e^{-\beta H})}.
\]

The sampler expands the partition function in powers of \(\beta\), represents
each sampled term with a local operator, and updates a padded operator string.
Energy and heat capacity follow from moments of its non-identity expansion
order.

Large \(\beta\) can approximate ground-state observables when it is large
relative to the inverse finite-size gap. This is a convergence condition, not
an automatic guarantee. Results should be checked as \(\beta\), system size,
thermalization, and sample count are varied.

## Where to begin

1. Read [Physical conventions](physical-conventions.md).
2. Run the small example in [Getting started](getting-started.md).
3. Select the relevant page under [Models](models/index.md).
4. Plan convergence tests using [Convergence](simulation/convergence.md).
5. Record the information listed under
   [Reproducibility](simulation/reproducibility.md).

Rust developers can use the generated [API reference](api.md) after learning
the physical conventions described here.
