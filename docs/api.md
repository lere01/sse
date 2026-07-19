# Rust API reference

The generated Rust API documentation describes types, methods, invariants, and
error conditions for developers integrating the numerical engine.

[Open the Rust API reference](https://lere01.github.io/sse/api/sse/).

To build the same reference locally, run:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

Then open `target/doc/sse/index.html`. The generated `target/` directory is not
committed to the repository. GitHub Pages rebuilds it from the source on every
deployment.
