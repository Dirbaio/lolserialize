// Example schema for lolserialize
// Syntax is Rust-like with TypeScript-style optional fields (name?: Type)

// Simple struct with fixed layout
// All fields are always present in serialized form
struct Point {
    x: f32
    y: f32
    z: f32
}

// Message with optional fields
// Optional fields can be omitted from serialization if not present
// Each field has an index for backwards compatibility (protobuf-style)
// Indices must be >= 1 (0 is reserved for wire format termination)
message UserProfile {
    id: u64 = 1
    username: string = 2
    email?: string = 3           // Optional field
    age?: u32 = 4
    verified: bool = 5
    score: f64 = 6
    metadata?: Metadata = 7      // Optional nested message
}

// Nested message example
message Metadata {
    created_at: u64 = 1
    updated_at: u64 = 2
    tags: [string] = 3           // Array syntax with []
}

// Enum is just a list of named tags (no data)
// Enum indices can start at 0
enum PlayerStatus {
    Idle = 0
    Moving = 1
    Fighting = 2
    Dead = 3
}

// Union can contain data for each variant
// Union indices must be >= 1 (0 is reserved)
union Result {
    Ok(u32) = 1
    Error(string) = 2
}

// Union with mixed variants
union Event {
    Click = 1
    Move(Point) = 2
    Resize(ResizeData) = 3
    Custom(GameState) = 4
}

struct ResizeData {
    width: u32
    height: u32
}

// Complex message demonstrating all features
message GameState {
    player_id: u64 = 1
    position: Point = 2
    health: u32 = 3
    max_health: u32 = 4
    inventory: [Item] = 5        // Array of items
    active_quest?: Quest = 6     // Optional
    status: PlayerStatus = 7     // Enum - just tag value on wire
    metadata?: {string: string} = 8  // Map syntax with {K: V}
}

struct Item {
    id: u32
    quantity: u16
    durability?: u8
}

message Quest {
    id: u32 = 1
    title: string = 2
    description?: string = 3
    progress: u32 = 4
    total: u32 = 5
    rewards: [Item] = 6          // Array syntax
}

// Struct for coordinate data (always fully serialized)
struct Vector3 {
    x: f64
    y: f64
    z: f64
}

// Message for network packets
message NetworkPacket {
    timestamp: u64 = 1
    sequence: u32 = 2
    payload: PayloadType = 3
    checksum?: u32 = 4
}

union PayloadType {
    Ping = 1
    Pong = 2
    Data([u8]) = 3               // Byte array
    Text(string) = 4
    Structured(GameState) = 5
}
