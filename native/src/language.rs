//! Source-level interpreter for AXIOM's neuroscience statements.
//! This module deliberately implements the documented prototype syntax without
//! delegating execution to Python.

use crate::simulation::{simulate_lif, LifParameters, Solver, SpikeTimeMethod, Stimulus};
use crate::units::{Dimension, Quantity, Unit};
use std::{collections::HashMap, fs, path::Path};

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub line: usize,
    pub message: String,
}
impl RuntimeError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self { line, message: message.into() }
    }
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)
    }
}
impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone)]
enum Value { Number(f64), Quantity(Quantity), Text(String), NumberList(Vec<f64>) }

fn eval_atom(expr: &str, vars: &HashMap<String, Value>, line: usize) -> Result<Value, RuntimeError> {
    let e = expr.trim();
    if e.is_empty() { return Err(RuntimeError::new(line, "expected an expression")); }
    if (e.starts_with('"') && e.ends_with('"')) || (e.starts_with('\'') && e.ends_with('\'')) {
        return Ok(Value::Text(e[1..e.len()-1].to_string()));
    }
    if let Some(v) = vars.get(e) { return Ok(v.clone()); }
    let pieces: Vec<&str> = e.split_whitespace().collect();
    if pieces.len() == 2 {
        if let (Ok(n), Some(unit)) = (pieces[0].parse::<f64>(), Unit::lookup(pieces[1])) {
            return Quantity::from_unit(n, unit).map(Value::Quantity)
                .map_err(|err| RuntimeError::new(line, err.to_string()));
        }
    }
    if let Ok(n) = e.parse::<f64>() {
        return Ok(Value::Number(n));
    }
    Err(RuntimeError::new(line, format!("unsupported expression '{e}' (expected a number, quantity, string, or variable)")))
}

fn as_quantity(value: Value, default_unit: &str, line: usize, label: &str) -> Result<Quantity, RuntimeError> {
    match value {
        Value::Quantity(q) => Ok(q),
        Value::Number(n) => {
            let unit = Unit::lookup(default_unit).ok_or_else(|| RuntimeError::new(line, "internal default unit error"))?;
            Quantity::from_unit(n, unit).map_err(|e| RuntimeError::new(line, e.to_string()))
        }
        _ => Err(RuntimeError::new(line, format!("{label} must be a numeric quantity"))),
    }
}

fn default_parameters() -> LifParameters {
    let q = |v, u| Quantity::from_unit(v, Unit::lookup(u).unwrap()).unwrap();
    LifParameters {
        membrane_time_constant: q(20.0, "ms"),
        resting_voltage: q(-65.0, "mV"),
        spike_threshold: q(-50.0, "mV"),
        reset_voltage: q(-65.0, "mV"),
        input_resistance: q(10.0, "MOhm"),
        initial_voltage: q(-65.0, "mV"),
        refractory_period: q(0.0, "ms"),
    }
}

fn set_parameter(params: &mut LifParameters, key: &str, value: Quantity, line: usize) -> Result<(), RuntimeError> {
    let (target, expected) = match key {
        "membrane_time" | "membrane_time_constant" | "tau" => (&mut params.membrane_time_constant, Dimension::TIME),
        "resting_voltage" | "rest_voltage" | "E_rest" => (&mut params.resting_voltage, Dimension::VOLTAGE),
        "spike_threshold" | "threshold" | "V_th" => (&mut params.spike_threshold, Dimension::VOLTAGE),
        "reset_voltage" | "reset" | "V_reset" => (&mut params.reset_voltage, Dimension::VOLTAGE),
        "input_resistance" | "resistance" | "R_m" => (&mut params.input_resistance, Dimension { exponents: [0,1,-1] }),
        "initial_voltage" | "initial" | "V_0" | "v0" => (&mut params.initial_voltage, Dimension::VOLTAGE),
        "refractory_period" | "t_ref" => (&mut params.refractory_period, Dimension::TIME),
        _ => return Err(RuntimeError::new(line, format!("unknown neuron parameter '{key}'"))),
    };
    if value.dimension == Dimension::DIMENSIONLESS {
        *target = Quantity::new(value.magnitude, expected).map_err(|e| RuntimeError::new(line, e.to_string()))?;
    } else if value.dimension == expected {
        *target = value;
    } else {
        return Err(RuntimeError::new(line, format!("parameter '{key}' has incompatible physical units")));
    }
    Ok(())
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else { value.to_string() }
}

