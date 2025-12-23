//! Runtime support for lolserialize encoding/decoding.

use std::collections::HashMap;
use std::hash::Hash;

// ============================================================================
// Wire types
// ============================================================================

/// Wire type constants for encoding/decoding.
pub mod wire {
    pub const FIXED8: u8 = 0;
    pub const VARINT: u8 = 1;
    pub const FIXED32: u8 = 2;
    pub const FIXED64: u8 = 3;
    pub const BYTES: u8 = 4;
    pub const MESSAGE: u8 = 5;
    pub const UNION: u8 = 6;
    pub const UNIT: u8 = 7;
}

/// Trait for types that have a wire type.
pub trait WireType {
    /// The wire type used when encoding this type as a message/union field.
    const WIRE_TYPE: u8;
    /// Fixed encoded size in bytes, if known at compile time.
    /// None means variable-length encoding.
    const FIXED_SIZE: Option<usize> = None;
}

// Wire type implementations for primitives
impl WireType for bool {
    const WIRE_TYPE: u8 = wire::FIXED8;
    const FIXED_SIZE: Option<usize> = Some(1);
}
impl WireType for u8 {
    const WIRE_TYPE: u8 = wire::FIXED8;
    const FIXED_SIZE: Option<usize> = Some(1);
}
impl WireType for i8 {
    const WIRE_TYPE: u8 = wire::FIXED8;
    const FIXED_SIZE: Option<usize> = Some(1);
}
impl WireType for u16 {
    const WIRE_TYPE: u8 = wire::VARINT;
}
impl WireType for u32 {
    const WIRE_TYPE: u8 = wire::VARINT;
}
impl WireType for u64 {
    const WIRE_TYPE: u8 = wire::VARINT;
}
impl WireType for i16 {
    const WIRE_TYPE: u8 = wire::VARINT;
}
impl WireType for i32 {
    const WIRE_TYPE: u8 = wire::VARINT;
}
impl WireType for i64 {
    const WIRE_TYPE: u8 = wire::VARINT;
}
impl WireType for f32 {
    const WIRE_TYPE: u8 = wire::FIXED32;
    const FIXED_SIZE: Option<usize> = Some(4);
}
impl WireType for f64 {
    const WIRE_TYPE: u8 = wire::FIXED64;
    const FIXED_SIZE: Option<usize> = Some(8);
}
impl WireType for String {
    const WIRE_TYPE: u8 = wire::BYTES;
}
impl<T> WireType for Vec<T> {
    const WIRE_TYPE: u8 = wire::BYTES;
}
impl<K, V> WireType for HashMap<K, V> {
    const WIRE_TYPE: u8 = wire::BYTES;
}

