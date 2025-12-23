# Wire Format Specification

## LEB128 Encoding

LEB128 (Little Endian Base 128) is a variable-length encoding for unsigned integers. Each byte uses 7 bits for data and 1 bit (MSB) to indicate continuation.

**Algorithm:**

- While value >= 128: emit `(value & 0x7F) | 0x80`, then `value >>= 7`
- Emit final byte `value & 0x7F` (no continuation bit)

**Examples:**

| Value | Hex bytes  | Explanation                                            |
| ----- | ---------- | ------------------------------------------------------ |
| 0     | `00`       | Single byte, no continuation                           |
| 1     | `01`       | Single byte                                            |
| 127   | `7f`       | Maximum single-byte value                              |
| 128   | `80 01`    | `0x80` (128 with continuation) + `0x01`                |
| 300   | `ac 02`    | `0xAC` (44 + continuation) + `0x02` → 44 + 2×128 = 300 |
| 16384 | `80 80 01` | Three bytes: 0 + 0×128 + 1×16384                       |

## Zigzag Encoding

Zigzag encoding maps signed integers to unsigned integers, ensuring small absolute values (positive or negative) produce small encoded values.

**Formulas:**

- Encode: `(n << 1) ^ (n >> 63)`
- Decode: `(n >> 1) ^ -(n & 1)`

**Examples:**

| Signed value | Zigzag unsigned | LEB128 bytes |
| ------------ | --------------- | ------------ |
| 0            | 0               | `00`         |
| -1           | 1               | `01`         |
| 1            | 2               | `02`         |
| -2           | 3               | `03`         |
| 64           | 128             | `80 01`      |
| -64          | 127             | `7f`         |
| -65          | 129             | `81 01`      |

## Type Summary

| Type     | Wire Type | Fixed Size | Encoding                   |
| -------- | --------- | ---------- | -------------------------- |
| `bool`   | FIXED8    | 1          | 0 = false, 1 = true        |
| `u8`     | FIXED8    | 1          | Single byte                |
| `i8`     | FIXED8    | 1          | Single byte (as unsigned)  |
| `u16`    | VARINT    | -          | LEB128                     |
| `u32`    | VARINT    | -          | LEB128                     |
| `u64`    | VARINT    | -          | LEB128                     |
| `i16`    | VARINT    | -          | Zigzag + LEB128            |
| `i32`    | VARINT    | -          | Zigzag + LEB128            |
| `i64`    | VARINT    | -          | Zigzag + LEB128            |
| `f32`    | FIXED32   | 4          | Little-endian IEEE 754     |
| `f64`    | FIXED64   | 8          | Little-endian IEEE 754     |
| `string` | BYTES     | -          | Length-prefixed UTF-8      |
| `[T]`    | BYTES     | -          | Length-prefixed elements   |
| `{K: V}` | BYTES     | -          | Length-prefixed entries    |
| struct   | BYTES     | see below  | Concatenated fields        |
| message  | MESSAGE   | -          | Tagged fields + terminator |
| enum     | VARINT    | -          | Variant index              |
| union    | UNION     | -          | Tagged payload             |

\* Structs have a fixed size if all fields are fixed-size and there are no optional fields.

## Fixed-size Types

A type is considered "fixed size" if it gets encoded to a constant amount of bytes, regardless of the value.

These types are fixed size types:

- 'u8', 'i8', 'bool': 1 byte.
- 'f32': 4 bytes.
- 'f64': 8 bytes.
- 'struct's: fixed size if all their fields are fixed size and non-optional. The size is the sum of al the field's sizes.

## Wire Types

Every value in a message or union field is encoded with a **wire type** that indicates how to skip the data without knowing the schema. This allows decoders to skip unknown fields.

| Wire Type | ID  | Description            | How to skip                              |
| --------- | --- | ---------------------- | ---------------------------------------- |
| `FIXED8`  | 0   | Single byte            | Skip 1 byte                              |
| `VARINT`  | 1   | LEB128-encoded integer | Read LEB128                              |
| `FIXED32` | 2   | 4 bytes (f32)          | Skip 4 bytes                             |
| `FIXED64` | 3   | 8 bytes (f64)          | Skip 8 bytes                             |
| `BYTES`   | 4   | Length-prefixed bytes  | Read LEB128 length, skip that many bytes |
| `MESSAGE` | 5   | Nested message         | Read fields until terminator             |
| `UNION`   | 6   | Nested union           | Read tag+wire, skip payload              |
| `UNIT`    | 7   | Unit (no payload)      | Nothing to skip                          |

## String

**Standalone:**

```
[length: LEB128] [utf8_bytes: length bytes]
```

**As field in message/union:** The field encoding already provides the byte length. The string data is just the raw UTF-8 bytes (no inner length prefix):

```
[utf8_bytes: length bytes]
```

## Array `[T]`

**Standalone:**

```
[count: LEB128] [element₀: T] [element₁: T] ... [elementₙ: T]
```

**As field in message/union:** The field encoding already provides the byte length. The encoding depends on whether `T` is fixed-size or not.

- **Fixed-size elements**: No count prefix needed - count is inferred from `byte_length / element_size`:

  ```
  [element₀: T] [element₁: T] ... [elementₙ: T]
  ```

