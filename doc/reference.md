# Volex Language Reference

Volex is a schema definition language for binary serialization. It provides a Rust-like syntax with TypeScript-style optional fields, supporting an efficient wire protocol.

## Comments

```vol
// Single-line comments start with //

/* Multi-line comments are enclosed
   in /* and */ markers */

/* They can also be used inline */ like this
```

## Primitive Types

### Integer Types

| Type  | Size     | Encoding        | Range                                                   |
| ----- | -------- | --------------- | ------------------------------------------------------- |
| `u8`  | 1 byte   | Fixed           | 0 to 255                                                |
| `i8`  | 1 byte   | Fixed           | -128 to 127                                             |
| `u16` | Variable | LEB128          | 0 to 65,535                                             |
| `u32` | Variable | LEB128          | 0 to 4,294,967,295                                      |
| `u64` | Variable | LEB128          | 0 to 18,446,744,073,709,551,615                         |
| `i16` | Variable | Zigzag + LEB128 | -32,768 to 32,767                                       |
| `i32` | Variable | Zigzag + LEB128 | -2,147,483,648 to 2,147,483,647                         |
| `i64` | Variable | Zigzag + LEB128 | -9,223,372,036,854,775,808 to 9,223,372,036,854,775,807 |

### Floating-Point Types

| Type  | Size    | Encoding               |
| ----- | ------- | ---------------------- |
| `f32` | 4 bytes | Little-endian IEEE 754 |
| `f64` | 8 bytes | Little-endian IEEE 754 |

### Other Primitives

| Type     | Encoding                 |
| -------- | ------------------------ |
| `bool`   | 1 byte (0=false, 1=true) |
| `string` | Length-prefixed UTF-8    |

## Container Types

### Arrays

Arrays use square bracket syntax:

```vol
[ElementType]
```

**Examples:**

```vol
struct Example {
    numbers: [u32]
    names: [string]
    points: [Point]
}
```

### Maps

Maps use curly brace syntax with key-value pairs:

```vol
{KeyType: ValueType}
```

Allowed map key types:

- `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`
- `bool`
- `string`
- `enum`s
- `[T]` arrays, if `T` is allowed as map key.
- `struct`s, `message`s, `union`s if all fields' types are allowed as map keys.

`f32`, `f64` and maps are disallowed.

Additionally, there's a few language-specific restrictions:

- Go: arrays and unions are disallowed.
- TypeScript: arrays, structs, messages, unions are disallowed.

**Examples:**

```vol
message Config {
    settings: {string: string} = 1
    scores: {u64: u32} = 2
}
```

## Struct

Structs are fixed-layout data structures. All fields are serialized one after another with no tags. This allows for more compact serialization, but it makes adding, removing or reordering fields a breaking change.

Fields can be made optional. When serialized, presence is indicated with a bitfield, so non-present fields use only one bit.

**Syntax:**

```vol
struct StructName {
    field_name: Type
    another_field: Type
}
```

**Examples:**

```vol
// Simple struct
struct Point {
    x: f32
    y: f32
    z: f32
}

// Empty struct
struct Empty {
}

// Nested structs
struct Line {
    start: Point
    end: Point
}

// Struct with optional field
struct OptionalData {
    id: u32
    name?: string
}
```

## Message

Messages are similar to structs except each field has an index which is used as a tag in the serialization. This allows for backwards and forwards compatibility when evolving protocols:

- Adding fields: when decoding data from a previous version of the software that had less fields, the new fields get their default value (null if optional, or zero, false, empty string/map/array otherwise).
- Removing fields: when decoding data, unknown tags are ignored.
- Reordering fields in the schema definition is not a breaking change, since it's the tag what matters.

Fields can be optional. When not present, they're omitted from the serialization.

Indices must be 1 or greater. 0 is not allowed. Indices can be non-sequential.

**Syntax:**

```vol
message MessageName {
    field_name: Type = index
    optional_field?: Type = index
}
```

**Examples:**

