# Validation

The codebase tests local identities and complete sampling behavior at several
levels.

## Local decomposition tests

For every basis state of small systems, tests reconstruct physical diagonal
energies from the sum of local SSE matrix elements and the declared energy
shift. Separate tests verify equal weights of matched TFIM constant and
spin-flip vertices.

## Trace preservation

Diagonal, TFIM cluster, local Rydberg, and global Rydberg updates are tested for
closure of the imaginary-time trace. Invalid beta values, unsupported update
rules, and malformed model references are rejected.

## Exact thermal checks

A single-spin TFIM test compares the sampled thermal energy with its analytic
value. The Rydberg benchmark constructs a dense occupation-basis Hamiltonian,
uses Jacobi rotations to obtain its eigenvalues, evaluates exact canonical
moments, and compares independent SSE chains against the exact energy.

## Parallel determinism

The same master seed and chain count produce identical per-chain results when
the number of Rayon worker threads changes. This verifies that scheduling does
not alter random streams.

## What validation does not prove

Passing the test suite does not establish equilibration or finite-size
convergence for every model parameter. It also does not prove ergodicity in
every asymptotic regime. Each scientific study must perform its own convergence
and cross-method checks.
