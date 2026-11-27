# AXIOM

### Readable scientific programming for computational neuroscience.

AXIOM is an independent programming language being built for computational neuroscience and scientific computing. Its design priorities are readable scientific code, explicit units, transparent assumptions, reproducibility, and useful diagnostics.

**Implementation status:** The project is transitioning from its original Python-hosted prototype to a native Rust toolchain. The new implementation lives in `native/` and has no Python runtime or Python dependency. It currently supports a small language core: `let`, `show`, arithmetic expressions, strings, scientific unit scale names, syntax checking, and command-line execution.

The former Python prototype remains in the repository as historical/reference implementation during the migration. It is not the implementation of the new native toolchain.

## Native toolchain

The language front end and command-line interpreter are implemented in Rust. This first milestone executes parsed AXIOM programs directly; machine-code generation is a later milestone.

### Requirements

- Rust stable toolchain (Cargo)

### Build and test

From the `native/` directory:

```sh
cargo test
cargo build --release
```

### Run a program

```sh
cargo run -- run examples/hello.ax
cargo run -- check examples/hello.ax
cargo run -- --version
```

After building, the executable is located at `native/target/release/axiom` (Windows: `axiom.exe`).

## First native program

Save as `hello.ax`:

```axiom
let duration = 20 ms
let doubled = duration * 2

show "AXIOM is running"
show doubled
```

## Language direction

AXIOM is intended to grow into a typed scientific language with:

- A formally specified grammar and source-aware diagnostics.
- Dimensional quantities and compile-time unit checking.
- A typed intermediate representation, bytecode compiler, and deterministic runtime.
- Scientific numerical solvers and reproducible simulation.
- Neuroscience primitives, beginning with leaky integrate-and-fire models.
- Stimulus definitions, recording, and structured data export.
- A stable CLI, package/build workflow, and optional native machine-code backend.

These are roadmap goals, not claims about features already implemented. See [the native implementation notes](docs/NATIVE_IMPLEMENTATION.md) for the current implementation boundary.

## Repository layout

```text
native/       Rust compiler and command-line tool
docs/         Language and implementation documentation
src/          Legacy Python prototype (migration reference only)
tests/        Legacy prototype tests
cpp/          Legacy optional numerical backend
examples/     Original prototype examples
```

## License

MIT