```vol
// Basic message
message UserProfile {
    id: u64 = 1
    username: string = 2
    email?: string = 3
    verified: bool = 4
}

// Empty message
message EmptyMessage {
}

// Message with collections
message GameState {
    player_id: u64 = 1
    inventory: [Item] = 2
    metadata?: {string: string} = 3
}
```

## Enum

Enums define a set of named variants without associated data, similar to C-style enums.

When decoding, an unknown variant results in an error. Therefore, adding variants is backwards-compatible, removing is not.

Indices must be 0 or greater, and can be non-sequential.

**Syntax:**

```vol
enum EnumName {
    VariantName = index
}
```

**Examples:**

```vol
// Sequential indices
enum Status {
    Idle = 0
    Active = 1
    Paused = 2
    Stopped = 3
}

// Non-sequential indices
enum Priority {
    Low = 0
    Medium = 5
    High = 10
    Critical = 100
}

// Single variant
enum Unit {
    Value = 0
}
```

## Union

Unions define tagged variants where each variant can carry associated data (like Rust enums or tagged unions).

When decoding, an unknown variant results in an error. Therefore, adding variants is backwards-compatible, removing is not.

Indices must be 1 or greater. 0 is not allowed. Indices can be non-sequential.

**Syntax:**

```vol
union UnionName {
    VariantName = index          // Unit variant
    VariantName(Type) = index    // Variant with data
}
```

**Examples:**

```vol
// Simple union
union Result {
    Ok(u32) = 1
    Error(string) = 2
}

// Union with unit variant
union Option {
    None = 1
    Some(u32) = 2
}

// Union with complex types
union Event {
    Click = 1
    Move(Point) = 2
    Resize(ResizeData) = 3
    Custom(GameState) = 4
}

// Nested unions
union Nested {
    Inner(InnerUnion) = 1
    Outer(string) = 2
}
```

## Service

Services define RPC interfaces with named methods. Each method has a request type, a response type, and an index for wire compatibility.

**Syntax:**

```vol
service ServiceName {
    fn method_name(RequestType) -> ResponseType = index
}
```

Method indices must be 1 or greater. 0 is not allowed. Indices can be non-sequential.

### Basic Methods

The simplest form of RPC method takes a request and returns a response:

```vol
service UserService {
    fn get_user(GetUserRequest) -> GetUserResponse = 1
    fn create_user(CreateUserRequest) -> CreateUserResponse = 2
    fn delete_user(DeleteUserRequest) -> DeleteUserResponse = 3
}

message GetUserRequest {
    id: u64 = 1
}

message GetUserResponse {
    user?: User = 1
}
```

### Streaming Methods

Methods can return a stream of responses using the `stream` keyword. The client sends one request, and the server sends multiple responses over time.

```vol
service EventService {
    fn subscribe(SubscribeRequest) -> stream Event = 1
}
```

### Complete Example

```vol
// Request/Response types
message LoginRequest {
    username: string = 1
    password: string = 2
}

message LoginResponse {
    token?: string = 1
    error?: string = 2
}

message LogoutRequest {
    token: string = 1
}

message LogoutResponse {}

message ChatMessage {
    sender: string = 1
    content: string = 2
    timestamp: u64 = 3
}

message SubscribeRequest {
    channel: string = 1
}

// Service definition
service MessagingService {
    // Simple request-response
    fn login(LoginRequest) -> LoginResponse = 1
    fn logout(LogoutRequest) -> LogoutResponse = 2

    // Server streaming
    fn subscribe(SubscribeRequest) -> stream ChatMessage = 3
}
```

### Error Handling

Error handling is not built into the service definition syntax. Instead, use union types as your response to represent success or failure:

```vol
union GetUserResponse {
    Ok(User) = 1
    NotFound = 2
    PermissionDenied(string) = 3
    InternalError(string) = 4
}

service UserService {
    fn get_user(GetUserRequest) -> GetUserResponse = 1
}
```

This approach gives you full control over error representation and keeps the schema explicit about what errors each method can return.
