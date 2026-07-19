# Reproducibility

The parallel TFIM runner derives each chain seed deterministically from a master
seed and chain index. Chain trajectories are therefore independent of the
number of Rayon worker threads used to schedule them. A regression test checks
this property.

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

Version 0.1.0 prints summaries but does not yet create a versioned result
manifest. Until that artifact format is implemented, preserve the exact command
line and redirect program output into a study-specific directory.

Never treat a random seed as a substitute for convergence testing. Exact
reproducibility can reproduce a biased or insufficiently equilibrated result.