- **Variable-size elements** (e.g., `[string]`, `[Message]`): Count prefix is required:
  ```
  [count: LEB128] [element₀: T] [element₁: T] ... [elementₙ: T]
  ```

The outer length allows skipping the entire array without parsing elements.

## Map `{K: V}`

**Standalone:**

```
[count: LEB128] [key₀: K] [value₀: V] [key₁: K] [value₁: V] ...
```

**As field in message/union:** The field encoding already provides the byte length. The encoding depends on whether `K`, `V` are fixed-size or not.

- **Fixed-size entries** (both K and V are fixed-size): No count prefix - count is inferred from `length / (key_size + value_size)`:

  ```
  [key₀: K] [value₀: V] [key₁: K] [value₁: V] ...
  ```

- **Variable-size entries**: Count prefix is required:
  ```
  [count: LEB128] [key₀: K] [value₀: V] [key₁: K] [value₁: V] ...
  ```

## Struct

Structs have a **fixed layout** - fields are encoded in declaration order. No field tags or length prefixes needed.

### Structs without optional fields

Simply concatenate all field values:

```
[field₀: T₀] [field₁: T₁] ... [fieldₙ: Tₙ]
```

**Example:**

```
struct Point {
    x: f32
    y: f32
    z: f32
}
```

Wire format: `[x: 4 bytes] [y: 4 bytes] [z: 4 bytes]` = 12 bytes total

### Structs with optional fields

If a struct has optional fields, a **presence bitfield** precedes the field data. Each optional field uses 1 bit to indicate whether it's present.

```
[presence_bits: ⌈optional_count/8⌉ bytes] [fields...]
```

- Bits are packed LSB-first: bit 0 of byte 0 = first optional field, bit 1 = second, etc.
- Required fields are always encoded
- Optional fields are only encoded if their presence bit is set

**Example:**

```
struct Item {
    id: u32           // required (field 0)
    quantity: u16     // required (field 1)
    durability?: u8   // optional (field 2, opt index 0)
}
```

Encoding `{ id: 5, quantity: 10, durability: Some(100) }`:

```
[01]                  // presence bits: bit 0 = 1 (durability present)
[05] [0a] [64]        // id=5, quantity=10, durability=100
```

Encoding `{ id: 5, quantity: 10, durability: None }`:

```
[00]                  // presence bits: bit 0 = 0 (durability absent)
[05] [0a]             // id=5, quantity=10 (no durability)
```

## Message

Messages support **backwards compatibility** through tagged fields. Unknown fields are skipped using wire type information.

```
[field₀] [field₁] ... [fieldₙ] [00]
```

Each field:

```
[tag: LEB128] [value]
```

Where `tag = (index << 3) | wire_type`:

- `index`: The field's declared index (from `= N`), **must be >= 1**
- `wire_type`: One of the wire type IDs (0-5)

The message is terminated by a `[00]` byte (tag with index=0).

**Rules:**

- **Field indices must start at 1** (index 0 is reserved for termination)
- Fields are encoded in index order (ascending)
- Optional fields with no value are omitted entirely
- Decoders skip fields with unknown indices using wire type (forwards compatibility)
- Decoders use default values for missing fields (backwards compatibility)

**Default values:**

- Integers: `0`
- Floats: `0.0`
- Bool: `false`
- String: `""`
- Array: `[]`
- Map: `{}`

**Example:**

```
message UserProfile {
    id: u64 = 1
    username: string = 2
    email?: string = 3
}
```

Encoding `{ id: 42, username: "alice" }` (email not set):

```
[08]                    // tag: index=1, wire=VARINT(0) → (1<<3)|0 = 8
[2a]                    // value: 42
[13]                    // tag: index=2, wire=BYTES(3) → (2<<3)|3 = 19
[05] "alice"            // value: length=5, "alice"
[00]                    // terminator
```

## Enum

Enums are simple tags with no payload. Encoded as a single LEB128 integer representing the variant's index.

```
[index: LEB128]
```

When used as a message/union field, enums use wire type `VARINT`.

**Example:**

```
enum PlayerStatus {
    Idle = 0
    Moving = 1
    Fighting = 2
    Dead = 3
}
```

`Moving` encodes as: `[01]`

## Union

Unions are tagged values. The tag encodes both the variant index and the wire type of the payload.

```
[tag: LEB128] [value]
```

Where `tag = (index << 3) | wire_type`:

- `index`: The variant's declared index (from `= N`), **must be >= 1**
- `wire_type`: Wire type of the payload

For unit variants (no payload), use wire type `UNIT` (no payload bytes):

```
[tag: LEB128]
```

**Example:**

```
union Result {
    Ok(u32) = 1
    Error(string) = 2
}
```

Encoding `Ok(42)`:

```
[08]                    // tag: index=1, wire=VARINT(0) → (1<<3)|0 = 8
[2a]                    // value: 42
```

Encoding `Error("not found")`:

```
[13]                    // tag: index=2, wire=BYTES(3) → (2<<3)|3 = 19
[09] "not found"        // value: length=9, "not found"
```

**Example with unit variant:**

```
union Event {
    Click = 1
    Move(Point) = 2
}
```

Encoding `Click`:

```
[0f]                    // tag: index=1, wire=UNIT(7) → (1<<3)|7 = 15
                        // (no payload)
```
