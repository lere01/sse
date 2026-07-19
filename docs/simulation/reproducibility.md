# Reproducibility

The configured runner derives each chain seed deterministically from a master
seed and chain index. Chain trajectories are therefore independent of the
number of Rayon worker threads used to schedule them within a software version.
A regression test checks this property.

Record the following for every scientific result:

- `sse` version and Git revision
- Rust toolchain version when building from source
- Complete Hamiltonian coefficients and sign convention
- Geometry, dimensions, site ordering, and boundary conditions
- Inverse temperature
- Initial operator-string cutoff
- Thermalization sweeps
- Measurement count and sweeps per measurement
- Number of chains and worker threads
- Master seed
- TFIM, local Rydberg, or global reference update
- Any post-processing used to estimate uncertainty

`sse run` preserves the submitted and resolved configurations, software
version, optional embedded Git revision, deterministic chain seeds, raw
expansion-order series, and named statistical diagnostics. Archive the entire
run directory rather than copying only the terminal summary. See
[Run artifacts and restart](../artifacts.md).

The random-number generator implementation may change across software
versions. Reproduction therefore requires both the seed and recorded software
version, not the seed alone.

Never treat a random seed as a substitute for convergence testing. Exact
reproducibility can reproduce a biased or insufficiently equilibrated result.