pub fn check_source(source: &str) -> Result<(), RuntimeError> {
    let lines: Vec<&str> = source.lines().collect();
    let mut in_neuron = false;
    let mut has_parameter = false;
    for (i, raw) in lines.iter().enumerate() {
        let line_no = i + 1;
        let clean = raw.split('#').next().unwrap_or("").trim();
        if clean.is_empty() { continue; }
        if clean.starts_with("neuron ") {
            if !clean.ends_with(':') { return Err(RuntimeError::new(line_no, "neuron declaration must end with ':'")); }
            in_neuron = true; has_parameter = false; continue;
        }
        if in_neuron && (raw.starts_with(' ') || raw.starts_with('\t')) {
            let Some((key, value)) = clean.split_once('=') else {
                return Err(RuntimeError::new(line_no, "neuron properties use 'name = value'"));
            };
            if key.trim().is_empty() || value.trim().is_empty() { return Err(RuntimeError::new(line_no, "invalid neuron parameter assignment")); }
            has_parameter = true;
            continue;
        }
        in_neuron = false;
        if clean.starts_with("let ") && !clean.contains('=') { return Err(RuntimeError::new(line_no, "assignment must use: let name = expression")); }
        if clean.starts_with("show ") || clean.starts_with("let ") || clean.starts_with("inject ") ||
           clean.starts_with("simulate ") || clean == "simulate" || clean.starts_with("save results to ") {
            continue;
        }
        return Err(RuntimeError::new(line_no, format!("unknown statement '{clean}'")));
    }
    let _ = has_parameter;
    Ok(())
}

