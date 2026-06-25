//! UI helpers for [`super::SourceKind`] (combo labels, feature-gated device list).

use super::SourceKind;

pub fn all_source_kinds() -> Vec<SourceKind> {
    let mut kinds = vec![SourceKind::Kiwi];
    #[cfg(feature = "airspy")]
    kinds.push(SourceKind::Airspy);
    #[cfg(feature = "rtlsdr")]
    kinds.push(SourceKind::RtlSdr);
    #[cfg(feature = "qmx")]
    kinds.push(SourceKind::Qmx);
    kinds
}

pub fn source_kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Kiwi => "KiwiSDR",
        #[cfg(feature = "airspy")]
        SourceKind::Airspy => "Airspy",
        #[cfg(feature = "rtlsdr")]
        SourceKind::RtlSdr => "RTL-SDR",
        #[cfg(feature = "qmx")]
        SourceKind::Qmx => "QMX",
    }
}

pub fn source_kind_labels() -> Vec<&'static str> {
    all_source_kinds()
        .into_iter()
        .map(source_kind_label)
        .collect()
}

pub fn source_kind_index(kind: SourceKind) -> usize {
    all_source_kinds()
        .iter()
        .position(|&k| k == kind)
        .unwrap_or(0)
}

pub fn source_kind_from_index(i: usize) -> SourceKind {
    all_source_kinds()
        .get(i)
        .copied()
        .unwrap_or(SourceKind::Kiwi)
}

pub fn is_local_source(kind: SourceKind) -> bool {
    match kind {
        SourceKind::Kiwi => false,
        #[cfg(feature = "airspy")]
        SourceKind::Airspy => true,
        #[cfg(feature = "rtlsdr")]
        SourceKind::RtlSdr => true,
        #[cfg(feature = "qmx")]
        SourceKind::Qmx => true,
    }
}
