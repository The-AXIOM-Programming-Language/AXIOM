//! Core EEG recording types for AXIOM.
//!
//! This module models recording metadata and channel-major sample data. It does
//! not yet read EDF files or perform signal processing.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalUnit { Volts, Microvolts }

#[derive(Debug, Clone, PartialEq)]
pub struct ElectrodePosition {
    /// Cartesian coordinates in metres, in a documented head-coordinate frame.
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub coordinate_frame: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EegChannel {
    pub label: String,
    pub unit: SignalUnit,
    pub electrode: Option<ElectrodePosition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference { CommonAverage, LinkedMastoids, Channel(String), Unknown }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataProvenance {
    Recorded { source: String },
    Simulated { model: String },
    Derived { method: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EegRecording {
    pub sampling_rate_hz: f64,
    pub channels: Vec<EegChannel>,
    /// One vector per channel. None represents a missing sample, not a zero.
    pub samples: Vec<Vec<Option<f64>>>,
    pub reference: Reference,
    pub provenance: DataProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingError {
    InvalidSamplingRate,
    NoChannels,
    ChannelSampleCountMismatch { channel_index: usize, expected: usize, actual: usize },
    NoSamples,
    NonFiniteSample { channel_index: usize, sample_index: usize },
    EmptyChannelLabel { channel_index: usize },
}

impl fmt::Display for RecordingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSamplingRate => write!(f, "sampling rate must be finite and greater than zero"),
            Self::NoChannels => write!(f, "recording must contain at least one channel"),
            Self::ChannelSampleCountMismatch { channel_index, expected, actual } =>
                write!(f, "channel {channel_index} has {actual} samples; expected {expected}"),
            Self::NoSamples => write!(f, "recording must contain at least one sample"),
            Self::NonFiniteSample { channel_index, sample_index } =>
                write!(f, "channel {channel_index}, sample {sample_index} is not finite"),
            Self::EmptyChannelLabel { channel_index } =>
                write!(f, "channel {channel_index} has an empty label"),
        }
    }
}
impl std::error::Error for RecordingError {}

impl EegRecording {
    pub fn new(
        sampling_rate_hz: f64,
        channels: Vec<EegChannel>,
        samples: Vec<Vec<Option<f64>>>,
        reference: Reference,
        provenance: DataProvenance,
    ) -> Result<Self, RecordingError> {
        if !sampling_rate_hz.is_finite() || sampling_rate_hz <= 0.0 {
            return Err(RecordingError::InvalidSamplingRate);
        }
        if channels.is_empty() { return Err(RecordingError::NoChannels); }
        for (index, channel) in channels.iter().enumerate() {
            if channel.label.trim().is_empty() {
                return Err(RecordingError::EmptyChannelLabel { channel_index: index });
            }
        }
        if samples.len() != channels.len() {
            return Err(RecordingError::ChannelSampleCountMismatch {
                channel_index: samples.len(), expected: channels.len(), actual: samples.len(),
            });
        }
        let sample_count = samples.first().map_or(0, Vec::len);
        if sample_count == 0 { return Err(RecordingError::NoSamples); }
        for (channel_index, channel_samples) in samples.iter().enumerate() {
            if channel_samples.len() != sample_count {
                return Err(RecordingError::ChannelSampleCountMismatch {
                    channel_index, expected: sample_count, actual: channel_samples.len(),
                });
            }
            for (sample_index, sample) in channel_samples.iter().enumerate() {
                if sample.is_some_and(|value| !value.is_finite()) {
                    return Err(RecordingError::NonFiniteSample { channel_index, sample_index });
                }
            }
        }
        Ok(Self { sampling_rate_hz, channels, samples, reference, provenance })
    }

    pub fn sample_count(&self) -> usize {
        self.samples.first().map_or(0, Vec::len)
    }

    pub fn duration_seconds(&self) -> f64 {
        self.sample_count() as f64 / self.sampling_rate_hz
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(label: &str) -> EegChannel {
        EegChannel { label: label.into(), unit: SignalUnit::Microvolts, electrode: None }
    }

    fn recording(samples: Vec<Vec<Option<f64>>>) -> Result<EegRecording, RecordingError> {
        EegRecording::new(
            256.0, vec![channel("O1"), channel("O2")], samples,
            Reference::CommonAverage,
            DataProvenance::Recorded { source: "test fixture".into() },
        )
    }

    #[test]
    fn accepts_rectangular_multichannel_data_and_preserves_missing_samples() {
        let result = recording(vec![vec![Some(1.0), None], vec![Some(-2.0), Some(3.0)]]).unwrap();
        assert_eq!(result.sample_count(), 2);
        assert_eq!(result.samples[0][1], None);
        assert_eq!(result.duration_seconds(), 2.0 / 256.0);
    }

    #[test]
    fn rejects_invalid_sampling_rate() {
        let result = EegRecording::new(
            0.0, vec![channel("Cz")], vec![vec![Some(0.0)]],
            Reference::Unknown, DataProvenance::Simulated { model: "test".into() },
        );
        assert_eq!(result.unwrap_err(), RecordingError::InvalidSamplingRate);
    }

    #[test]
    fn rejects_ragged_channel_data() {
        let result = recording(vec![vec![Some(1.0), Some(2.0)], vec![Some(3.0)]]);
        assert!(matches!(result, Err(RecordingError::ChannelSampleCountMismatch { channel_index: 1, .. })));
    }

    #[test]
    fn rejects_non_finite_samples() {
        let result = recording(vec![vec![Some(f64::NAN)], vec![Some(0.0)]]);
        assert!(matches!(result, Err(RecordingError::NonFiniteSample { channel_index: 0, sample_index: 0 })));
    }

    #[test]
    fn rejects_empty_channel_labels() {
        let result = EegRecording::new(
            256.0, vec![channel(" ")], vec![vec![Some(0.0)]],
            Reference::Unknown, DataProvenance::Recorded { source: "fixture".into() },
        );
        assert_eq!(result.unwrap_err(), RecordingError::EmptyChannelLabel { channel_index: 0 });
    }
}
