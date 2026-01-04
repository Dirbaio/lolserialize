// Volex RPC Client Infrastructure for TypeScript
// This implements the client-side RPC protocol as defined in doc/rpc_transport.md

import { Buf, encodeVarint, decodeVarint, flattenBuf } from './volex.js';

// ============================================================================
// RPC Message Types
// ============================================================================

// Server -> Client message types (0x00-0x7F)
const RPC_TYPE_RESPONSE = 0x00;
const RPC_TYPE_STREAM_ITEM = 0x01;
const RPC_TYPE_STREAM_END = 0x02;
const RPC_TYPE_ERROR = 0x03;

// Client -> Server message types (0x80-0xFF)
const RPC_TYPE_REQUEST = 0x80;
const RPC_TYPE_CANCEL = 0x81;

// ============================================================================
// RPC Errors
// ============================================================================

export const ERR_CODE_UNKNOWN_METHOD = 1;
export const ERR_CODE_DECODE_ERROR = 2;
export const ERR_CODE_HANDLER_ERROR = 3;

export class RpcError extends Error {
  code: number;

  constructor(code: number, message: string) {
    super(message);
    this.code = code;
    this.name = 'RpcError';
  }

  isStreamClosed(): boolean {
    return this.code === 0 && this.message === 'stream closed';
  }

  isStreamCancelled(): boolean {
    return this.code === 0 && this.message === 'stream cancelled';
  }

  static streamClosed(): RpcError {
    return new RpcError(0, 'stream closed');
  }

  static streamCancelled(): RpcError {
    return new RpcError(0, 'stream cancelled');
  }
}

// ============================================================================
// Transport
// ============================================================================

export interface Transport {
  send(data: Uint8Array): Promise<void>;
  recv(): Promise<Uint8Array>;
  close(): void;
}

// ============================================================================
// TCP Transport (for Node.js)
// ============================================================================

export class TcpTransport implements Transport {
  private socket: any; // net.Socket
  private buffer: Uint8Array = new Uint8Array(0);
  private waiters: Array<{
    resolve: (value: Uint8Array) => void;
    reject: (error: Error) => void;
  }> = [];
  private closed = false;
  private error: Error | null = null;

  constructor(socket: any) {
    this.socket = socket;

    // Disable Nagle's algorithm for lower latency
    socket.setNoDelay(true);

    socket.on('data', (data: Uint8Array) => {
      // Append to buffer
      const newBuf = new Uint8Array(this.buffer.length + data.length);
      newBuf.set(this.buffer);
      newBuf.set(data, this.buffer.length);
      this.buffer = newBuf;

      // Try to process complete messages
      this.processBuffer();
    });

    socket.on('close', () => {
      this.closed = true;
      const err = new Error('connection closed');
      this.error = err;
      // Reject all pending waiters
      for (const waiter of this.waiters) {
        waiter.reject(err);
      }
      this.waiters = [];
    });

    socket.on('error', (err: Error) => {
      this.closed = true;
      this.error = err;
      // Reject all pending waiters
      for (const waiter of this.waiters) {
        waiter.reject(err);
      }
      this.waiters = [];
    });
  }

  private processBuffer(): void {
    while (this.waiters.length > 0) {
      // Try to read a complete LEB128 length + payload
      const buf = new Buf(this.buffer, 0);

      // Check if we have enough bytes to read the length
      if (buf.data.length === 0) {
        break;
      }

      // Try to decode length
      let length: number;
      const startOffset = buf.offset;
      try {
        length = decodeVarint(buf);
      } catch {
        // Not enough bytes for length
        break;
      }

      const headerLen = buf.offset - startOffset;
      const totalLen = headerLen + length;

      if (this.buffer.length < totalLen) {
        // Not enough bytes for payload
        break;
      }

      // Extract the message
      const payload = this.buffer.slice(headerLen, totalLen);
      this.buffer = this.buffer.slice(totalLen);

      // Resolve the first waiter
      const waiter = this.waiters.shift()!;
      waiter.resolve(payload);
    }
  }

  async send(data: Uint8Array): Promise<void> {
    if (this.closed) {
      throw this.error || new Error('connection closed');
    }

    // Encode length prefix as LEB128
    const lenBuf: Uint8Array[] = [];
    encodeVarint(data.length, lenBuf);
    const lenBytes = flattenBuf(lenBuf);

    // Combine length and data
    const packet = new Uint8Array(lenBytes.length + data.length);
    packet.set(lenBytes);
    packet.set(data, lenBytes.length);

    return new Promise((resolve, reject) => {
      this.socket.write(packet, (err?: Error) => {
        if (err) {
          reject(err);
        } else {
          resolve();
        }
      });
    });
  }

