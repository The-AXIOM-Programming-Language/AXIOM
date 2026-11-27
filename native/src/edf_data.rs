//! EDF data-record decoding and EDF+ TAL annotation parsing.
//!
//! EDF ordinary samples are signed little-endian i16 values. EDF+ annotation
//! channels are byte streams stored in the same two-byte slots; they must not
//! be calibrated as physiological signals.

use crate::edf::{EdfHeader, EdfSignalHeader};

#[derive(Debug, Clone, PartialEq)]
pub struct EdfSignalSamples {
    pub digital: Vec<i16>,
    /// Calibrated values in the physical unit named by the signal header.
    pub physical: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EdfSignalRecord {
    Samples(EdfSignalSamples),
    AnnotationBytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdfDataRecord {
    pub signals: Vec<EdfSignalRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdfData {
    pub records: Vec<EdfDataRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tal {
    pub onset_seconds: f64,
    pub duration_seconds: Option<f64>,
    /// Text fields are retained in order. The first empty text in the first
    /// TAL of each EDF+ record is the record time-keeping marker.
    pub texts: Vec<String>,
}

impl Tal {
    pub fn is_timekeeping(&self) -> bool {
        self.texts.first().is_some_and(|text| text.is_empty())
    }
}

/// Decode all complete EDF data records following an already-parsed header.
/// A declared record count is enforced. For an EDF header with record count
/// -1, the number of records is inferred from the exact remaining byte count.
pub fn decode_data_records(header: &EdfHeader, bytes: &[u8]) -> Result<EdfData, EdfDataError> {
    if bytes.len() < header.header_bytes {
        return Err(EdfDataError::TruncatedHeader {
            expected: header.header_bytes,
            actual: bytes.len(),
        });
    }
    let payload = &bytes[header.header_bytes..];
    let record_bytes = header.signals.iter().try_fold(0usize, |sum, signal| {
        let channel_bytes = signal.samples_per_record.checked_mul(2)
            .ok_or(EdfDataError::SizeOverflow)?;
        sum.checked_add(channel_bytes).ok_or(EdfDataError::SizeOverflow)
    })?;
    if record_bytes == 0 {
        return Err(EdfDataError::ZeroRecordSize);
    }
    let record_count = match header.data_record_count {
        Some(count) => {
            let expected = count.checked_mul(record_bytes).ok_or(EdfDataError::SizeOverflow)?;
            if payload.len() != expected {
                return Err(EdfDataError::PayloadLengthMismatch {
                    expected,
                    actual: payload.len(),
                });
            }
            count
        }
        None => {
            if payload.len() % record_bytes != 0 {
                return Err(EdfDataError::IncompleteFinalRecord {
                    record_bytes,
                    remaining_bytes: payload.len() % record_bytes,
                });
            }
            payload.len() / record_bytes
        }
    };

    let mut records = Vec::with_capacity(record_count);
    let mut offset = 0usize;
    for _ in 0..record_count {
        let mut signals = Vec::with_capacity(header.signals.len());
        for signal in &header.signals {
            let byte_count = signal.samples_per_record.checked_mul(2)
                .ok_or(EdfDataError::SizeOverflow)?;
            let end = offset.checked_add(byte_count).ok_or(EdfDataError::SizeOverflow)?;
            let raw = payload.get(offset..end).ok_or(EdfDataError::IncompleteFinalRecord {
                record_bytes,
                remaining_bytes: payload.len().saturating_sub(offset),
            })?;
            offset = end;

            if signal.is_annotation_channel() {
                signals.push(EdfSignalRecord::AnnotationBytes(raw.to_vec()));
            } else {
                let digital: Vec<i16> = raw.chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                let physical = digital.iter().map(|&sample| calibrate_sample(signal, sample)).collect();
                signals.push(EdfSignalRecord::Samples(EdfSignalSamples { digital, physical }));
            }
        }
        records.push(EdfDataRecord { signals });
    }
    Ok(EdfData { records })
}

fn calibrate_sample(signal: &EdfSignalHeader, digital: i16) -> f64 {
    let gain = (signal.physical_maximum - signal.physical_minimum)
        / (signal.digital_maximum - signal.digital_minimum) as f64;
    signal.physical_minimum
        + (digital as f64 - signal.digital_minimum as f64) * gain
}

/// Parse the byte stream from one EDF+ annotation signal in one data record.
/// Zero bytes after the final TAL are padding. Text is decoded as UTF-8.
pub fn parse_tals(bytes: &[u8]) -> Result<Vec<Tal>, EdfDataError> {
    let mut tals = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] == 0 {
            if bytes[cursor..].iter().any(|&byte| byte != 0) {
                return Err(EdfDataError::NonZeroAfterTalPadding);
            }
            break;
        }

        let timestamp_start = cursor;
        while cursor < bytes.len() && bytes[cursor] != 0x14 && bytes[cursor] != 0x15 {
            cursor += 1;
        }
        if cursor == bytes.len() {
            return Err(EdfDataError::MalformedTal("timestamp has no separator".into()));
        }
        let onset_seconds = parse_tal_number(&bytes[timestamp_start..cursor], true)?;

        let duration_seconds = if bytes[cursor] == 0x15 {
            cursor += 1;
            let duration_start = cursor;
            while cursor < bytes.len() && bytes[cursor] != 0x14 {
                cursor += 1;
            }
            if cursor == bytes.len() {
                return Err(EdfDataError::MalformedTal("duration has no text separator".into()));
            }
            let duration = parse_tal_number(&bytes[duration_start..cursor], false)?;
            if duration < 0.0 {
                return Err(EdfDataError::MalformedTal("duration cannot be negative".into()));
            }
            Some(duration)
        } else {
            None
        };

        if cursor >= bytes.len() || bytes[cursor] != 0x14 {
            return Err(EdfDataError::MalformedTal("missing text separator".into()));
        }
        cursor += 1;
        let mut texts = Vec::new();
        let mut text_start = cursor;
        loop {
            while cursor < bytes.len() && bytes[cursor] != 0x14 && bytes[cursor] != 0 {
                cursor += 1;
            }
            if cursor == bytes.len() {
                return Err(EdfDataError::MalformedTal("TAL has no zero terminator".into()));
            }
            // A separator immediately followed by NUL terminates the text
            // list; it is not an additional empty annotation field unless
            // this is the first (timekeeping) text field.
            if cursor > text_start || texts.is_empty() {
                let text = std::str::from_utf8(&bytes[text_start..cursor])
                    .map_err(|_| EdfDataError::InvalidAnnotationUtf8)?
                    .to_owned();
                texts.push(text);
            }
            if bytes[cursor] == 0 {
                cursor += 1;
                break;
            }
            cursor += 1;
            text_start = cursor;
        }
        tals.push(Tal { onset_seconds, duration_seconds, texts });
    }
    Ok(tals)
}

fn parse_tal_number(bytes: &[u8], signed: bool) -> Result<f64, EdfDataError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| EdfDataError::MalformedTal("timestamp is not ASCII".into()))?;
    let valid = if signed {
        let digits = text.strip_prefix('+').or_else(|| text.strip_prefix('-'));
        digits.is_some_and(valid_decimal)
    } else {
        valid_decimal(text)
    };
    if !valid {
        return Err(EdfDataError::MalformedTal(format!("invalid TAL number '{text}'")));
    }
    let value = text.parse::<f64>()
        .map_err(|_| EdfDataError::MalformedTal(format!("invalid TAL number '{text}'")))?;
    if !value.is_finite() {
        return Err(EdfDataError::MalformedTal("TAL number is not finite".into()));
    }
    Ok(value)
}

