//! Metadata-only reader for the fixed and per-signal EDF/EDF+ headers.
//!
//! Signal sample payload decoding is deliberately separate. This parser keeps
//! the original header fields needed to calibrate and interpret later samples.

#[derive(Debug, Clone, PartialEq)]
pub struct EdfSignalHeader {
    pub label: String,
    pub transducer: String,
    pub physical_dimension: String,
    pub physical_minimum: f64,
    pub physical_maximum: f64,
    pub digital_minimum: i32,
    pub digital_maximum: i32,
    pub prefiltering: String,
    pub samples_per_record: usize,
    pub reserved: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdfPlusMode { PlainEdf, Continuous, Discontinuous }

#[derive(Debug, Clone, PartialEq)]
pub struct EdfHeader {
    pub version: String,
    pub patient_id: String,
    pub recording_id: String,
    pub start_date: String,
    pub start_time: String,
    pub header_bytes: usize,
    pub reserved: String,
    /// None represents EDF's -1 (record count not yet known).
    pub data_record_count: Option<usize>,
    pub data_record_duration_seconds: f64,
    pub signals: Vec<EdfSignalHeader>,
    pub plus_mode: EdfPlusMode,
}

impl EdfHeader {
    /// Parse only the EDF fixed and per-signal header. The input may contain
    /// additional bytes after the header; those bytes are not decoded here.
    pub fn parse(bytes: &[u8]) -> Result<Self, EdfError> {
        if bytes.len() < 256 { return Err(EdfError::TruncatedFixedHeader { actual: bytes.len() }); }
        let fixed = FixedFields::new(&bytes[..256])?;
        let header_bytes = parse_usize(fixed.field(184, 8)?, "header byte count")?;
        let signal_count = parse_usize(fixed.field(252, 4)?, "signal count")?;
        if signal_count == 0 { return Err(EdfError::NoSignals); }
        let expected_header_bytes = 256usize.checked_add(signal_count.checked_mul(256).ok_or(EdfError::HeaderSizeOverflow)?)
            .ok_or(EdfError::HeaderSizeOverflow)?;
        if header_bytes != expected_header_bytes {
            return Err(EdfError::InvalidHeaderByteCount { declared: header_bytes, expected: expected_header_bytes });
        }
        if bytes.len() < header_bytes {
            return Err(EdfError::TruncatedSignalHeaders { expected: header_bytes, actual: bytes.len() });
        }

        let record_count_text = fixed.field(236, 8)?;
        let data_record_count = if record_count_text.trim() == "-1" {
            None
        } else {
            Some(parse_usize(record_count_text, "data record count")?)
        };
        let data_record_duration_seconds = parse_f64(fixed.field(244, 8)?, "data record duration")?;
        if data_record_duration_seconds <= 0.0 {
            return Err(EdfError::InvalidRecordDuration(data_record_duration_seconds));
        }

        let signal_bytes = &bytes[256..header_bytes];
        let mut cursor = 0;
        let labels = read_signal_fields(signal_bytes, &mut cursor, signal_count, 16)?;
        let transducers = read_signal_fields(signal_bytes, &mut cursor, signal_count, 80)?;
        let dimensions = read_signal_fields(signal_bytes, &mut cursor, signal_count, 8)?;
        let physical_minima = read_signal_fields(signal_bytes, &mut cursor, signal_count, 8)?;
        let physical_maxima = read_signal_fields(signal_bytes, &mut cursor, signal_count, 8)?;
        let digital_minima = read_signal_fields(signal_bytes, &mut cursor, signal_count, 8)?;
        let digital_maxima = read_signal_fields(signal_bytes, &mut cursor, signal_count, 8)?;
        let prefilters = read_signal_fields(signal_bytes, &mut cursor, signal_count, 80)?;
        let samples = read_signal_fields(signal_bytes, &mut cursor, signal_count, 8)?;
        let reserved = read_signal_fields(signal_bytes, &mut cursor, signal_count, 32)?;

        let mut signals = Vec::with_capacity(signal_count);
        for i in 0..signal_count {
            let physical_minimum = parse_f64(&physical_minima[i], "physical minimum")?;
            let physical_maximum = parse_f64(&physical_maxima[i], "physical maximum")?;
            let digital_minimum = parse_i32(&digital_minima[i], "digital minimum")?;
            let digital_maximum = parse_i32(&digital_maxima[i], "digital maximum")?;
            let samples_per_record = parse_usize(&samples[i], "samples per record")?;
            if physical_minimum >= physical_maximum {
                return Err(EdfError::InvalidPhysicalRange { signal_index: i });
            }
            if digital_minimum >= digital_maximum {
                return Err(EdfError::InvalidDigitalRange { signal_index: i });
            }
            if samples_per_record == 0 { return Err(EdfError::ZeroSamplesPerRecord { signal_index: i }); }
            signals.push(EdfSignalHeader {
                label: labels[i].clone(),
                transducer: transducers[i].clone(),
                physical_dimension: dimensions[i].clone(),
                physical_minimum,
                physical_maximum,
                digital_minimum,
                digital_maximum,
                prefiltering: prefilters[i].clone(),
                samples_per_record,
                reserved: reserved[i].clone(),
            });
        }

        let plus_mode = match fixed.field(192, 44)?.trim() {
            "" => EdfPlusMode::PlainEdf,
            value if value.starts_with("EDF+C") => EdfPlusMode::Continuous,
            value if value.starts_with("EDF+D") => EdfPlusMode::Discontinuous,
            value => return Err(EdfError::UnsupportedReservedField(value.to_owned())),
        };

        Ok(Self {
            version: fixed.field(0, 8)?.trim().to_owned(),
            patient_id: fixed.field(8, 80)?.trim().to_owned(),
            recording_id: fixed.field(88, 80)?.trim().to_owned(),
            start_date: fixed.field(168, 8)?.trim().to_owned(),
            start_time: fixed.field(176, 8)?.trim().to_owned(),
            header_bytes,
            reserved: fixed.field(192, 44)?.trim().to_owned(),
            data_record_count,
            data_record_duration_seconds,
            signals,
            plus_mode,
        })
    }

