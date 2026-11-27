# Native AXIOM implementation

AXIOM is transitioning from a Python-hosted DSL to an independently implemented language toolchain.

## Current implementation boundary

The `native/` directory contains the Rust implementation. It has no Python runtime, Python package dependency, or third-party Rust dependency.

The first vertical slice provides:

- A source lexer with line-aware lexical errors.
- A parser for `let` declarations and `show` statements.
- Arithmetic expressions with precedence and parentheses.
- Numeric and string values.
- Built-in scientific unit scale names.
- A command-line interface with `run`, `check`, and `--version`.
- Unit tests for arithmetic, precedence, strings, unknown names, and division by zero.

## Build and run

Install the Rust stable toolchain, then from this directory:

```sh
cargo test
cargo run -- run examples/hello.ax
cargo run -- check examples/hello.ax
cargo build --release
```

The executable is `target/release/axiom` (or `axiom.exe` on Windows).

## Language status

This is the initial native implementation, not feature parity with the former Python prototype. At this stage, unit names are numeric scale constants; dimensional type checking is not implemented. Neuron declarations, stimuli, simulation, recording, and CSV export remain future compiler/runtime work.

## Roadmap

1. Stabilize the lexical and syntactic grammar.
2. Add source spans and structured diagnostics.
3. Add a typed value system with dimensional quantities.
4. Lower the AST into a typed AXIOM intermediate representation and bytecode.
5. Implement a deterministic virtual machine.
6. Port the LIF model, stimuli, solvers, and recording into the native runtime.
7. Add an optional native machine-code backend after the language semantics and VM are stable, with reproducible numerical regression tests.
