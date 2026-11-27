//! Core EEG domain types for AXIOM.
//!
//! Samples are stored in time-ordered segments. Each channel has its own
//! sampling rate, and segment onset times preserve gaps in discontinuous data.
//! This module does not perform file I/O, filtering, resampling, or localization.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplingRate { hz: f64 }

impl SamplingRate {
    pub fn new(hz: f64) -> Result<Self, EegError> {
        if !hz.is_finite() || hz <= 0.0 { return Err(EegError::InvalidSamplingRate(hz)); }
        Ok(Self { hz })
    }
    pub fn hz(self) -> f64 { self.hz }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoltageUnit { Volt, Millivolt, Microvolt }

impl VoltageUnit {
    pub fn to_volts_scale(self) -> f64 {
        match self { Self::Volt => 1.0, Self::Millivolt => 1e-3, Self::Microvolt => 1e-6 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelReference {
    Unknown,
    CommonAverage,
    LinkedMastoids,
    Electrode(String),
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Electrode { pub label: String }

impl Electrode {
    pub fn new(label: impl Into<String>) -> Result<Self, EegError> {
        let label = label.into();
        if label.trim().is_empty() { return Err(EegError::EmptyElectrodeLabel); }
        Ok(Self { label })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EegChannel {
    pub label: String,
    pub unit: VoltageUnit,
    pub electrode: Option<Electrode>,
    pub reference: ChannelReference,
    pub sampling_rate: SamplingRate,
}

impl EegChannel {
    pub fn new(
        label: impl Into<String>,
        unit: VoltageUnit,
        electrode: Option<Electrode>,
        reference: ChannelReference,
        sampling_rate: SamplingRate,
    ) -> Result<Self, EegError> {
        let label = label.into();
        if label.trim().is_empty() { return Err(EegError::EmptyChannelLabel); }
        Ok(Self { label, unit, electrode, reference, sampling_rate })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataOrigin { Recorded, Simulated, Derived }

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub onset_seconds: f64,
    pub duration_seconds: f64,
    pub label: String,
}

impl Annotation {
    pub fn new(onset_seconds: f64, duration_seconds: f64, label: impl Into<String>) -> Result<Self, EegError> {
        let label = label.into();
        if !onset_seconds.is_finite() || onset_seconds < 0.0 {
            return Err(EegError::InvalidAnnotationOnset(onset_seconds));
        }
        if !duration_seconds.is_finite() || duration_seconds < 0.0 {
            return Err(EegError::InvalidAnnotationDuration(duration_seconds));
        }
        if label.trim().is_empty() { return Err(EegError::EmptyAnnotationLabel); }
        Ok(Self { onset_seconds, duration_seconds, label })
    }
}

/// A contiguous interval in the recording timeline.
///
/// There is one sample vector per channel. The vectors may have different
/// lengths because channels may have different sampling rates, but each
/// vector must span this segment's declared duration.
#[derive(Debug, Clone, PartialEq)]
pub struct EegSegment {
    pub onset_seconds: f64,
    pub duration_seconds: f64,
    pub samples_by_channel: Vec<Vec<f64>>,
}

impl EegSegment {
    pub fn new(
        onset_seconds: f64,
        duration_seconds: f64,
        samples_by_channel: Vec<Vec<f64>>,
    ) -> Result<Self, EegError> {
        if !onset_seconds.is_finite() || onset_seconds < 0.0 {
            return Err(EegError::InvalidSegmentOnset(onset_seconds));
        }
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            return Err(EegError::InvalidSegmentDuration(duration_seconds));
        }
        if samples_by_channel.is_empty() { return Err(EegError::NoChannelData); }
        if samples_by_channel.iter().any(Vec::is_empty) { return Err(EegError::EmptySegmentChannel); }
        Ok(Self { onset_seconds, duration_seconds, samples_by_channel })
    }
    pub fn end_seconds(&self) -> f64 { self.onset_seconds + self.duration_seconds }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EegRecording {
    pub channels: Vec<EegChannel>,
    pub segments: Vec<EegSegment>,
    pub annotations: Vec<Annotation>,
    pub origin: DataOrigin,
}

impl EegRecording {
    pub fn new(
        channels: Vec<EegChannel>,
        segments: Vec<EegSegment>,
        annotations: Vec<Annotation>,
        origin: DataOrigin,
    ) -> Result<Self, EegError> {
        if channels.is_empty() { return Err(EegError::NoChannels); }
        if segments.is_empty() { return Err(EegError::NoSegments); }

        let mut previous_end = 0.0;
        for (segment_index, segment) in segments.iter().enumerate() {
            if segment.samples_by_channel.len() != channels.len() {
                return Err(EegError::ChannelDataCountMismatch {
                    segment_index,
                    channels: channels.len(),
                    data_vectors: segment.samples_by_channel.len(),
                });
            }
            if segment.onset_seconds < previous_end {
                return Err(EegError::OverlappingOrUnorderedSegments {
                    segment_index,
                    previous_end_seconds: previous_end,
                    onset_seconds: segment.onset_seconds,
                });
            }
            for (channel_index, samples) in segment.samples_by_channel.iter().enumerate() {
                let actual_duration = samples.len() as f64 / channels[channel_index].sampling_rate.hz();
                let tolerance = 1e-9_f64.max(segment.duration_seconds.abs() * 1e-9);
                if (actual_duration - segment.duration_seconds).abs() > tolerance {
                    return Err(EegError::SegmentSampleDurationMismatch {
                        segment_index, channel_index,
                        expected_seconds: segment.duration_seconds,
                        actual_seconds: actual_duration,
                    });
                }
            }
            previous_end = segment.end_seconds();
        }

        let recording_end = previous_end;
        for annotation in &annotations {
            let end = annotation.onset_seconds + annotation.duration_seconds;
            if end > recording_end {
                return Err(EegError::AnnotationOutsideRecording {
                    end_seconds: end,
                    recording_end_seconds: recording_end,
                });
            }
        }

        Ok(Self { channels, segments, annotations, origin })
    }

    /// Timeline duration from time zero to the end of the final segment.
    /// This includes any gaps between segments.
    pub fn duration_seconds(&self) -> f64 {
        self.segments.last().map_or(0.0, EegSegment::end_seconds)
    }

    pub fn segment_count(&self) -> usize { self.segments.len() }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EegError {
    InvalidSamplingRate(f64),
    EmptyChannelLabel,
    EmptyElectrodeLabel,
    EmptyAnnotationLabel,
    InvalidAnnotationOnset(f64),
    InvalidAnnotationDuration(f64),
    InvalidSegmentOnset(f64),
    InvalidSegmentDuration(f64),
    NoChannels,
    NoSegments,
    NoChannelData,
    EmptySegmentChannel,
    ChannelDataCountMismatch { segment_index: usize, channels: usize, data_vectors: usize },
    OverlappingOrUnorderedSegments { segment_index: usize, previous_end_seconds: f64, onset_seconds: f64 },
    SegmentSampleDurationMismatch { segment_index: usize, channel_index: usize, expected_seconds: f64, actual_seconds: f64 },
    AnnotationOutsideRecording { end_seconds: f64, recording_end_seconds: f64 },
}

impl std::fmt::Display for EegError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSamplingRate(v) => write!(f, "sampling rate must be finite and positive, got {v}"),
            Self::EmptyChannelLabel => write!(f, "channel label cannot be empty"),
            Self::EmptyElectrodeLabel => write!(f, "electrode label cannot be empty"),
            Self::EmptyAnnotationLabel => write!(f, "annotation label cannot be empty"),
            Self::InvalidAnnotationOnset(v) => write!(f, "annotation onset must be finite and non-negative, got {v}"),
            Self::InvalidAnnotationDuration(v) => write!(f, "annotation duration must be finite and non-negative, got {v}"),
            Self::InvalidSegmentOnset(v) => write!(f, "segment onset must be finite and non-negative, got {v}"),
            Self::InvalidSegmentDuration(v) => write!(f, "segment duration must be finite and positive, got {v}"),
            Self::NoChannels => write!(f, "an EEG recording must contain at least one channel"),
            Self::NoSegments => write!(f, "an EEG recording must contain at least one segment"),
            Self::NoChannelData => write!(f, "a segment must contain channel data"),
            Self::EmptySegmentChannel => write!(f, "a segment cannot contain an empty channel sample vector"),
            Self::ChannelDataCountMismatch { segment_index, channels, data_vectors } => write!(f, "segment {segment_index}: {channels} channels declared but {data_vectors} sample vectors supplied"),
            Self::OverlappingOrUnorderedSegments { segment_index, previous_end_seconds, onset_seconds } => write!(f, "segment {segment_index} starts at {onset_seconds}s before the previous segment ends at {previous_end_seconds}s"),
            Self::SegmentSampleDurationMismatch { segment_index, channel_index, expected_seconds, actual_seconds } => write!(f, "segment {segment_index}, channel {channel_index}: samples span {actual_seconds}s, expected {expected_seconds}s"),
            Self::AnnotationOutsideRecording { end_seconds, recording_end_seconds } => write!(f, "annotation ends at {end_seconds}s, beyond recording end {recording_end_seconds}s"),
        }
    }
}

impl std::error::Error for EegError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(label: &str, hz: f64) -> EegChannel {
        EegChannel::new(
            label, VoltageUnit::Microvolt, Some(Electrode::new(label).unwrap()),
            ChannelReference::Unknown, SamplingRate::new(hz).unwrap(),
        ).unwrap()
    }

    #[test]
    fn represents_channels_with_different_sampling_rates() {
        let recording = EegRecording::new(
            vec![channel("O1", 256.0), channel("AUX", 128.0)],
            vec![EegSegment::new(0.0, 1.0, vec![vec![1.0; 256], vec![2.0; 128]]).unwrap()],
            vec![], DataOrigin::Recorded,
        ).unwrap();
        assert_eq!(recording.channels[0].sampling_rate.hz(), 256.0);
        assert_eq!(recording.channels[1].sampling_rate.hz(), 128.0);
        assert_eq!(recording.duration_seconds(), 1.0);
    }

    #[test]
    fn preserves_a_discontinuous_timeline() {
        let recording = EegRecording::new(
            vec![channel("Cz", 10.0)],
            vec![
                EegSegment::new(0.0, 1.0, vec![vec![0.0; 10]]).unwrap(),
                EegSegment::new(3.0, 1.0, vec![vec![1.0; 10]]).unwrap(),
            ],
            vec![Annotation::new(2.0, 0.0, "event_during_gap").unwrap()],
            DataOrigin::Recorded,
        ).unwrap();
        assert_eq!(recording.segment_count(), 2);
        assert_eq!(recording.duration_seconds(), 4.0);
        assert_eq!(recording.segments[1].onset_seconds, 3.0);
    }

    #[test]
    fn rejects_overlapping_segments() {
        let result = EegRecording::new(
            vec![channel("Cz", 10.0)],
            vec![
                EegSegment::new(0.0, 1.0, vec![vec![0.0; 10]]).unwrap(),
                EegSegment::new(0.5, 1.0, vec![vec![1.0; 10]]).unwrap(),
            ],
            vec![], DataOrigin::Recorded,
        );
        assert!(matches!(result, Err(EegError::OverlappingOrUnorderedSegments { .. })));
    }

    #[test]
    fn rejects_sample_counts_that_do_not_match_segment_duration() {
        let result = EegRecording::new(
            vec![channel("Cz", 10.0)],
            vec![EegSegment::new(0.0, 1.0, vec![vec![0.0; 9]]).unwrap()],
            vec![], DataOrigin::Recorded,
        );
        assert!(matches!(result, Err(EegError::SegmentSampleDurationMismatch { .. })));
    }

    #[test]
    fn rejects_annotations_beyond_recording_end() {
        let result = EegRecording::new(
            vec![channel("Cz", 10.0)],
            vec![EegSegment::new(0.0, 1.0, vec![vec![0.0; 10]]).unwrap()],
            vec![Annotation::new(0.9, 0.2, "event").unwrap()],
            DataOrigin::Recorded,
        );
        assert!(matches!(result, Err(EegError::AnnotationOutsideRecording { .. })));
    }

    #[test]
    fn converts_microvolts_to_volts() {
        assert!((VoltageUnit::Microvolt.to_volts_scale() - 1e-6).abs() < 1e-15);
    }
}
