# Volex RPC Transport

This document describes the wire protocol for Volex RPC services.

## Overview

The Volex RPC protocol is split into two layers:

1. **Framing layer** - Delivers discrete messages over the underlying transport
2. **Message layer** - The actual RPC protocol messages

This separation allows the message layer to work unchanged across different transports.

## Framing Layer

The framing layer is responsible for delivering discrete binary messages. Each transport implements framing differently.

### TCP / Unix Sockets

Messages are length-prefixed:

```
[length: LEB128] [payload: length bytes]
```

Multiple messages are concatenated on the stream. The receiver reads the length, then reads exactly that many bytes for the payload.

### WebSocket

Each WebSocket message contains exactly one RPC message. No additional framing is needed since WebSocket already provides message boundaries.

Use binary WebSocket frames (opcode 0x02).

### Other Transports

Any transport that can deliver discrete binary messages can be used. The only requirement is reliable, ordered delivery of complete messages.

---

# Message Layer

The message layer defines the RPC protocol messages exchanged between client and server. This layer is transport-agnostic.

## Message Format

All messages share a common header:

```
[message_type: u8] [call_id: LEB128] [payload...]
```

The `call_id` identifies which call the message belongs to. Call IDs are chosen by the client and must be unique among active calls on that connection. Once a call completes, the ID can be reused.

## Message Types

Server→Client messages use IDs 0x00-0x7F. Client→Server messages use IDs 0x80-0xFF. This separation allows bidirectional RPC over a single transport.

| Message Type  | ID   | Direction       | Description                            |
| ------------- | ---- | --------------- | -------------------------------------- |
| RESPONSE      | 0x00 | Server→Client   | Unary response (ends call)             |
| STREAM_ITEM   | 0x01 | Server→Client   | Stream item (more to come)             |
| STREAM_END    | 0x02 | Server→Client   | End of stream (ends call)              |
| ERROR         | 0x03 | Server→Client   | Error response (ends call)             |
| REQUEST       | 0x80 | Client→Server   | Request from client                    |
| CANCEL        | 0x81 | Client→Server   | Cancel an in-progress call             |

## Message Payloads

### REQUEST (0x80)

Sends a request from client to server.

```
[0x80] [call_id: LEB128] [method_index: LEB128] [body: bytes]
```

### RESPONSE (0x00)

Successful unary response. Ends the call.

```
[0x00] [call_id: LEB128] [body: bytes]
```

### STREAM_ITEM (0x01)

Stream item from server. Used for streaming methods.

```
[0x01] [call_id: LEB128] [body: bytes]
```

The server sends zero or more STREAM_ITEM messages, followed by either STREAM_END or ERROR to end the call.

### STREAM_END (0x02)

Signals successful end of a stream. Ends the call.

```
[0x02] [call_id: LEB128]
```

### ERROR (0x03)

Error response. Ends the call.

```
[0x03] [call_id: LEB128] [error_code: LEB128] [error_message: string]
```

Error codes:

| Code | Meaning            |
| ---- | ------------------ |
| 1    | Unknown method     |
| 2    | Decode error       |
| 3    | Handler error      |

The error message is a human-readable description encoded as a volex string (LEB128 length + UTF-8 bytes).

### CANCEL (0x81)

Requests cancellation of an in-progress call.

```
[0x81] [call_id: LEB128]
```

Sent by the client to abort a call. The server should stop processing and respond with an ERROR.

## Call Flows

### Unary (Request-Response)

```
Client                          Server
  |                               |
  |-------- REQUEST ------------->|
  |   call_id=1                   |
  |   method_index=1              |
  |   body=...                    |
  |                               |
  |<-------- RESPONSE ------------|
  |   call_id=1                   |
  |   body=...                    |
  |                               |
```

### Server Streaming

```
Client                          Server
  |                               |
  |-------- REQUEST ------------->|
  |   call_id=1                   |
  |   method_index=2              |
  |   body=...                    |
  |                               |
  |<------- STREAM_ITEM ----------|
  |   call_id=1                   |
  |   body=...                    |
  |                               |
  |<------- STREAM_ITEM ----------|
  |   call_id=1                   |
  |   body=...                    |
  |                               |
  |<------- STREAM_END -----------|
  |   call_id=1                   |
  |                               |
```

### Error

```
Client                          Server
  |                               |
  |-------- REQUEST ------------->|
  |   call_id=1                   |
  |   method_index=99             |
  |   body=...                    |
  |                               |
  |<---------- ERROR -------------|
  |   call_id=1                   |
  |   error_code=1                |
  |   error_message="unknown..."  |
  |                               |
```

## Multiplexing

Multiple calls can be in flight simultaneously on a single connection. Messages from different calls can be interleaved. Each message includes the `call_id` to identify which call it belongs to.

```
Client                          Server
  |                               |
  |-- REQUEST (call_id=1) ------->|
  |-- REQUEST (call_id=2) ------->|
  |<-- STREAM_ITEM (call_id=1) ---|
  |<-- RESPONSE (call_id=2) ------|
  |<-- STREAM_ITEM (call_id=1) ---|
  |<-- STREAM_END (call_id=1) ----|
  |                               |
```

## Method Resolution

Methods are identified by their index as declared in the schema:

```vol
service UserService {
    fn get_user(GetUserRequest) -> GetUserResponse = 1
    fn list_users(ListUsersRequest) -> stream User = 2
}
```

The `method_index` in REQUEST frames corresponds to these indices. Unknown method indices result in an ERROR message with code=1.

## Connection Lifecycle

1. **Establish**: Client connects to server over the transport layer
2. **Active**: Client and server exchange frames for RPC calls
3. **Close**: Either side can close the connection
   - Graceful: Wait for in-flight calls to complete, then close
   - Immediate: Close connection; in-flight calls are implicitly cancelled

## Error Handling

Protocol-level errors are reported via the ERROR message. Application-level errors should be encoded in the response message itself (using union types as shown in the language reference).

| Scenario              | Handling                    |
| --------------------- | --------------------------- |
| Unknown method        | ERROR with code=1           |
| Malformed request     | ERROR with code=2           |
| Handler error         | ERROR with code=3           |
| Connection closed     | Implicit cancellation       |

## Example Wire Encoding

Given this service:

```vol
message EchoRequest {
    text: string = 1
}

message EchoResponse {
    text: string = 1
}

service EchoService {
    fn echo(EchoRequest) -> EchoResponse = 1
}
```

A call to `echo` with `{ text: "hello" }`:

**REQUEST message:**
```
Message type: 0x80 (REQUEST)
Call ID:      0x01 (1)
Method index: 0x01 (1)
Body:         0c 05 68 65 6c 6c 6f 00
              │  │  └─────────────┴─ "hello" + message terminator
              │  └─ string length 5
              └─ tag: field=1, wire=BYTES
```

**RESPONSE message:**
```
Message type: 0x00 (RESPONSE)
Call ID:      0x01 (1)
Body:         0c 05 68 65 6c 6c 6f 00
              └─ same encoding as request
```

On TCP, each message would be prefixed with its length (LEB128). On WebSocket, each message is sent as a separate binary frame.