/// Error type for decoding operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Unexpected end of input.
    UnexpectedEof,
    /// Invalid UTF-8 in string.
    InvalidUtf8,
    /// Invalid bool value (not 0 or 1).
    InvalidBool(u8),
    /// Unknown enum variant.
    UnknownEnumVariant(u32),
    /// Unknown union variant.
    UnknownUnionVariant(u32),
    /// LEB128 overflow.
    Leb128Overflow,
    /// Invalid wire type.
    InvalidWireType(u8),
    /// Invalid length (not a multiple of element size).
    InvalidLength,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::UnexpectedEof => write!(f, "unexpected end of input"),
            DecodeError::InvalidUtf8 => write!(f, "invalid UTF-8 in string"),
            DecodeError::InvalidBool(v) => write!(f, "invalid bool value: {}", v),
            DecodeError::UnknownEnumVariant(v) => write!(f, "unknown enum variant: {}", v),
            DecodeError::UnknownUnionVariant(v) => write!(f, "unknown union variant: {}", v),
            DecodeError::Leb128Overflow => write!(f, "LEB128 overflow"),
            DecodeError::InvalidWireType(v) => write!(f, "invalid wire type: {}", v),
            DecodeError::InvalidLength => {
                write!(f, "invalid length (not a multiple of element size)")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Trait for types that can be encoded.
pub trait Encode: WireType {
    /// Encode the value to the buffer. Self-describing format.
    fn encode(&self, buf: &mut Vec<u8>);

    /// Encode when the byte length will be written by the caller (for BYTES wire type fields).
    /// Default just calls encode() - override for types that can skip inner length prefixes.
    fn encode_delimited(&self, buf: &mut Vec<u8>) {
        self.encode(buf);
    }
}

/// Trait for types that can be decoded.
pub trait Decode: WireType + Sized {
    /// Decode a value from the buffer. Self-describing format.
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError>;

    /// Decode when the byte length is already known (for BYTES wire type fields).
    /// The caller has already read the length prefix and provides the exact byte count.
    /// Default just calls decode() - override for types that can skip inner length prefixes.
    fn decode_delimited(len: usize, buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let _ = len; // unused in default impl
        Self::decode(buf)
    }
}

// ============================================================================
// LEB128 encoding/decoding helpers
// ============================================================================

/// Encode an unsigned integer as LEB128.
#[inline]
pub fn encode_leb128_u64(mut value: u64, buf: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Decode an unsigned LEB128 integer.
#[inline]
pub fn decode_leb128_u64(buf: &mut &[u8]) -> Result<u64, DecodeError> {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }
        let byte = buf[0];
        *buf = &buf[1..];

        if shift >= 64 {
            return Err(DecodeError::Leb128Overflow);
        }

        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
    }
}

/// Zigzag encode a signed integer.
#[inline]
pub fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

/// Zigzag decode an unsigned integer to signed.
#[inline]
pub fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ (-((n & 1) as i64))
}

// ============================================================================
// Primitive type implementations
// ============================================================================

impl Encode for bool {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(if *self { 1 } else { 0 });
    }
}

impl Decode for bool {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }
        let byte = buf[0];
        *buf = &buf[1..];
        match byte {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::InvalidBool(byte)),
        }
    }
}

impl Encode for u8 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(*self);
    }
}

impl Decode for u8 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }
        let byte = buf[0];
        *buf = &buf[1..];
        Ok(byte)
    }
}

impl Encode for i8 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(*self as u8);
    }
}

impl Decode for i8 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.is_empty() {
            return Err(DecodeError::UnexpectedEof);
        }
        let byte = buf[0];
        *buf = &buf[1..];
        Ok(byte as i8)
    }
}

impl Encode for u16 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_leb128_u64(*self as u64, buf);
    }
}

impl Decode for u16 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let v = decode_leb128_u64(buf)?;
        Ok(v as u16)
    }
}

impl Encode for u32 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_leb128_u64(*self as u64, buf);
    }
}

impl Decode for u32 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let v = decode_leb128_u64(buf)?;
        Ok(v as u32)
    }
}

impl Encode for u64 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_leb128_u64(*self, buf);
    }
}

impl Decode for u64 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        decode_leb128_u64(buf)
    }
}

impl Encode for i16 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_leb128_u64(zigzag_encode(*self as i64), buf);
    }
}

impl Decode for i16 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let v = decode_leb128_u64(buf)?;
        Ok(zigzag_decode(v) as i16)
    }
}

impl Encode for i32 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_leb128_u64(zigzag_encode(*self as i64), buf);
    }
}

impl Decode for i32 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let v = decode_leb128_u64(buf)?;
        Ok(zigzag_decode(v) as i32)
    }
}

impl Encode for i64 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_leb128_u64(zigzag_encode(*self), buf);
    }
}

impl Decode for i64 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let v = decode_leb128_u64(buf)?;
        Ok(zigzag_decode(v))
    }
}

impl Encode for f32 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl Decode for f32 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 4 {
            return Err(DecodeError::UnexpectedEof);
        }
        let bytes = [buf[0], buf[1], buf[2], buf[3]];
        *buf = &buf[4..];
        Ok(f32::from_le_bytes(bytes))
    }
}