  async recv(): Promise<Uint8Array> {
    if (this.closed) {
      throw this.error || new Error('connection closed');
    }

    // Check if we already have a complete message in the buffer
    if (this.buffer.length > 0) {
      const buf = new Buf(this.buffer, 0);
      try {
        const length = decodeVarint(buf);
        const headerLen = buf.offset;
        const totalLen = headerLen + length;

        if (this.buffer.length >= totalLen) {
          const payload = this.buffer.slice(headerLen, totalLen);
          this.buffer = this.buffer.slice(totalLen);
          return payload;
        }
      } catch {
        // Not enough bytes
      }
    }

    // Wait for more data
    return new Promise((resolve, reject) => {
      this.waiters.push({ resolve, reject });
    });
  }

  close(): void {
    if (!this.closed) {
      this.closed = true;
      this.socket.end();
    }
  }
}

// ============================================================================
// Client Infrastructure
// ============================================================================

type StreamEvent =
  | { type: 'item'; data: Uint8Array }
  | { type: 'end' }
  | { type: 'error'; error: RpcError };

interface PendingUnary {
  type: 'unary';
  resolve: (data: Uint8Array) => void;
  reject: (error: RpcError) => void;
}

interface PendingStream {
  type: 'stream';
  queue: StreamEvent[];
  waiters: Array<(event: StreamEvent) => void>;
}

type PendingRequest = PendingUnary | PendingStream;

export class ClientBase {
  private transport: Transport;
  private nextId = 1;
  private pending = new Map<number, PendingRequest>();
  private running = false;
  private runPromise: Promise<void> | null = null;
  private recvError: Error | null = null;

  constructor(transport: Transport) {
    this.transport = transport;
  }

  async run(): Promise<void> {
    if (this.running) {
      throw new Error('run() already called');
    }
    this.running = true;

    try {
      while (true) {
        const data = await this.transport.recv();
        this.handleMessage(data);
      }
    } catch (e) {
      this.recvError = e as Error;
      // Notify all pending requests
      const err = new RpcError(0, (e as Error).message);
      for (const [, req] of this.pending) {
        if (req.type === 'unary') {
          req.reject(err);
        } else {
          const event: StreamEvent = { type: 'error', error: err };
          for (const waiter of req.waiters) {
            waiter(event);
          }
          req.waiters = [];
          req.queue.push(event);
        }
      }
      this.pending.clear();
      throw e;
    }
  }

  private handleMessage(data: Uint8Array): void {
    const buf = new Buf(data, 0);

    if (data.length === 0) {
      return; // Invalid message
    }

    const msgType = data[0];
    buf.offset = 1;

    // Decode request ID
    const requestId = decodeVarint(buf);

    const req = this.pending.get(requestId);
    if (!req) {
      return; // Unknown request ID
    }

    switch (msgType) {
      case RPC_TYPE_RESPONSE: {
        if (req.type === 'unary') {
          const payload = data.slice(buf.offset);
          this.pending.delete(requestId);
          req.resolve(payload);
        }
        break;
      }

      case RPC_TYPE_STREAM_ITEM: {
        if (req.type === 'stream') {
          const payload = data.slice(buf.offset);
          const event: StreamEvent = { type: 'item', data: payload };
          if (req.waiters.length > 0) {
            const waiter = req.waiters.shift()!;
            waiter(event);
          } else {
            req.queue.push(event);
          }
        }
        break;
      }

      case RPC_TYPE_STREAM_END: {
        if (req.type === 'stream') {
          this.pending.delete(requestId);
          const event: StreamEvent = { type: 'end' };
          if (req.waiters.length > 0) {
            const waiter = req.waiters.shift()!;
            waiter(event);
          } else {
            req.queue.push(event);
          }
        }
        break;
      }

      case RPC_TYPE_ERROR: {
        const errCode = decodeVarint(buf);
        const errLen = decodeVarint(buf);
        const errBytes = data.slice(buf.offset, buf.offset + errLen);
        const errMsg = new TextDecoder().decode(errBytes);
        const error = new RpcError(errCode, errMsg);

        this.pending.delete(requestId);

        if (req.type === 'unary') {
          req.reject(error);
        } else {
          const event: StreamEvent = { type: 'error', error };
          if (req.waiters.length > 0) {
            const waiter = req.waiters.shift()!;
            waiter(event);
          } else {
            req.queue.push(event);
          }
        }
        break;
      }
    }
  }

