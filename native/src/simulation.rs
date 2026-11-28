//! Native scientific simulation primitives for AXIOM.
//!
//! The numerical conventions intentionally mirror the Python reference:
//! post-step threshold detection, reset-valued voltage samples at spikes,
//! half-open stimulus windows, and optional linear event-time interpolation.

use crate::units::{Dimension, Quantity, UnitError};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct SimulationError(pub String);

impl fmt::Display for SimulationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}
impl std::error::Error for SimulationError {}

impl From<UnitError> for SimulationError {
    fn from(value: UnitError) -> Self { Self(value.to_string()) }
}

fn require_dimension(q: Quantity, expected: Dimension, name: &str) -> Result<f64, SimulationError> {
    if q.dimension != expected {
        return Err(SimulationError(format!("{name} has dimension {:?}; expected {:?}", q.dimension.exponents, expected.exponents)));
    }
    Ok(q.magnitude)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Solver { Euler, Rk2, Rk4 }

impl Solver {
    pub fn parse(name: &str) -> Result<Self, SimulationError> {
        match name.trim().to_ascii_lowercase().as_str() {
            "euler" => Ok(Self::Euler),
            "rk2" => Ok(Self::Rk2),
            "rk4" => Ok(Self::Rk4),
            other => Err(SimulationError(format!("unsupported solver '{other}'; choose euler, rk2, or rk4"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpikeTimeMethod { Grid, Linear }

#[derive(Debug, Clone, PartialEq)]
pub enum Stimulus {
    Constant { amplitude: Quantity },
    Step { amplitude: Quantity, start_time: Quantity, duration: Option<Quantity> },
    PulseTrain { amplitude: Quantity, start_time: Quantity, pulse_duration: Quantity, frequency: Quantity, num_pulses: usize },
    Combined { stimuli: Vec<Stimulus> },
}

impl Stimulus {
    pub fn constant(amplitude: Quantity) -> Self { Self::Constant { amplitude } }

    pub fn current_at_ms(&self, time_ms: f64) -> Result<f64, SimulationError> {
        let time = Quantity::new(time_ms, Dimension::TIME)?;
        self.current_at(time)
    }

    pub fn current_at(&self, time: Quantity) -> Result<f64, SimulationError> {
        let t = require_dimension(time, Dimension::TIME, "stimulus query time")?;
        match self {
            Self::Constant { amplitude } => {
                require_dimension(*amplitude, Dimension::CURRENT, "constant-current amplitude")
            }
            Self::Step { amplitude, start_time, duration } => {
                let amp = require_dimension(*amplitude, Dimension::CURRENT, "step-current amplitude")?;
                let start = require_dimension(*start_time, Dimension::TIME, "step start_time")?;
                if start < 0.0 { return Err(SimulationError("start_time cannot be negative".into())); }
                if let Some(d) = duration {
                    let width = require_dimension(*d, Dimension::TIME, "step duration")?;
                    if width < 0.0 { return Err(SimulationError("duration cannot be negative".into())); }
                    if t >= start + width { return Ok(0.0); }
                }
                Ok(if t < start { 0.0 } else { amp })
            }
            Self::PulseTrain { amplitude, start_time, pulse_duration, frequency, num_pulses } => {
                let amp = require_dimension(*amplitude, Dimension::CURRENT, "pulse amplitude")?;
                let start = require_dimension(*start_time, Dimension::TIME, "pulse start_time")?;
                let width = require_dimension(*pulse_duration, Dimension::TIME, "pulse_duration")?;
                let freq = require_dimension(*frequency, Dimension { exponents: [-1, 0, 0] }, "pulse frequency")?;
                if start < 0.0 { return Err(SimulationError("start_time cannot be negative".into())); }
                if width <= 0.0 { return Err(SimulationError("pulse_duration must be positive".into())); }
                if freq <= 0.0 { return Err(SimulationError("frequency must be positive".into())); }
                if *num_pulses == 0 { return Err(SimulationError("num_pulses must be positive".into())); }
                let period = 1.0 / freq;
                if width > period + 1e-9 {
                    return Err(SimulationError(format!("pulse_duration ({width} ms) cannot exceed pulse period ({period} ms)")));
                }
                if t < start { return Ok(0.0); }
                let elapsed = t - start;
                let index = ((elapsed / period) * 1_000_000_000.0).round() / 1_000_000_000.0;
                let pulse_index = index.floor() as usize;
                let within = elapsed - pulse_index as f64 * period;
                Ok(if pulse_index < *num_pulses && within >= 0.0 && within < width - 1e-12 { amp } else { 0.0 })
            }
            Self::Combined { stimuli } => {
                let mut total = 0.0;
                for stimulus in stimuli { total += stimulus.current_at(time)?; }
                if !total.is_finite() { return Err(SimulationError("combined stimulus produced a non-finite current".into())); }
                Ok(total)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LifParameters {
    pub membrane_time_constant: Quantity,
    pub resting_voltage: Quantity,
    pub spike_threshold: Quantity,
    pub reset_voltage: Quantity,
    pub input_resistance: Quantity,
    pub initial_voltage: Quantity,
    pub refractory_period: Quantity,
}

impl LifParameters {
    pub fn validate(&self) -> Result<(), SimulationError> {
        let tau = require_dimension(self.membrane_time_constant, Dimension::TIME, "membrane_time_constant")?;
        let rest = require_dimension(self.resting_voltage, Dimension::VOLTAGE, "resting_voltage")?;
        let threshold = require_dimension(self.spike_threshold, Dimension::VOLTAGE, "spike_threshold")?;
        let reset = require_dimension(self.reset_voltage, Dimension::VOLTAGE, "reset_voltage")?;
        let resistance = require_dimension(self.input_resistance, Dimension { exponents: [0, 1, -1] }, "input_resistance")?;
        require_dimension(self.initial_voltage, Dimension::VOLTAGE, "initial_voltage")?;
        let refractory = require_dimension(self.refractory_period, Dimension::TIME, "refractory_period")?;
        if tau <= 0.0 { return Err(SimulationError("membrane_time_constant must be positive".into())); }
        if resistance <= 0.0 { return Err(SimulationError("input_resistance must be positive".into())); }
        if threshold <= reset { return Err(SimulationError("spike_threshold must be greater than reset_voltage".into())); }
        if refractory < 0.0 { return Err(SimulationError("refractory_period cannot be negative".into())); }
        if !rest.is_finite() { return Err(SimulationError("resting_voltage must be finite".into())); }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LifResult {
    pub times_ms: Vec<f64>,
    pub membrane_voltage_mV: Vec<f64>,
    pub spike_times_ms: Vec<f64>,
    pub injected_current_nA: Vec<f64>,
}

fn derivative(v: f64, current: f64, tau: f64, rest: f64, resistance: f64) -> f64 {
    (rest - v + resistance * current) / tau
}

fn integrate(solver: Solver, v: f64, c0: f64, cm: f64, c1: f64, dt: f64, tau: f64, rest: f64, resistance: f64) -> f64 {
    let k1 = derivative(v, c0, tau, rest, resistance);
    match solver {
        Solver::Euler => v + dt * k1,
        Solver::Rk2 => {
            let midpoint = v + 0.5 * dt * k1;
            let k2 = derivative(midpoint, cm, tau, rest, resistance);
            v + dt * k2
        }
        Solver::Rk4 => {
            let v2 = v + 0.5 * dt * k1;
            let k2 = derivative(v2, cm, tau, rest, resistance);
            let v3 = v + 0.5 * dt * k2;
            let k3 = derivative(v3, cm, tau, rest, resistance);
            let v4 = v + dt * k3;
            let k4 = derivative(v4, c1, tau, rest, resistance);
            v + (dt / 6.0) * (k1 + 2.0 * k2 + 2.0 * k3 + k4)
        }
    }
}

pub fn simulate_lif(
    parameters: &LifParameters,
    stimulus: &Stimulus,
    duration: Quantity,
    time_step: Quantity,
    solver: Solver,
    spike_time_method: SpikeTimeMethod,
) -> Result<LifResult, SimulationError> {
    parameters.validate()?;
    let total = require_dimension(duration, Dimension::TIME, "duration")?;
    let dt = require_dimension(time_step, Dimension::TIME, "time_step")?;
    if total < 0.0 { return Err(SimulationError("duration cannot be negative".into())); }
    if dt <= 0.0 { return Err(SimulationError("time_step must be positive".into())); }
    let exact_steps = total / dt;
    let rounded_steps = exact_steps.round();
    if (exact_steps - rounded_steps).abs() > 1e-10_f64.max(1e-10 * exact_steps.abs()) {
        return Err(SimulationError("duration must be an integer multiple of time_step".into()));
    }
    if rounded_steps > usize::MAX as f64 {
        return Err(SimulationError("requested simulation has too many steps".into()));
    }
    let steps = rounded_steps as usize;

    let tau = parameters.membrane_time_constant.magnitude;
    let rest = parameters.resting_voltage.magnitude;
    let threshold = parameters.spike_threshold.magnitude;
    let reset = parameters.reset_voltage.magnitude;
    let resistance = parameters.input_resistance.magnitude;
    let refractory = parameters.refractory_period.magnitude;
    let mut voltage = parameters.initial_voltage.magnitude;

    let mut times = Vec::with_capacity(steps.saturating_add(1));
    let mut voltages = Vec::with_capacity(steps.saturating_add(1));
    let mut currents = Vec::with_capacity(steps.saturating_add(1));
    let mut spikes = Vec::new();
    times.push(0.0);
    voltages.push(voltage);
    currents.push(stimulus.current_at_ms(0.0)?);
    let mut last_spike_time = f64::NEG_INFINITY;

    for step in 1..=steps {
        let previous_time = (step - 1) as f64 * dt;
        let current_time = step as f64 * dt;
        let previous_voltage = voltage;
        let in_refractory = previous_time < last_spike_time + refractory;

        let current_i;
        if in_refractory {
            voltage = reset;
            current_i = stimulus.current_at_ms(current_time)?;
        } else {
            let c0 = stimulus.current_at_ms(previous_time)?;
            let cm = match solver {
                Solver::Euler => c0,
                Solver::Rk2 | Solver::Rk4 => stimulus.current_at_ms(previous_time + 0.5 * dt)?,
            };
            let c1 = if solver == Solver::Rk4 { stimulus.current_at_ms(current_time)? } else { cm };
            voltage = integrate(solver, voltage, c0, cm, c1, dt, tau, rest, resistance);
            current_i = stimulus.current_at_ms(current_time)?;
        }

        if voltage >= threshold {
            let spike_time = if spike_time_method == SpikeTimeMethod::Linear && !in_refractory {
                let delta = voltage - previous_voltage;
                if delta > 0.0 {
                    let fraction = ((threshold - previous_voltage) / delta).clamp(0.0, 1.0);
                    previous_time + fraction * dt
                } else { current_time }
            } else { current_time };
            spikes.push(spike_time);
            last_spike_time = current_time;
            voltage = reset;
        }

        if !voltage.is_finite() {
            return Err(SimulationError("simulation produced a non-finite membrane voltage".into()));
        }
        times.push(current_time);
        voltages.push(voltage);
        currents.push(current_i);
    }

    Ok(LifResult {
        times_ms: times,
        membrane_voltage_mV: voltages,
        spike_times_ms: spikes,
        injected_current_nA: currents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Unit;

    fn q(value: f64, symbol: &str) -> Quantity {
        Quantity::from_unit(value, Unit::lookup(symbol).unwrap()).unwrap()
    }
    fn params() -> LifParameters {
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

    #[test]
    fn rk4_tracks_closed_form_lif_solution() {
        let result = simulate_lif(&params(), &Stimulus::constant(q(2.0, "nA")), q(20.0, "ms"), q(0.01, "ms"), Solver::Rk4, SpikeTimeMethod::Grid).unwrap();
        let expected = -45.0 + (-20.0) * (-1.0_f64).exp();
        assert!((result.membrane_voltage_mV.last().unwrap() - expected).abs() < 1e-4);
    }

    #[test]
    fn spike_is_recorded_on_grid_and_trace_contains_reset() {
        let result = simulate_lif(&params(), &Stimulus::constant(q(2.0, "nA")), q(60.0, "ms"), q(0.1, "ms"), Solver::Rk4, SpikeTimeMethod::Grid).unwrap();
        assert!(!result.spike_times_ms.is_empty());
        assert!(result.spike_times_ms[0] >= 27.7 && result.spike_times_ms[0] <= 27.9);
        assert!(result.membrane_voltage_mV.iter().all(|v| *v < -50.0));
    }

    #[test]
    fn rejects_duration_not_divisible_by_step() {
        let result = simulate_lif(&params(), &Stimulus::constant(q(1.0, "nA")), q(10.0, "ms"), q(0.3, "ms"), Solver::Euler, SpikeTimeMethod::Grid);
        assert!(result.unwrap_err().0.contains("integer multiple"));
    }

    #[test]
    fn step_stimulus_uses_half_open_interval() {
        let stimulus = Stimulus::Step { amplitude: q(2.0, "nA"), start_time: q(5.0, "ms"), duration: Some(q(3.0, "ms")) };
        assert_eq!(stimulus.current_at_ms(4.999).unwrap(), 0.0);
        assert_eq!(stimulus.current_at_ms(5.0).unwrap(), 2.0);
        assert_eq!(stimulus.current_at_ms(7.999).unwrap(), 2.0);
        assert_eq!(stimulus.current_at_ms(8.0).unwrap(), 0.0);
    }

    #[test]
    fn ohm_ampere_product_has_voltage_dimension() {
        let qv = q(10.0, "MOhm").mul(q(2.0, "nA")).unwrap();
        assert_eq!(qv.dimension, Dimension::VOLTAGE);
        assert_eq!(qv.magnitude, 20.0);
    }
}