impl Encode for f64 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
}

impl Decode for f64 {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 8 {
            return Err(DecodeError::UnexpectedEof);
        }
        let bytes = [buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]];
        *buf = &buf[8..];
        Ok(f64::from_le_bytes(bytes))
    }
}

// ============================================================================
// String implementation
// ============================================================================

impl Encode for String {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        // Self-describing: write length prefix then raw bytes
        encode_leb128_u64(self.len() as u64, buf);
        buf.extend_from_slice(self.as_bytes());
    }

    #[inline]
    fn encode_delimited(&self, buf: &mut Vec<u8>) {
        // Caller knows length, just write raw bytes
        buf.extend_from_slice(self.as_bytes());
    }
}

impl Decode for String {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        // Self-describing: read length prefix then raw bytes
        let len = decode_leb128_u64(buf)? as usize;
        if buf.len() < len {
            return Err(DecodeError::UnexpectedEof);
        }
        let bytes = &buf[..len];
        *buf = &buf[len..];
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::InvalidUtf8)
    }

    #[inline]
    fn decode_delimited(len: usize, buf: &mut &[u8]) -> Result<Self, DecodeError> {
        // Caller provides length, just read raw bytes
        if buf.len() < len {
            return Err(DecodeError::UnexpectedEof);
        }
        let bytes = &buf[..len];
        *buf = &buf[len..];
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::InvalidUtf8)
    }
}

// ============================================================================
// Vec (Array) implementation
// ============================================================================

impl<T: Encode> Encode for Vec<T> {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        // Self-describing: write count then elements
        encode_leb128_u64(self.len() as u64, buf);
        for item in self {
            item.encode(buf);
        }
    }

    #[inline]
    fn encode_delimited(&self, buf: &mut Vec<u8>) {
        if T::FIXED_SIZE.is_some() {
            // Fixed-size elements: skip count (can be inferred from byte_length / element_size)
            for item in self {
                item.encode(buf);
            }
        } else {
            // Variable-size elements: still need count
            encode_leb128_u64(self.len() as u64, buf);
            for item in self {
                item.encode(buf);
            }
        }
    }
}

impl<T: Decode> Decode for Vec<T> {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        // Self-describing: read count then elements
        let count = decode_leb128_u64(buf)? as usize;
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            result.push(T::decode(buf)?);
        }
        Ok(result)
    }

    #[inline]
    fn decode_delimited(len: usize, buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < len {
            return Err(DecodeError::UnexpectedEof);
        }
        let mut slice = &buf[..len];
        let result = if let Some(elem_size) = T::FIXED_SIZE {
            // Fixed-size elements: infer count from byte length
            if len % elem_size != 0 {
                return Err(DecodeError::InvalidLength);
            }
            let count = len / elem_size;
            let mut result = Vec::with_capacity(count);
            for _ in 0..count {
                result.push(T::decode(&mut slice)?);
            }
            result
        } else {
            // Variable-size elements: read count
            let count = decode_leb128_u64(&mut slice)? as usize;
            let mut result = Vec::with_capacity(count);
            for _ in 0..count {
                result.push(T::decode(&mut slice)?);
            }
            result
        };
        *buf = &buf[len..];
        Ok(result)
    }
}

// ============================================================================
// HashMap (Map) implementation
// ============================================================================

