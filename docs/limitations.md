# Known limitations

Version 0.1.0 has the following explicit limitations:

- The stable user interface is a Rust API plus Cargo examples, not a
  configuration-driven standalone CLI.
- Result manifests and versioned JSON, CSV, or Parquet schemas are not yet
  implemented.
- Built-in sampleable models are limited to ferromagnetic TFIM and Rydberg
  Hamiltonians in the documented sign conventions.
- Custom coordinates do not support periodic simulation cells.
- Periodic long-range Rydberg interactions use one minimum-image pair term and
  do not perform an Ewald or repeated-image sum.
- Lattices store unique unordered site pairs and do not represent bond
  multiplicity on tiny periodic cells.
- The global Rydberg cluster proposal can have poor acceptance.
- The exact Rydberg benchmark uses a dense Jacobi solver and is limited to six
  sites.
- The library does not automatically certify thermalization, inverse-temperature
  convergence, finite-size convergence, or ergodicity.

These constraints should remain visible in publications and result reports.
Future versions should remove or refine them only together with tests and
updated documentation.