pub fn run_source(source: &str) -> Result<Vec<String>, RuntimeError> {
    check_source(source)?;
    let lines: Vec<&str> = source.lines().collect();
    let mut vars: HashMap<String, Value> = HashMap::new();
    let mut params = default_parameters();
    let mut stimulus = Stimulus::constant(Quantity::from_unit(0.0, Unit::lookup("nA").unwrap()).unwrap());
    let mut last_result = None;
    let mut output = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let line_no = i + 1;
        let raw = lines[i];
        let clean = raw.split('#').next().unwrap_or("").trim();
        if clean.is_empty() { i += 1; continue; }

        if clean.starts_with("neuron ") {
            let mut local = default_parameters();
            i += 1;
            while i < lines.len() && (lines[i].starts_with(' ') || lines[i].starts_with('\t') || lines[i].trim().is_empty()) {
                let property = lines[i].split('#').next().unwrap_or("").trim();
                if !property.is_empty() {
                    let (key, expr) = property.split_once('=').ok_or_else(|| RuntimeError::new(i+1, "expected key = value"))?;
                    let value = as_quantity(eval_atom(expr, &vars, i+1)?, "mV", i+1, key.trim())?;
                    set_parameter(&mut local, key.trim(), value, i+1)?;
                }
                i += 1;
            }
            local.validate().map_err(|e| RuntimeError::new(line_no, e.to_string()))?;
            params = local;
            continue;
        }

        if let Some(rest) = clean.strip_prefix("let ") {
            let (name, expr) = rest.split_once('=').ok_or_else(|| RuntimeError::new(line_no, "assignment must use 'let name = expression'"))?;
            let name = name.trim();
            if name.is_empty() || !name.chars().enumerate().all(|(n,c)| c == '_' || c.is_ascii_alphanumeric() && (n > 0 || !c.is_ascii_digit())) {
                return Err(RuntimeError::new(line_no, "invalid variable name"));
            }
            if ["let","show","neuron","inject","simulate","save"].contains(&name) { return Err(RuntimeError::new(line_no, format!("'{name}' is reserved"))); }
            vars.insert(name.to_string(), eval_atom(expr, &vars, line_no)?);
        } else if let Some(expr) = clean.strip_prefix("show ") {
            let value = if expr.trim() == "spike_count" {
                Value::Number(last_result.as_ref().map(|r: &crate::simulation::LifResult| r.spike_times_ms.len()).unwrap_or(0) as f64)
            } else if expr.trim() == "spike_times" {
                Value::NumberList(last_result.as_ref().map(|r: &crate::simulation::LifResult| r.spike_times_ms.clone()).unwrap_or_default())
            } else if expr.trim() == "membrane_voltage" {
                Value::NumberList(last_result.as_ref().map(|r: &crate::simulation::LifResult| r.membrane_voltage_mV.clone()).unwrap_or_default())
            } else { eval_atom(expr, &vars, line_no)? };
            match value {
                Value::Number(n) => output.push(n.to_string()),
                Value::Quantity(q) => output.push(q.magnitude.to_string()),
                Value::Text(s) => output.push(s),
                Value::NumberList(xs) => output.push(format!("[{}]", xs.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))),
            }
        } else if let Some(rest) = clean.strip_prefix("inject ") {
            if let Some(expr) = rest.strip_prefix("current ") {
                let amp = as_quantity(eval_atom(expr, &vars, line_no)?, "nA", line_no, "current")?;
                stimulus = Stimulus::constant(amp);
            } else if let Some(expr) = rest.strip_prefix("step current ") {
                let (amplitude, tail) = expr.split_once(" from ").ok_or_else(|| RuntimeError::new(line_no, "step syntax: inject step current <amplitude> from <start> for <duration>"))?;
                let (start, duration) = tail.split_once(" for ").ok_or_else(|| RuntimeError::new(line_no, "step syntax requires 'for <duration>'"))?;
                stimulus = Stimulus::Step {
                    amplitude: as_quantity(eval_atom(amplitude, &vars, line_no)?, "nA", line_no, "amplitude")?,
                    start_time: as_quantity(eval_atom(start, &vars, line_no)?, "ms", line_no, "start time")?,
                    duration: Some(as_quantity(eval_atom(duration, &vars, line_no)?, "ms", line_no, "duration")?),
                };
            } else if let Some(expr) = rest.strip_prefix("pulse train current ") {
                let (amplitude, tail) = expr.split_once(" starting at ").ok_or_else(|| RuntimeError::new(line_no, "pulse train syntax requires 'starting at'"))?;
                let (start, tail) = tail.split_once(" pulse_duration ").ok_or_else(|| RuntimeError::new(line_no, "pulse train syntax requires 'pulse_duration'"))?;
                let (width, tail) = tail.split_once(" frequency ").ok_or_else(|| RuntimeError::new(line_no, "pulse train syntax requires 'frequency'"))?;
                let (frequency, count) = tail.split_once(" pulses ").ok_or_else(|| RuntimeError::new(line_no, "pulse train syntax requires 'pulses'"))?;
                let count_value = eval_atom(count, &vars, line_no)?;
                let count_number = match count_value {
                    Value::Number(n) => n,
                    Value::Quantity(q) if q.dimension == Dimension::DIMENSIONLESS => q.magnitude,
                    _ => return Err(RuntimeError::new(line_no, "pulse count must be an integer")),
                };
                if count_number < 1.0 || count_number.fract() != 0.0 || count_number > usize::MAX as f64 {
                    return Err(RuntimeError::new(line_no, "pulse count must be a positive integer"));
                }
                stimulus = Stimulus::PulseTrain {
                    amplitude: as_quantity(eval_atom(amplitude, &vars, line_no)?, "nA", line_no, "amplitude")?,
                    start_time: as_quantity(eval_atom(start, &vars, line_no)?, "ms", line_no, "start time")?,
                    pulse_duration: as_quantity(eval_atom(width, &vars, line_no)?, "ms", line_no, "pulse duration")?,
                    frequency: as_quantity(eval_atom(frequency, &vars, line_no)?, "Hz", line_no, "frequency")?,
                    num_pulses: count_number as usize,
                };
            } else {
                return Err(RuntimeError::new(line_no, "supported forms: inject current ..., inject step current ..., inject pulse train current ..."));
            }
        } else if let Some(rest) = clean.strip_prefix("simulate") {
            let mut duration = Quantity::from_unit(100.0, Unit::lookup("ms").unwrap()).unwrap();
            let mut dt = Quantity::from_unit(0.1, Unit::lookup("ms").unwrap()).unwrap();
            let mut solver = Solver::Euler;
            let clauses: Vec<&str> = rest.trim().split(" using ").collect();
            if let Some(first) = clauses.first() {
                if let Some(expr) = first.trim().strip_prefix("for ") {
                    duration = as_quantity(eval_atom(expr, &vars, line_no)?, "ms", line_no, "duration")?;
                } else if !first.trim().is_empty() {
                    return Err(RuntimeError::new(line_no, "simulate syntax: simulate for <duration> using time_step <step>"));
                }
            }
            for clause in clauses.iter().skip(1) {
                let c = clause.trim();
                if let Some(expr) = c.strip_prefix("time_step ") {
                    dt = as_quantity(eval_atom(expr, &vars, line_no)?, "ms", line_no, "time_step")?;
                } else if let Some(name) = c.strip_prefix("solver ") {
                    solver = Solver::parse(name).map_err(|e| RuntimeError::new(line_no, e.to_string()))?;
                } else {
                    return Err(RuntimeError::new(line_no, format!("unknown simulate option '{c}'")));
                }
            }
            let result = simulate_lif(&params, &stimulus, duration, dt, solver, SpikeTimeMethod::Grid)
                .map_err(|e| RuntimeError::new(line_no, e.to_string()))?;
            last_result = Some(result);
        } else if let Some(path_expr) = clean.strip_prefix("save results to ") {
            let result = last_result.as_ref().ok_or_else(|| RuntimeError::new(line_no, "no simulation result exists; run simulate first"))?;
            let path_value = eval_atom(path_expr, &vars, line_no)?;
            let path = match path_value { Value::Text(s) => s, _ => return Err(RuntimeError::new(line_no, "save path must be a quoted string")) };
            let mut csv = String::from("time_ms,membrane_voltage_mV,injected_current_nA\n");
            for idx in 0..result.times_ms.len() {
                csv.push_str(&format!("{},{},{}\n", result.times_ms[idx], result.membrane_voltage_mV[idx], result.injected_current_nA[idx]));
            }
            let spikes_path = Path::new(&path);
            fs::write(spikes_path, csv).map_err(|e| RuntimeError::new(line_no, format!("cannot write '{}': {e}", path)))?;
        }
        i += 1;
    }
    Ok(output)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_the_original_first_neuron_program_shape() {
        let source = r#"neuron "Example Neuron":
    membrane_time = 20 ms
    resting_voltage = -65 mV
    spike_threshold = -50 mV
    reset_voltage = -65 mV
    input_resistance = 10 MOhm
    initial_voltage = -65 mV

inject current 2 nA
simulate for 100 ms using time_step 0.1 ms using solver rk4
show spike_count
show spike_times
"#;
        let output = run_source(source).unwrap();
        assert_eq!(output.len(), 2);
        assert!(output[0].parse::<usize>().unwrap() > 0);
        assert!(output[1].starts_with('['));
    }

    #[test]
    fn accepts_pulse_train_stimulus() {
        let source = "inject pulse train current 2 nA starting at 0 ms pulse_duration 1 ms frequency 10 Hz pulses 5\nsimulate for 500 ms using time_step 0.1 ms using solver rk4\nshow spike_count\n";
        assert!(run_source(source).is_ok());
    }

    #[test]
    fn rejects_unknown_neuron_parameter() {
        let source = "neuron \"x\":\n    magic_parameter = 1 ms\n";
        assert!(run_source(source).unwrap_err().message.contains("unknown neuron parameter"));
    }
}