  async callUnary(methodIndex: number, payload: Uint8Array): Promise<Uint8Array> {
    if (this.recvError) {
      throw new RpcError(0, this.recvError.message);
    }

    const requestId = this.nextId++;

    // Build request message
    const buf: Uint8Array[] = [];
    buf.push(new Uint8Array([RPC_TYPE_REQUEST]));
    encodeVarint(requestId, buf);
    encodeVarint(methodIndex, buf);
    buf.push(payload);
    const packet = flattenBuf(buf);

    // Create promise for response
    const responsePromise = new Promise<Uint8Array>((resolve, reject) => {
      this.pending.set(requestId, { type: 'unary', resolve, reject });
    });

    // Send request
    try {
      await this.transport.send(packet);
    } catch (e) {
      this.pending.delete(requestId);
      throw new RpcError(0, (e as Error).message);
    }

    // Wait for response
    return responsePromise;
  }

  async callStream(methodIndex: number, payload: Uint8Array): Promise<StreamReceiver> {
    if (this.recvError) {
      throw new RpcError(0, this.recvError.message);
    }

    const requestId = this.nextId++;

    // Build request message
    const buf: Uint8Array[] = [];
    buf.push(new Uint8Array([RPC_TYPE_REQUEST]));
    encodeVarint(requestId, buf);
    encodeVarint(methodIndex, buf);
    buf.push(payload);
    const packet = flattenBuf(buf);

    // Create pending stream
    const pendingStream: PendingStream = {
      type: 'stream',
      queue: [],
      waiters: [],
    };
    this.pending.set(requestId, pendingStream);

    // Send request
    try {
      await this.transport.send(packet);
    } catch (e) {
      this.pending.delete(requestId);
      throw new RpcError(0, (e as Error).message);
    }

    return new StreamReceiver(
      requestId,
      pendingStream,
      this.pending,
      this.transport,
    );
  }

  private async sendCancel(requestId: number): Promise<void> {
    const buf: Uint8Array[] = [];
    buf.push(new Uint8Array([RPC_TYPE_CANCEL]));
    encodeVarint(requestId, buf);
    const packet = flattenBuf(buf);
    try {
      await this.transport.send(packet);
    } catch {
      // Best effort
    }
  }
}

export class StreamReceiver {
  private requestId: number;
  private pending: PendingStream;
  private allPending: Map<number, PendingRequest>;
  private transport: Transport;
  private cancelled = false;

  constructor(
    requestId: number,
    pending: PendingStream,
    allPending: Map<number, PendingRequest>,
    transport: Transport,
  ) {
    this.requestId = requestId;
    this.pending = pending;
    this.allPending = allPending;
    this.transport = transport;
  }

  async recv(): Promise<Uint8Array> {
    if (this.cancelled) {
      throw RpcError.streamCancelled();
    }

    // Check queue first
    if (this.pending.queue.length > 0) {
      const event = this.pending.queue.shift()!;
      return this.handleEvent(event);
    }

    // Wait for event
    const event = await new Promise<StreamEvent>((resolve) => {
      this.pending.waiters.push(resolve);
    });

    return this.handleEvent(event);
  }

  private handleEvent(event: StreamEvent): Uint8Array {
    switch (event.type) {
      case 'item':
        return event.data;
      case 'end':
        throw RpcError.streamClosed();
      case 'error':
        throw event.error;
    }
  }

  async cancel(): Promise<void> {
    if (this.cancelled) {
      return;
    }
    this.cancelled = true;

    this.allPending.delete(this.requestId);

    // Wake up any waiting recv() calls with a cancellation event
    const cancelEvent: StreamEvent = { type: 'error', error: RpcError.streamCancelled() };
    for (const waiter of this.pending.waiters) {
      waiter(cancelEvent);
    }
    this.pending.waiters = [];

    // Send cancel message
    const buf: Uint8Array[] = [];
    buf.push(new Uint8Array([RPC_TYPE_CANCEL]));
    encodeVarint(this.requestId, buf);
    const packet = flattenBuf(buf);
    try {
      await this.transport.send(packet);
    } catch {
      // Best effort
    }
  }
}
