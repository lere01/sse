# Models

Version 0.1.0 provides sign-safe local SSE decompositions for:

- The ferromagnetic transverse-field Ising model
- A long-range Rydberg occupation model

The public `Hamiltonian` type can describe additional physical terms, but the
generic sampler requires an `SseModel` implementation with valid non-negative
local matrix elements and compatible updates. A physical Hamiltonian
description alone is not sufficient to make a new model sampleable.

Read the model page before selecting parameters:

- [Transverse-field Ising model](tfim.md)
- [Rydberg model](rydberg.md)