impl<K: Encode, V: Encode> Encode for HashMap<K, V> {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        // Self-describing: write count then entries
        encode_leb128_u64(self.len() as u64, buf);
        for (k, v) in self {
            k.encode(buf);
            v.encode(buf);
        }
    }

    #[inline]
    fn encode_delimited(&self, buf: &mut Vec<u8>) {
        let entry_size = match (K::FIXED_SIZE, V::FIXED_SIZE) {
            (Some(k), Some(v)) => Some(k + v),
            _ => None,
        };

        if entry_size.is_some() {
            // Fixed-size entries: skip count (can be inferred from byte_length / entry_size)
            for (k, v) in self {
                k.encode(buf);
                v.encode(buf);
            }
        } else {
            // Variable-size entries: still need count
            encode_leb128_u64(self.len() as u64, buf);
            for (k, v) in self {
                k.encode(buf);
                v.encode(buf);
            }
        }
    }
}

impl<K: Decode + Eq + Hash, V: Decode> Decode for HashMap<K, V> {
    #[inline]
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        // Self-describing: read count then entries
        let count = decode_leb128_u64(buf)? as usize;
        let mut result = HashMap::with_capacity(count);
        for _ in 0..count {
            let k = K::decode(buf)?;
            let v = V::decode(buf)?;
            result.insert(k, v);
        }
        Ok(result)
    }

    #[inline]
    fn decode_delimited(len: usize, buf: &mut &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < len {
            return Err(DecodeError::UnexpectedEof);
        }
        let mut slice = &buf[..len];
        let entry_size = match (K::FIXED_SIZE, V::FIXED_SIZE) {
            (Some(k), Some(v)) => Some(k + v),
            _ => None,
        };
        let result = if let Some(entry_size) = entry_size {
            // Fixed-size entries: infer count from byte length
            if len % entry_size != 0 {
                return Err(DecodeError::InvalidLength);
            }
            let count = len / entry_size;
            let mut result = HashMap::with_capacity(count);
            for _ in 0..count {
                let k = K::decode(&mut slice)?;
                let v = V::decode(&mut slice)?;
                result.insert(k, v);
            }
            result
        } else {
            // Variable-size entries: read count
            let count = decode_leb128_u64(&mut slice)? as usize;
            let mut result = HashMap::with_capacity(count);
            for _ in 0..count {
                let k = K::decode(&mut slice)?;
                let v = V::decode(&mut slice)?;
                result.insert(k, v);
            }
            result
        };
        *buf = &buf[len..];
        Ok(result)
    }
}

// ============================================================================
// Option implementation (for struct optional fields)
// ============================================================================

// Option doesn't have a real wire type - it inherits from the inner type.
// This is only used for struct optional fields which are handled specially.
impl<T: WireType> WireType for Option<T> {
    const WIRE_TYPE: u8 = T::WIRE_TYPE;
    const FIXED_SIZE: Option<usize> = T::FIXED_SIZE;
}

impl<T: Encode> Encode for Option<T> {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        if let Some(v) = self {
            v.encode(buf);
        }
    }
}

// Note: Option<T> decode is handled specially in generated code via presence bits

// ============================================================================
// Message encoding/decoding helpers
// ============================================================================

/// Encode a message field tag (index and wire type).
#[inline]
pub fn encode_tag(index: u32, wire_type: u8, buf: &mut Vec<u8>) {
    let tag = ((index as u64) << 3) | (wire_type as u64);
    encode_leb128_u64(tag, buf);
}

/// Decode a message field tag, returning (index, wire_type), or None if terminator.
#[inline]
pub fn decode_tag(buf: &mut &[u8]) -> Result<Option<(u32, u8)>, DecodeError> {
    let tag = decode_leb128_u64(buf)?;
    if tag == 0 {
        return Ok(None);
    }
    let wire_type = (tag & 0x7) as u8;
    let index = (tag >> 3) as u32;
    Ok(Some((index, wire_type)))
}