fn valid_decimal(text: &str) -> bool {
    if text.is_empty() { return false; }
    let mut dots = 0usize;
    let mut digits = 0usize;
    for byte in text.bytes() {
        match byte {
            b'0'..=b'9' => digits += 1,
            b'.' => dots += 1,
            _ => return false,
        }
    }
    digits > 0 && dots <= 1
}

#[derive(Debug, Clone, PartialEq)]
pub enum EdfDataError {
    TruncatedHeader { expected: usize, actual: usize },
    SizeOverflow,
    ZeroRecordSize,
    PayloadLengthMismatch { expected: usize, actual: usize },
    IncompleteFinalRecord { record_bytes: usize, remaining_bytes: usize },
    NonZeroAfterTalPadding,
    InvalidAnnotationUtf8,
    MalformedTal(String),
}

impl std::fmt::Display for EdfDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TruncatedHeader { expected, actual } => write!(f, "EDF header needs {expected} bytes; received {actual}"),
            Self::SizeOverflow => write!(f, "EDF data size overflows this platform's address space"),
            Self::ZeroRecordSize => write!(f, "EDF data record has zero bytes"),
            Self::PayloadLengthMismatch { expected, actual } => write!(f, "EDF payload has {actual} bytes; header requires exactly {expected}"),
            Self::IncompleteFinalRecord { record_bytes, remaining_bytes } => write!(f, "incomplete EDF data record: record size is {record_bytes} bytes, {remaining_bytes} bytes remain"),
            Self::NonZeroAfterTalPadding => write!(f, "non-zero byte found after TAL padding began"),
            Self::InvalidAnnotationUtf8 => write!(f, "EDF+ annotation text is not valid UTF-8"),
            Self::MalformedTal(message) => write!(f, "malformed EDF+ TAL: {message}"),
        }
    }
}
impl std::error::Error for EdfDataError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edf::{EdfPlusMode, EdfSignalHeader};

    fn signal(label: &str, samples: usize, pmin: f64, pmax: f64) -> EdfSignalHeader {
        EdfSignalHeader {
            label: label.into(), transducer: String::new(), physical_dimension: "uV".into(),
            physical_minimum: pmin, physical_maximum: pmax,
            digital_minimum: -32768, digital_maximum: 32767,
            prefiltering: String::new(), samples_per_record: samples, reserved: String::new(),
        }
    }

    fn header(signals: Vec<EdfSignalHeader>, count: Option<usize>) -> EdfHeader {
        EdfHeader {
            version: "0".into(), patient_id: String::new(), recording_id: String::new(),
            start_date: String::new(), start_time: String::new(), header_bytes: 256,
            reserved: String::new(), data_record_count: count,
            data_record_duration_seconds: 1.0, signals, plus_mode: EdfPlusMode::PlainEdf,
        }
    }

    #[test]
    fn decodes_little_endian_samples_and_calibrates_without_losing_raw_values() {
        let h = header(vec![signal("O1", 2, -100.0, 100.0)], Some(1));
        let mut bytes = vec![0; 256];
        bytes.extend_from_slice(&i16::MIN.to_le_bytes());
        bytes.extend_from_slice(&i16::MAX.to_le_bytes());
        let decoded = decode_data_records(&h, &bytes).unwrap();
        let EdfSignalRecord::Samples(samples) = &decoded.records[0].signals[0] else { panic!("expected samples") };
        assert_eq!(samples.digital, vec![i16::MIN, i16::MAX]);
        assert!((samples.physical[0] + 100.0).abs() < 1e-9);
        assert!((samples.physical[1] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn preserves_annotation_signal_as_raw_bytes() {
        let h = header(vec![signal("EDF Annotations", 4, -1.0, 1.0)], Some(1));
        let mut bytes = vec![0; 256];
        bytes.extend_from_slice(b"+0\x14\x14\0\0\0\0");
        let decoded = decode_data_records(&h, &bytes).unwrap();
        assert_eq!(decoded.records[0].signals[0], EdfSignalRecord::AnnotationBytes(b"+0\x14\x14\0\0\0\0".to_vec()));
    }

    #[test]
    fn infers_unknown_record_count_from_complete_payload() {
        let h = header(vec![signal("Cz", 1, -1.0, 1.0)], None);
        let mut bytes = vec![0; 256];
        bytes.extend_from_slice(&42i16.to_le_bytes());
        assert_eq!(decode_data_records(&h, &bytes).unwrap().records.len(), 1);
    }

    #[test]
    fn rejects_payload_with_wrong_length() {
        let h = header(vec![signal("Cz", 2, -1.0, 1.0)], Some(1));
        let mut bytes = vec![0; 256];
        bytes.extend_from_slice(&0i16.to_le_bytes());
        assert!(matches!(decode_data_records(&h, &bytes), Err(EdfDataError::PayloadLengthMismatch { .. })));
    }

    #[test]
    fn parses_timekeeping_and_event_tals() {
        let bytes = b"+0.25\x14\x14\0+1.5\x15" ;
        let mut bytes = bytes.to_vec();
        bytes.extend_from_slice(b"0.2\x14stimulus\x14\0\0\0");
        let tals = parse_tals(&bytes).unwrap();
        assert_eq!(tals.len(), 2);
        assert!(tals[0].is_timekeeping());
        assert_eq!(tals[0].onset_seconds, 0.25);
        assert_eq!(tals[1].onset_seconds, 1.5);
        assert_eq!(tals[1].duration_seconds, Some(0.2));
        assert_eq!(tals[1].texts, vec!["stimulus"]);
    }

    #[test]
    fn rejects_malformed_tal_timestamp() {
        assert!(parse_tals(b"+1e3\x14event\x14\0").is_err());
    }
}
