# Physical conventions

This page defines conventions shared by all built-in models. Model-specific
Hamiltonians are stated on their dedicated pages.

## Units

The implementation uses dimensionless units with Boltzmann's constant
\(k_B=1\). Inverse temperature is therefore measured in inverse energy units:

\[
\beta = \frac{1}{T}.
\]

The user is responsible for expressing all Hamiltonian coefficients and
\(\beta\) in a consistent system of units.

## Spin basis

The sampled basis is the Pauli-z basis:

\[
\sigma^z |\uparrow\rangle = +|\uparrow\rangle,
\qquad
\sigma^z |\downarrow\rangle = -|\downarrow\rangle.
\]

In the Rust API these states are `Spin::Up` and `Spin::Down`. Rydberg models
interpret them as occupation values

\[
n_i = \frac{1+\sigma_i^z}{2},
\qquad
|\downarrow\rangle \mapsto n_i=0,
\qquad
|\uparrow\rangle \mapsto n_i=1.
\]

## Site indexing

All sites have contiguous unsigned integer identifiers. A chain uses increasing
site IDs along the x direction. A rectangular lattice has unit spacing and
row-major indexing:

\[
i = x + L_x y.
\]

Two-dimensional coordinates can be recovered as
\(x=i\bmod L_x\) and \(y=\lfloor i/L_x\rfloor\).

## Boundary conditions and distances

Chains support open or periodic boundaries. Rectangular lattices independently
select open or periodic boundaries along x and y. Periodic directions use the
minimum-image displacement. For direction length \(L\), a displacement is
mapped approximately into \([-L/2,L/2]\).

Custom coordinates are currently open. They use direct Euclidean distances and
do not define a periodic simulation cell.

## Pair selection

Pairs are unordered and represented once. Built-in geometric shells use
squared distance:

| Selection | Squared distance on a unit rectangular lattice |
| --- | ---: |
| Nearest neighbour | 1 |
| Diagonal next-nearest neighbour | 2 |
| Axial next-next-nearest neighbour | 4 |

Tiny periodic lattices can identify the same physical neighbour through more
than one displacement, but the current lattice representation stores each
unordered site pair only once. The model therefore represents a simple graph,
not a multigraph with bond multiplicity.

## Energy-shift convention

The sign-safe local model writes the physical Hamiltonian as

\[
H = E_{\mathrm{shift}} - \sum_a B_a,
\]

where the matrix elements sampled from each \(B_a\) are non-negative. The
reported energy restores \(E_{\mathrm{shift}}\). It is not an arbitrary change
to the physical Hamiltonian.
