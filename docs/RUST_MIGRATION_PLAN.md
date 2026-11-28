# AXIOM Rust-Native Migration Plan

## Objective

Make Rust the implementation language for the AXIOM language toolchain and its scientific runtime while preserving the documented `.ax` language and its computational-neuroscience purpose.

This plan distinguishes existing implementation from planned work. A feature is not considered migrated merely because a Rust data type or module exists; it must be connected to the language/runtime where appropriate and covered by tests.

## Repository audit baseline

The migration branch starts from the repository's `main` branch.

### Existing Rust implementation

- `native/src/main.rs`: a small lexer, parser, expression evaluator, and CLI in one file.
- `native/src/eeg.rs`: EEG channel, segment, annotation, and recording domain types.
- `native/src/edf.rs` and `native/src/edf_data.rs`: EDF header/data decoding and EDF+ annotation parsing.
- `native/Cargo.toml`: currently has no third-party Rust dependencies.

### Legacy Python reference implementation

- `src/axiom/interpreter.py`: AXIOM statement execution and expression evaluation.
- `src/axiom/units.py`: dimensional quantities and unit conversion.
- `src/axiom/neuroscience.py`: current-driven leaky integrate-and-fire (LIF) model and simulation orchestration.
- `src/axiom/solvers.py`: Forward Euler, midpoint RK2, and RK4 steps.
- `src/axiom/stimuli.py`: constant, step, pulse-train, and combined current stimuli.
- `src/axiom/recording.py`: simulation results and CSV export.
- `src/axiom/native.py` and `cpp/`: optional C++ LIF integration backend and its Python bridge.

The Python implementation remains a behavioral reference during migration. It should not remain a runtime dependency of the completed Rust toolchain.

## Target architecture

Organize the Rust crate into explicit layers:

1. **Source layer** — source files, spans, line/column mapping.
2. **Lexer** — tokens with source spans and structured lexical errors.
3. **Parser** — AST for declarations, expressions, neuron blocks, stimuli, simulation, output, and save statements.
4. **Semantic analysis** — name resolution, reserved words, type checking, dimensional analysis, and validation of model/stimulus parameters.
5. **Typed IR** — a stable representation between the language frontend and execution backend.
6. **Runtime/VM** — deterministic evaluation and simulation orchestration.
7. **Scientific domain library** — units, LIF models, solvers, stimuli, recordings, EEG/EDF data.
8. **CLI** — `run`, `check`, `inspect`, and version reporting.

Keep the language frontend independent from the scientific runtime APIs. The CLI should call library functions rather than contain compiler logic.

## Migration phases

### Phase 0 — Baseline and preservation

- Record the current Python behavior and documented language grammar.
- Identify mismatches between the language specification, examples, Python interpreter, and current Rust parser.
- Preserve the Python implementation as a temporary reference.
- Add a migration test matrix for every currently supported language construct and scientific feature.

### Phase 1 — Rust crate structure and language frontend

- Split the monolithic `main.rs` into library modules.
- Introduce source spans and structured diagnostics.
- Define and test the lexer and parser against the documented `.ax` grammar.
- Preserve existing simple programs (`let`, `show`, arithmetic, strings, comments, and unit literals).
- Add AST nodes for neuron definitions, current injection, simulation, and CSV saving.

### Phase 2 — Dimensional quantities and semantic analysis

- Port the Python unit model into Rust with explicit dimension exponents and scales.
- Preserve conversions and reject dimensionally invalid operations.
- Check addition/subtraction compatibility and multiplication/division dimension composition.
- Add compile/check-time validation where possible; keep runtime checks for values that cannot be known statically.
- Do not model units as bare numeric scale constants.

### Phase 3 — Scientific runtime

- Port simulation result/recording structures and CSV output.
- Port constant, step, pulse-train, and combined stimuli.
- Port Euler, midpoint RK2, and RK4 integration.
- Port LIF parameter validation, threshold handling, reset, refractory behavior, and spike-time conventions.
- Compare Rust outputs with the Python reference on fixed, documented test cases.

### Phase 4 — Language/runtime integration

- Connect parsed neuron, stimulus, simulation, and save statements to the Rust runtime.
- Expose simulation outputs such as membrane voltage, spike times, and spike count to AXIOM expressions.
- Implement `run`, `check`, and `inspect` with consistent diagnostics and exit codes.
- Add executable examples for every supported construct.

### Phase 5 — EEG/EDF integration

- Audit the existing EEG and EDF modules independently.
- Preserve raw digital samples, calibrated physical samples, channel metadata, discontinuous segment timing, and annotations.
- Add end-to-end tests from EDF decoding into AXIOM EEG recording types.
- Decide which operations are library APIs and which, if any, belong in the `.ax` language.

### Phase 6 — Compiler backend and release hardening

- Stabilize language semantics before selecting a machine-code backend.
- Introduce bytecode/VM execution as the initial portable backend.
- Consider native code generation only after typed IR and runtime behavior are stable.
- Add regression, property-based, malformed-input, and numerical tests.
- Document supported features, limitations, reproducible build steps, and release compatibility.

## Correctness and migration rules

- The Python implementation is a reference, not an unquestionable specification; discrepancies must be documented and resolved deliberately.
- Never silently change numerical conventions such as integration step timing, stimulus boundary behavior, threshold crossing, reset timing, or refractory intervals.
- Every ported numerical feature needs fixed test cases and comparisons against analytical results or the Python reference where appropriate.
- No performance, scientific-validity, or feature-completeness claims should be made without corresponding measurements or tests.
- Do not delete the Python reference or C++ backend until replacement coverage and a migration decision are documented.
- Keep the `.ax` language independent: Rust is the implementation language, not the language users write.
