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

## Message Types

Each message starts with a message type byte:

| Message Type     | ID  | Description                              |
| ---------------- | --- | ---------------------------------------- |
| REQUEST          | 1   | Request from client to server            |
| RESPONSE         | 2   | Final response from server (ends call)   |
| RESPONSE_STREAM  | 3   | Stream item from server (more to come)   |
| CANCEL           | 4   | Cancel an in-progress call               |

## Call IDs

Each call is identified by a `call_id` (u32). Call IDs are chosen by the client and must be unique among active calls on that connection. Once a call completes (RESPONSE or CANCEL), the ID can be reused.

## Message Formats

### REQUEST

Sends a request from client to server.

```
[message_type: u8 = 1]
[call_id: LEB128]
[method_index: LEB128]
[body: bytes]
```

### RESPONSE

Final response from server. Ends the call.

```
[message_type: u8 = 2]
[call_id: LEB128]
[status: u8]
[body: bytes]           // Only if status = 0 (OK)
```

Status codes:

| Status | Meaning            |
| ------ | ------------------ |
| 0      | OK                 |
| 1      | Unknown method     |
| 2      | Decode error       |
| 3      | Internal error     |
| 4      | Cancelled          |

For unary methods, the body contains the response. For streaming methods, RESPONSE signals the end of the stream (body is empty).

### RESPONSE_STREAM

Stream item from server. Used for streaming methods.

```
[message_type: u8 = 3]
[call_id: LEB128]
[body: bytes]
```

The server sends zero or more RESPONSE_STREAM messages, followed by a final RESPONSE to end the call.

### CANCEL

Requests cancellation of an in-progress call.

```
[message_type: u8 = 4]
[call_id: LEB128]
```

Sent by the client to abort a call. The server should stop processing and respond with status=4 (Cancelled).

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
  |   status=0                    |
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
  |<--- RESPONSE_STREAM ----------|
  |   call_id=1                   |
  |   body=...                    |
  |                               |
  |<--- RESPONSE_STREAM ----------|
  |   call_id=1                   |
  |   body=...                    |
  |                               |
  |<-------- RESPONSE ------------|
  |   call_id=1                   |
  |   status=0                    |
  |                               |
```

## Multiplexing

Multiple calls can be in flight simultaneously on a single connection. Messages from different calls can be interleaved. Each message includes the `call_id` to identify which call it belongs to.

```
Client                          Server
  |                               |
  |-- REQUEST (call_id=1) ------->|
  |-- REQUEST (call_id=2) ------->|
  |<-- RESPONSE_STREAM (call_id=1)|
  |<-- RESPONSE (call_id=2) ------|
  |<-- RESPONSE_STREAM (call_id=1)|
  |<-- RESPONSE (call_id=1) ------|
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

The `method_index` in REQUEST frames corresponds to these indices. Unknown method indices result in a RESPONSE with status=1 (Unknown method).

## Connection Lifecycle

1. **Establish**: Client connects to server over the transport layer
2. **Active**: Client and server exchange frames for RPC calls
3. **Close**: Either side can close the connection
   - Graceful: Wait for in-flight calls to complete, then close
   - Immediate: Close connection; in-flight calls are implicitly cancelled

## Error Handling

Errors are reported via the RESPONSE frame's status field. Application-level errors should be encoded in the response message itself (using union types as shown in the language reference), while protocol-level errors use status codes.

| Scenario | Handling |
| -------- | -------- |
| Unknown method | RESPONSE with status=1 |
| Malformed request | RESPONSE with status=2 |
| Server internal error | RESPONSE with status=3 |
| Client cancellation | RESPONSE with status=4 |
| Connection closed | Implicit cancellation of all in-flight calls |

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
Message type: 0x01 (REQUEST)
Call ID:      0x01 (1)
Method index: 0x01 (1)
Body:         0c 05 68 65 6c 6c 6f 00
              │  │  └─────────────┴─ "hello" + message terminator
              │  └─ string length 5
              └─ tag: field=1, wire=BYTES
```

**RESPONSE message:**
```
Message type: 0x02 (RESPONSE)
Call ID:      0x01 (1)
Status:       0x00 (OK)
Body:         0c 05 68 65 6c 6c 6f 00
              └─ same encoding as request
```

On TCP, each message would be prefixed with its length (LEB128). On WebSocket, each message is sent as a separate binary frame.
