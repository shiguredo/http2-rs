#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use shiguredo_http2::webtransport::{Capsule, CapsuleEncoder, varint};

fn clamp_varint(v: u64) -> u64 {
    v % (varint::MAX_VALUE + 1)
}

#[derive(Debug, Arbitrary)]
enum FuzzCapsule {
    Datagram {
        data: Vec<u8>,
    },
    // length は u16 に制限 (エンコーダーが length バイト分のゼロ埋めを
    // メモリ確保するため、usize のまま生成すると OOM になる)
    Padding {
        length: u16,
    },
    WtResetStream {
        stream_id: u64,
        error_code: u64,
        reliable_size: u64,
    },
    WtStopSending {
        stream_id: u64,
        error_code: u64,
    },
    WtStream {
        stream_id: u64,
        data: Vec<u8>,
        fin: bool,
    },
    WtMaxData {
        maximum: u64,
    },
    WtMaxStreamData {
        stream_id: u64,
        maximum: u64,
    },
    WtMaxStreams {
        maximum: u64,
        bidirectional: bool,
    },
    WtDataBlocked {
        maximum: u64,
    },
    WtStreamDataBlocked {
        stream_id: u64,
        maximum: u64,
    },
    WtStreamsBlocked {
        maximum: u64,
        bidirectional: bool,
    },
    WtCloseSession {
        error_code: u32,
        reason: Vec<u8>,
    },
    WtDrainSession,
    Unknown {
        capsule_type: u64,
        data: Vec<u8>,
    },
}

fn to_capsule(input: &FuzzCapsule) -> Capsule {
    match input {
        FuzzCapsule::Datagram { data } => Capsule::Datagram { data: data.clone() },
        FuzzCapsule::Padding { length } => Capsule::Padding {
            length: *length as usize,
        },
        FuzzCapsule::WtResetStream {
            stream_id,
            error_code,
            reliable_size,
        } => Capsule::WtResetStream {
            stream_id: clamp_varint(*stream_id),
            error_code: clamp_varint(*error_code),
            reliable_size: clamp_varint(*reliable_size),
        },
        FuzzCapsule::WtStopSending {
            stream_id,
            error_code,
        } => Capsule::WtStopSending {
            stream_id: clamp_varint(*stream_id),
            error_code: clamp_varint(*error_code),
        },
        FuzzCapsule::WtStream {
            stream_id,
            data,
            fin,
        } => Capsule::WtStream {
            stream_id: clamp_varint(*stream_id),
            data: data.clone(),
            fin: *fin,
        },
        FuzzCapsule::WtMaxData { maximum } => Capsule::WtMaxData {
            maximum: clamp_varint(*maximum),
        },
        FuzzCapsule::WtMaxStreamData { stream_id, maximum } => Capsule::WtMaxStreamData {
            stream_id: clamp_varint(*stream_id),
            maximum: clamp_varint(*maximum),
        },
        FuzzCapsule::WtMaxStreams {
            maximum,
            bidirectional,
        } => Capsule::WtMaxStreams {
            maximum: clamp_varint(*maximum),
            bidirectional: *bidirectional,
        },
        FuzzCapsule::WtDataBlocked { maximum } => Capsule::WtDataBlocked {
            maximum: clamp_varint(*maximum),
        },
        FuzzCapsule::WtStreamDataBlocked { stream_id, maximum } => {
            Capsule::WtStreamDataBlocked {
                stream_id: clamp_varint(*stream_id),
                maximum: clamp_varint(*maximum),
            }
        }
        FuzzCapsule::WtStreamsBlocked {
            maximum,
            bidirectional,
        } => Capsule::WtStreamsBlocked {
            maximum: clamp_varint(*maximum),
            bidirectional: *bidirectional,
        },
        FuzzCapsule::WtCloseSession { error_code, reason } => Capsule::WtCloseSession {
            error_code: *error_code,
            reason: String::from_utf8_lossy(reason).into_owned(),
        },
        FuzzCapsule::WtDrainSession => Capsule::WtDrainSession,
        FuzzCapsule::Unknown { capsule_type, data } => Capsule::Unknown {
            capsule_type: clamp_varint(*capsule_type),
            data: data.clone(),
        },
    }
}

fuzz_target!(|input: FuzzCapsule| {
    // 任意の Capsule をエンコードし、パニックしないことのみを検証する。
    let capsule = to_capsule(&input);
    let mut encoder = CapsuleEncoder::new();
    encoder.encode(&capsule);
});