    /// Total duration when the header declares a known number of records.
    pub fn duration_seconds(&self) -> Option<f64> {
        self.data_record_count.map(|count| count as f64 * self.data_record_duration_seconds)
    }

    pub fn sampling_rate_hz(&self, signal_index: usize) -> Option<f64> {
        self.signals.get(signal_index).map(|signal| {
            signal.samples_per_record as f64 / self.data_record_duration_seconds
        })
    }
}

struct FixedFields<'a> { bytes: &'a [u8] }

impl<'a> FixedFields<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, EdfError> {
        if bytes.iter().any(|byte| !byte.is_ascii()) { return Err(EdfError::NonAsciiHeader); }
        Ok(Self { bytes })
    }
    fn field(&self, start: usize, length: usize) -> Result<&'a str, EdfError> {
        std::str::from_utf8(&self.bytes[start..start + length]).map_err(|_| EdfError::NonAsciiHeader)
    }
}

fn read_signal_fields(
    bytes: &[u8], cursor: &mut usize, signal_count: usize, width: usize,
) -> Result<Vec<String>, EdfError> {
    let total = signal_count.checked_mul(width).ok_or(EdfError::HeaderSizeOverflow)?;
    let end = cursor.checked_add(total).ok_or(EdfError::HeaderSizeOverflow)?;
    let field_bytes = bytes.get(*cursor..end).ok_or(EdfError::TruncatedSignalHeaders {
        expected: end, actual: bytes.len(),
    })?;
    if field_bytes.iter().any(|byte| !byte.is_ascii()) { return Err(EdfError::NonAsciiHeader); }
    let values = field_bytes.chunks_exact(width)
        .map(|chunk| std::str::from_utf8(chunk).unwrap().trim().to_owned())
        .collect();
    *cursor = end;
    Ok(values)
}

fn parse_f64(value: &str, field: &'static str) -> Result<f64, EdfError> {
    let parsed = value.trim().parse::<f64>().map_err(|_| EdfError::InvalidNumber {
        field, value: value.trim().to_owned(),
    })?;
    if !parsed.is_finite() {
        return Err(EdfError::InvalidNumber { field, value: value.trim().to_owned() });
    }
    Ok(parsed)
}

fn parse_i32(value: &str, field: &'static str) -> Result<i32, EdfError> {
    value.trim().parse::<i32>().map_err(|_| EdfError::InvalidInteger {
        field, value: value.trim().to_owned(),
    })
}