/// Encode a tagged field value (used for message fields and union variants).
/// Gets the wire type from T::WIRE_TYPE.
#[inline]
pub fn encode_field<T: Encode>(index: u32, value: &T, buf: &mut Vec<u8>) {
    let wire_type = T::WIRE_TYPE;
    encode_tag(index, wire_type, buf);
    if wire_type == wire::BYTES {
        // Write length prefix, then delimited content
        let start = buf.len();
        buf.push(0); // placeholder for length
        value.encode_delimited(buf);
        let data_len = buf.len() - start - 1;
        if data_len < 128 {
            buf[start] = data_len as u8;
        } else {
            let data = buf[start + 1..].to_vec();
            buf.truncate(start);
            encode_leb128_u64(data_len as u64, buf);
            buf.extend_from_slice(&data);
        }
    } else {
        value.encode(buf);
    }
}

/// Encode a message terminator.
#[inline]
pub fn encode_message_end(buf: &mut Vec<u8>) {
    buf.push(0);
}

/// Skip a field value based on wire type.
#[inline]
pub fn skip_by_wire_type(wire_type: u8, buf: &mut &[u8]) -> Result<(), DecodeError> {
    match wire_type {
        wire::FIXED8 => {
            if buf.is_empty() {
                return Err(DecodeError::UnexpectedEof);
            }
            *buf = &buf[1..];
            Ok(())
        }
        wire::VARINT => {
            decode_leb128_u64(buf)?;
            Ok(())
        }
        wire::FIXED32 => {
            if buf.len() < 4 {
                return Err(DecodeError::UnexpectedEof);
            }
            *buf = &buf[4..];
            Ok(())
        }
        wire::FIXED64 => {
            if buf.len() < 8 {
                return Err(DecodeError::UnexpectedEof);
            }
            *buf = &buf[8..];
            Ok(())
        }
        wire::BYTES => {
            let len = decode_leb128_u64(buf)? as usize;
            if buf.len() < len {
                return Err(DecodeError::UnexpectedEof);
            }
            *buf = &buf[len..];
            Ok(())
        }
        wire::MESSAGE => {
            // Read fields until terminator
            while let Some((_, wt)) = decode_tag(buf)? {
                skip_by_wire_type(wt, buf)?;
            }
            Ok(())
        }
        wire::UNION => {
            // Read tag and skip payload
            let tag = decode_leb128_u64(buf)?;
            let wt = (tag & 0x7) as u8;
            skip_by_wire_type(wt, buf)?;
            Ok(())
        }
        wire::UNIT => {
            // No payload to skip
            Ok(())
        }
        _ => Err(DecodeError::InvalidWireType(wire_type)),
    }
}

/// Decode a tagged field value (used for message fields and union variants).
/// Uses decode_delimited for BYTES wire type.
#[inline]
pub fn decode_field<T: Decode>(buf: &mut &[u8]) -> Result<T, DecodeError> {
    if T::WIRE_TYPE == wire::BYTES {
        // Read length prefix, then decode delimited content
        let len = decode_leb128_u64(buf)? as usize;
        T::decode_delimited(len, buf)
    } else {
        T::decode(buf)
    }
}

// ============================================================================
// Struct presence bits helpers
// ============================================================================

/// Encode presence bits for optional fields.
#[inline]
pub fn encode_presence_bits(bits: &[bool], buf: &mut Vec<u8>) {
    let num_bytes = (bits.len() + 7) / 8;
    for byte_idx in 0..num_bytes {
        let mut byte = 0u8;
        for bit_idx in 0..8 {
            let idx = byte_idx * 8 + bit_idx;
            if idx < bits.len() && bits[idx] {
                byte |= 1 << bit_idx;
            }
        }
        buf.push(byte);
    }
}

/// Decode presence bits for optional fields.
#[inline]
pub fn decode_presence_bits(count: usize, buf: &mut &[u8]) -> Result<Vec<bool>, DecodeError> {
    let num_bytes = (count + 7) / 8;
    if buf.len() < num_bytes {
        return Err(DecodeError::UnexpectedEof);
    }
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        let present = (buf[byte_idx] >> bit_idx) & 1 == 1;
        result.push(present);
    }
    *buf = &buf[num_bytes..];
    Ok(result)
}
