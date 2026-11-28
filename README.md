# AXIOM

### Readable scientific programming for computational neuroscience.

AXIOM is an independent programming language being built for computational neuroscience and scientific computing. Its design priorities are readable scientific code, explicit units, transparent assumptions, reproducibility, and useful diagnostics.

**Implementation status:** The project is transitioning from its original Python-hosted prototype to a native Rust toolchain. The new implementation lives in `native/` and has no Python runtime or Python dependency. It currently supports a small language core: `let`, `show`, arithmetic expressions, strings, scientific unit scale names, syntax checking, and command-line execution.

The former Python prototype remains in the repository as a behavioral reference during migration. It is not required to build or run the native toolchain.

## Native toolchain

The language front end, neuroscience interpreter, and command-line interface are implemented in Rust. The native runtime executes the original neuron-block, current-injection, simulation, display, and CSV-export program shape directly; machine-code generation remains a later milestone.

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
cargo run -- check examples/hello.ax\ncargo run -- check ../examples/first_neuron.ax\ncargo run -- run ../examples/first_neuron.ax\ncargo run -- run ../examples/pulse_train_neuron.ax
cargo run -- --version\ncargo run -- help\ncargo run -- doctor\ncargo run -- new my_experiment
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
- A stable CLI, package/build workflow, and optional native machine-code backend. The current `axiom build` command performs source validation and explicitly reports that native code generation is not yet available; it does not pretend to produce a binary.

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