fn parse_usize(value: &str, field: &'static str) -> Result<usize, EdfError> {
    value.trim().parse::<usize>().map_err(|_| EdfError::InvalidInteger {
        field, value: value.trim().to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum EdfError {
    TruncatedFixedHeader { actual: usize },
    TruncatedSignalHeaders { expected: usize, actual: usize },
    NonAsciiHeader,
    InvalidInteger { field: &'static str, value: String },
    InvalidNumber { field: &'static str, value: String },
    InvalidHeaderByteCount { declared: usize, expected: usize },
    HeaderSizeOverflow,
    NoSignals,
    InvalidRecordDuration(f64),
    InvalidPhysicalRange { signal_index: usize },
    InvalidDigitalRange { signal_index: usize },
    ZeroSamplesPerRecord { signal_index: usize },
    UnsupportedReservedField(String),
}

impl std::fmt::Display for EdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TruncatedFixedHeader { actual } => write!(f, "EDF fixed header needs 256 bytes; received {actual}"),
            Self::TruncatedSignalHeaders { expected, actual } => write!(f, "EDF signal header needs {expected} bytes; received {actual}"),
            Self::NonAsciiHeader => write!(f, "EDF header contains non-ASCII bytes"),
            Self::InvalidInteger { field, value } => write!(f, "invalid integer in {field}: '{value}'"),
            Self::InvalidNumber { field, value } => write!(f, "invalid number in {field}: '{value}'"),
            Self::InvalidHeaderByteCount { declared, expected } => write!(f, "EDF header declares {declared} bytes; expected {expected} for its signal count"),
            Self::HeaderSizeOverflow => write!(f, "EDF header size overflows this platform's address space"),
            Self::NoSignals => write!(f, "EDF header declares zero signals"),
            Self::InvalidRecordDuration(value) => write!(f, "EDF data-record duration must be positive, got {value}"),
            Self::InvalidPhysicalRange { signal_index } => write!(f, "signal {signal_index} has an invalid physical calibration range"),
            Self::InvalidDigitalRange { signal_index } => write!(f, "signal {signal_index} has an invalid digital calibration range"),
            Self::ZeroSamplesPerRecord { signal_index } => write!(f, "signal {signal_index} declares zero samples per data record"),
            Self::UnsupportedReservedField(value) => write!(f, "unsupported EDF reserved field '{value}'"),
        }
    }
}
impl std::error::Error for EdfError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(out: &mut Vec<u8>, value: &str, width: usize) {
        assert!(value.len() <= width);
        out.extend_from_slice(value.as_bytes());
        out.extend(std::iter::repeat(b' ').take(width - value.len()));
    }

    fn fixture(reserved: &str) -> Vec<u8> {
        let mut out = Vec::new();
        field(&mut out, "0", 8);
        field(&mut out, "test patient", 80);
        field(&mut out, "test recording", 80);
        field(&mut out, "01.01.24", 8);
        field(&mut out, "12.00.00", 8);
        field(&mut out, "512", 8);
        field(&mut out, reserved, 44);
        field(&mut out, "2", 8);
        field(&mut out, "1", 8);
        field(&mut out, "2", 4);
        // EDF stores each per-signal field as a column across all signals.
        for (value, width) in [("O1",16), ("O2",16)] { field(&mut out,value,width); }
        for _ in 0..2 { field(&mut out,"",80); }
        for _ in 0..2 { field(&mut out,"uV",8); }
        for _ in 0..2 { field(&mut out,"-100",8); }
        for _ in 0..2 { field(&mut out,"100",8); }
        for _ in 0..2 { field(&mut out,"-32768",8); }
        for _ in 0..2 { field(&mut out,"32767",8); }
        for _ in 0..2 { field(&mut out,"",80); }
        field(&mut out,"256",8);
        field(&mut out,"128",8);
        for _ in 0..2 { field(&mut out,"",32); }
        out
    }

    #[test]
    fn parses_signal_metadata_and_per_channel_rates() {
        let header = EdfHeader::parse(&fixture("")).unwrap();
        assert_eq!(header.signals.len(), 2);
        assert_eq!(header.signals[0].label, "O1");
        assert_eq!(header.sampling_rate_hz(0), Some(256.0));
        assert_eq!(header.sampling_rate_hz(1), Some(128.0));
        assert_eq!(header.duration_seconds(), Some(2.0));
        assert_eq!(header.plus_mode, EdfPlusMode::PlainEdf);
    }

    #[test]
    fn recognizes_edf_plus_discontinuous_recordings() {
        let header = EdfHeader::parse(&fixture("EDF+D")).unwrap();
        assert_eq!(header.plus_mode, EdfPlusMode::Discontinuous);
    }

    #[test]
    fn recognizes_edf_plus_continuous_recordings() {
        let header = EdfHeader::parse(&fixture("EDF+C")).unwrap();
        assert_eq!(header.plus_mode, EdfPlusMode::Continuous);
    }

    #[test]
    fn rejects_a_truncated_fixed_header() {
        assert!(matches!(EdfHeader::parse(&[0; 20]), Err(EdfError::TruncatedFixedHeader { .. })));
    }

    #[test]
    fn rejects_invalid_header_byte_count() {
        let mut bytes = fixture("");
        bytes[184..192].copy_from_slice(b"00000257");
        assert!(matches!(EdfHeader::parse(&bytes), Err(EdfError::InvalidHeaderByteCount { .. })));
    }
}
