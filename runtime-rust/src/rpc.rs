//! RPC infrastructure for volex services.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::rc::Rc;

use tokio::sync::{mpsc, oneshot};

use crate::{DecodeError, Encode, decode_leb128_u64, encode_leb128_u64};

// ============================================================================
// Utilities
// ============================================================================

/// A guard that runs a closure when dropped.
struct OnDrop<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> OnDrop<F> {
    fn new(f: F) -> Self {
        Self(Some(f))
    }

    /// Defuses the guard, preventing the closure from running on drop.
    fn defuse(&mut self) {
        self.0.take();
    }
}

impl<F: FnOnce()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

// ============================================================================
// RPC Errors
// ============================================================================

/// Error codes for RPC errors.
pub const ERR_CODE_UNKNOWN_METHOD: u32 = 1;
pub const ERR_CODE_DECODE_ERROR: u32 = 2;
pub const ERR_CODE_HANDLER_ERROR: u32 = 3;

/// RPC error type.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: u32,
    pub message: String,
}

impl RpcError {
    /// Creates a new RPC error.
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Creates an RPC error from a decode error.
    pub fn decode(e: DecodeError) -> Self {
        Self {
            code: ERR_CODE_DECODE_ERROR,
            message: format!("decode error: {}", e),
        }
    }

    /// Creates an RPC error for stream closed.
    pub fn stream_closed() -> Self {
        Self {
            code: 0,
            message: "stream closed".to_string(),
        }
    }

    /// Returns true if this is a stream closed error.
    pub fn is_stream_closed(&self) -> bool {
        self.code == 0 && self.message == "stream closed"
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

// ============================================================================
// RPC Message Types
// ============================================================================

// Server -> Client message types (0x00-0x7F)
const RPC_TYPE_RESPONSE: u8 = 0x00;
const RPC_TYPE_STREAM_ITEM: u8 = 0x01;
const RPC_TYPE_STREAM_END: u8 = 0x02;
const RPC_TYPE_ERROR: u8 = 0x03;

// Client -> Server message types (0x80-0xFF)
const RPC_TYPE_REQUEST: u8 = 0x80;
const RPC_TYPE_CANCEL: u8 = 0x81;

// ============================================================================
// Transport
// ============================================================================

/// Transport trait for sending and receiving binary messages.
pub trait Transport {
    /// Sends a binary message.
    fn send(&self, data: Vec<u8>) -> impl Future<Output = Result<(), RpcError>>;
    /// Receives a binary message.
    fn recv(&self) -> impl Future<Output = Result<Vec<u8>, RpcError>>;
}

// ============================================================================
// Server Infrastructure
// ============================================================================

/// Stream sender for streaming responses (server-side, typed).
///
/// Wraps a `ServerStreamSender` to provide typed sending.
pub struct StreamSender<T: Encode> {
    base: StreamSenderBase,
    _phantom: PhantomData<T>,
}

impl<T: Encode> StreamSender<T> {
    /// Creates a new typed stream sender.
    pub fn new(base: StreamSenderBase) -> Self {
        Self {
            base,
            _phantom: PhantomData,
        }
    }

    /// Sends an item to the stream.
    ///
    /// Returns an error if the stream has been closed (e.g., due to transport error).
    pub async fn send(&self, item: T) -> Result<(), RpcError> {
        let mut buf = Vec::new();
        item.encode(&mut buf);
        self.base.send(buf).await
    }

    /// Marks the stream as finished with an error.
    /// After calling this, no StreamEnd will be sent on drop.
    pub async fn error(self, code: u32, message: &str) {
        self.base.error(code, message).await;
    }
}

/// Type alias for unary method handlers.
type UnaryHandler = Box<dyn Fn(Vec<u8>) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, RpcError>>>>>;

/// Type alias for streaming method handlers.
/// Stream handlers do not return a Result - errors must be reported via ServerStreamSender::error.
type StreamHandler = Box<dyn Fn(Vec<u8>, StreamSenderBase) -> std::pin::Pin<Box<dyn Future<Output = ()>>>>;

/// Method handler enum.
enum MethodHandler {
    Unary(UnaryHandler),
    Stream(StreamHandler),
}

/// Stream sender for server-side streaming responses.
/// Sends items immediately to the transport. Sends StreamEnd on drop.
pub struct StreamSenderBase {
    call_id: u64,
    tx: mpsc::Sender<Vec<u8>>,
    finished: bool,
}

impl StreamSenderBase {
    /// Sends a stream item (already encoded).
    pub async fn send(&self, payload: Vec<u8>) -> Result<(), RpcError> {
        let packet = Self::make_stream_item_packet(self.call_id, payload);
        self.tx
            .send(packet)
            .await
            .map_err(|_| RpcError::new(0, "transport closed"))
    }

    /// Marks the stream as finished with an error.
    /// After calling this, no StreamEnd will be sent on drop.
    pub async fn error(mut self, code: u32, message: &str) {
        self.finished = true;
        let packet = Self::make_error_packet(self.call_id, code, message);
        let _ = self.tx.send(packet).await;
    }

    fn make_stream_item_packet(request_id: u64, payload: Vec<u8>) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(RPC_TYPE_STREAM_ITEM);
        encode_leb128_u64(request_id, &mut buf);
        buf.extend_from_slice(&payload);
        buf
    }

    fn make_stream_end_packet(request_id: u64) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(RPC_TYPE_STREAM_END);
        encode_leb128_u64(request_id, &mut buf);
        buf
    }

    fn make_error_packet(request_id: u64, code: u32, message: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(RPC_TYPE_ERROR);
        encode_leb128_u64(request_id, &mut buf);
        encode_leb128_u64(code as u64, &mut buf);
        encode_leb128_u64(message.len() as u64, &mut buf);
        buf.extend_from_slice(message.as_bytes());
        buf
    }
}

impl Drop for StreamSenderBase {
    fn drop(&mut self) {
        if !self.finished {
            let packet = Self::make_stream_end_packet(self.call_id);
            let _ = self.tx.try_send(packet);
        }
    }
}

/// Server base providing common server functionality.
pub struct ServerBase<Tr: Transport> {
    transport: Tr,
    handlers: HashMap<u32, MethodHandler>,
}

impl<Tr: Transport> ServerBase<Tr> {
    /// Creates a new server base.
    pub fn new(transport: Tr) -> Self {
        Self {
            transport,
            handlers: HashMap::new(),
        }
    }

    /// Registers a unary method handler.
    pub fn register_unary<F, Fut>(&mut self, method_index: u32, handler: F)
    where
        F: Fn(Vec<u8>) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<u8>, RpcError>> + 'static,
    {
        self.handlers.insert(
            method_index,
            MethodHandler::Unary(Box::new(move |payload| Box::pin(handler(payload)))),
        );
    }

    /// Registers a streaming method handler.
    ///
    /// Stream handlers do not return a Result. Errors must be reported by calling
    /// `stream.error()`. If the handler returns normally without calling
    /// `error`, a StreamEnd message is sent automatically when the
    /// stream sender is dropped.
    pub fn register_stream<F, Fut>(&mut self, method_index: u32, handler: F)
    where
        F: Fn(Vec<u8>, StreamSenderBase) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        self.handlers.insert(
            method_index,
            MethodHandler::Stream(Box::new(move |payload, stream| Box::pin(handler(payload, stream)))),
        );
    }

    /// Serves requests until the transport is closed or an error occurs.
    ///
    /// This function must be run on a single-threaded (local) tokio runtime.
    /// When this future is dropped, all active request handlers are aborted.
    pub async fn serve(self) -> Result<(), RpcError> {
        use tokio::task::JoinHandle;

        let handlers = Rc::new(self.handlers);

        // Single channel for all outgoing packets
        let (tx_send, mut tx_recv) = mpsc::channel::<Vec<u8>>(64);

        // Track active requests for cancellation
        let active_requests: Rc<RefCell<HashMap<u64, JoinHandle<()>>>> = Rc::new(RefCell::new(HashMap::new()));

        // Guard to abort all active requests when serve() is dropped
        let _guard = OnDrop::new({
            let active_requests = active_requests.clone();
            move || {
                for (_, handle) in active_requests.borrow().iter() {
                    handle.abort();
                }
            }
        });

        // Receive loop - runs until transport error
        let rx_loop = async {
            loop {
                let data = self.transport.recv().await?;

                let mut buf = data.as_slice();

                // Decode message type
                if buf.is_empty() {
                    continue; // Invalid message, ignore
                }
                let msg_type = buf[0];
                buf = &buf[1..];

                // Decode call ID
                let call_id = match decode_leb128_u64(&mut buf) {
                    Ok(id) => id,
                    Err(_) => continue, // Invalid message, ignore
                };

                match msg_type {
                    RPC_TYPE_REQUEST => {
                        // Decode method index
                        let method_index = match decode_leb128_u64(&mut buf) {
                            Ok(idx) => idx as u32,
                            Err(_) => {
                                let _ = tx_send
                                    .send(Self::make_error_packet(
                                        call_id,
                                        ERR_CODE_DECODE_ERROR,
                                        "failed to decode method index",
                                    ))
                                    .await;
                                continue;
                            }
                        };

                        // Look up the handler
                        let handler = match handlers.get(&method_index) {
                            Some(h) => h,
                            None => {
                                let _ = tx_send
                                    .send(Self::make_error_packet(
                                        call_id,
                                        ERR_CODE_UNKNOWN_METHOD,
                                        "unknown method",
                                    ))
                                    .await;
                                continue;
                            }
                        };

                        let payload = buf.to_vec();
                        let tx_send = tx_send.clone();

                        // Spawn handler in a separate task for cancellation support
                        let handle = match handler {
                            MethodHandler::Unary(handler) => {
                                let fut = handler(payload);
                                let active_requests = active_requests.clone();
                                tokio::task::spawn_local(async move {
                                    match fut.await {
                                        Ok(response) => {
                                            let _ = tx_send.send(Self::make_response_packet(call_id, response)).await;
                                        }
                                        Err(e) => {
                                            let _ = tx_send
                                                .send(Self::make_error_packet(
                                                    call_id,
                                                    ERR_CODE_HANDLER_ERROR,
                                                    &e.message,
                                                ))
                                                .await;
                                        }
                                    }
                                    // Remove from active requests
                                    active_requests.borrow_mut().remove(&call_id);
                                })
                            }
                            MethodHandler::Stream(handler) => {
                                let stream = StreamSenderBase {
                                    call_id,
                                    tx: tx_send.clone(),
                                    finished: false,
                                };
                                let fut = handler(payload, stream);
                                let active_requests = active_requests.clone();
                                tokio::task::spawn_local(async move {
                                    fut.await;
                                    // StreamEnd sent by ServerStreamSender drop
                                    // Remove from active requests
                                    active_requests.borrow_mut().remove(&call_id);
                                })
                            }
                        };

                        // Track the request
                        active_requests.borrow_mut().insert(call_id, handle);
                    }
                    RPC_TYPE_CANCEL => {
                        // Cancel the request if it exists
                        if let Some(handle) = active_requests.borrow_mut().remove(&call_id) {
                            handle.abort();
                        }
                    }
                    _ => {
                        // Unknown message type, ignore
                    }
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), RpcError>(())
        };

        // Send loop - runs until channel closed or transport error
        let tx_loop = async {
            while let Some(packet) = tx_recv.recv().await {
                self.transport.send(packet).await?;
            }
            Ok::<(), RpcError>(())
        };

        // Run both loops, return first error
        tokio::try_join!(rx_loop, tx_loop).map(|_| ())
    }

    fn make_response_packet(request_id: u64, payload: Vec<u8>) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(RPC_TYPE_RESPONSE);
        encode_leb128_u64(request_id, &mut buf);
        buf.extend_from_slice(&payload);
        buf
    }

    fn make_error_packet(request_id: u64, code: u32, message: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(RPC_TYPE_ERROR);
        encode_leb128_u64(request_id, &mut buf);
        encode_leb128_u64(code as u64, &mut buf);
        encode_leb128_u64(message.len() as u64, &mut buf);
        buf.extend_from_slice(message.as_bytes());
        buf
    }
}

// ============================================================================
// Client Infrastructure
// ============================================================================

/// Stream item from server - either data or error.
enum StreamEvent {
    Item(Vec<u8>),
    End,
    Error(RpcError),
}

/// Pending request tracking.
enum PendingRequest {
    Unary {
        resp_tx: oneshot::Sender<Result<Vec<u8>, RpcError>>,
    },
    Stream {
        stream_tx: mpsc::Sender<StreamEvent>,
    },
}

/// Client base providing common client functionality.
pub struct ClientBase<Tr: Transport> {
    transport: Tr,
    next_id: RefCell<u64>,
    pending: Rc<RefCell<HashMap<u64, PendingRequest>>>,
    tx_send: mpsc::Sender<Vec<u8>>,
    tx_recv: RefCell<Option<mpsc::Receiver<Vec<u8>>>>,
}

impl<Tr: Transport> ClientBase<Tr> {
    /// Creates a new client base.
    pub fn new(transport: Tr) -> Self {
        let (tx_send, tx_recv) = mpsc::channel(64);
        Self {
            transport,
            next_id: RefCell::new(1),
            pending: Rc::new(RefCell::new(HashMap::new())),
            tx_send,
            tx_recv: RefCell::new(Some(tx_recv)),
        }
    }

    /// Runs the client's send and receive loops.
    ///
    /// This function runs until the transport is closed or an error occurs.
    /// Call this from within a `LocalSet` context.
    pub async fn run(&self) -> Result<(), RpcError> {
        let mut tx_recv = self.tx_recv.borrow_mut().take().expect("run() called twice");

        // Guard to notify all pending requests when run() exits (error or drop)
        let _pending_guard = OnDrop::new({
            let pending = self.pending.clone();
            move || {
                let err = RpcError::new(0, "transport closed");
                for (_, req) in pending.borrow_mut().drain() {
                    match req {
                        PendingRequest::Unary { resp_tx } => {
                            let _ = resp_tx.send(Err(err.clone()));
                        }
                        PendingRequest::Stream { stream_tx } => {
                            let _ = stream_tx.try_send(StreamEvent::Error(err.clone()));
                        }
                    }
                }
            }
        });

        // Receive loop - runs until transport error
        let rx_loop = async {
            loop {
                let data = self.transport.recv().await?;

                let mut buf = data.as_slice();

                // Decode message type
                if buf.is_empty() {
                    continue; // Invalid message, ignore
                }
                let msg_type = buf[0];
                buf = &buf[1..];

                // Decode request ID
                let request_id = match decode_leb128_u64(&mut buf) {
                    Ok(id) => id,
                    Err(_) => continue, // Invalid message, ignore
                };

                let mut pending = self.pending.borrow_mut();
                let req = match pending.get_mut(&request_id) {
                    Some(req) => req,
                    None => continue, // Unknown request ID, ignore
                };

                match msg_type {
                    RPC_TYPE_RESPONSE => {
                        if let PendingRequest::Unary { .. } = req {
                            if let Some(PendingRequest::Unary { resp_tx }) = pending.remove(&request_id) {
                                let _ = resp_tx.send(Ok(buf.to_vec()));
                            }
                        }
                    }
                    RPC_TYPE_STREAM_ITEM => {
                        if let PendingRequest::Stream { stream_tx } = req {
                            let _ = stream_tx.send(StreamEvent::Item(buf.to_vec())).await;
                        }
                    }
                    RPC_TYPE_STREAM_END => {
                        if let PendingRequest::Stream { .. } = req {
                            if let Some(PendingRequest::Stream { stream_tx }) = pending.remove(&request_id) {
                                let _ = stream_tx.send(StreamEvent::End).await;
                            }
                        }
                    }
                    RPC_TYPE_ERROR => {
                        let err_code = decode_leb128_u64(&mut buf).unwrap_or(0) as u32;
                        let err_len = decode_leb128_u64(&mut buf).unwrap_or(0) as usize;
                        let err_msg = if buf.len() >= err_len {
                            String::from_utf8_lossy(&buf[..err_len]).to_string()
                        } else {
                            "unknown error".to_string()
                        };
                        let err = RpcError::new(err_code, err_msg);
                        match pending.remove(&request_id) {
                            Some(PendingRequest::Unary { resp_tx }) => {
                                let _ = resp_tx.send(Err(err));
                            }
                            Some(PendingRequest::Stream { stream_tx }) => {
                                let _ = stream_tx.send(StreamEvent::Error(err)).await;
                            }
                            None => {}
                        }
                    }
                    _ => {
                        // Unknown message type, ignore
                    }
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), RpcError>(())
        };

        // Send loop - runs until channel closed or transport error
        let tx_loop = async {
            while let Some(packet) = tx_recv.recv().await {
                self.transport.send(packet).await?;
            }
            Ok::<(), RpcError>(())
        };

        // Run both loops, return first error
        tokio::try_join!(rx_loop, tx_loop).map(|_| ())
    }

    /// Makes a unary RPC call.
    ///
    /// If this future is dropped before completion, a cancel message is sent.
    pub async fn call_unary(&self, method_index: u32, payload: Vec<u8>) -> Result<Vec<u8>, RpcError> {
        // Allocate request ID
        let request_id = {
            let mut next_id = self.next_id.borrow_mut();
            let id = *next_id;
            *next_id += 1;
            id
        };

        // Create response channel
        let (resp_tx, resp_rx) = oneshot::channel();

        // Register pending request
        self.pending
            .borrow_mut()
            .insert(request_id, PendingRequest::Unary { resp_tx });

        // Build request message
        let mut buf = Vec::new();
        buf.push(RPC_TYPE_REQUEST);
        encode_leb128_u64(request_id, &mut buf);
        encode_leb128_u64(method_index as u64, &mut buf);
        buf.extend_from_slice(&payload);

        // Send request via channel
        if self.tx_send.send(buf).await.is_err() {
            self.pending.borrow_mut().remove(&request_id);
            return Err(RpcError::new(0, "transport closed"));
        }

        // Guard to send cancel message if dropped
        let mut guard = OnDrop::new({
            let tx_send = self.tx_send.clone();
            let pending = self.pending.clone();
            move || {
                pending.borrow_mut().remove(&request_id);
                let mut buf = Vec::new();
                buf.push(RPC_TYPE_CANCEL);
                encode_leb128_u64(request_id, &mut buf);
                let _ = tx_send.try_send(buf);
            }
        });

        // Wait for response
        let result = resp_rx.await.map_err(|_| RpcError::new(0, "response channel closed"))?;
        guard.defuse();
        result
    }

    /// Makes a streaming RPC call.
    pub async fn call_stream(&self, method_index: u32, payload: Vec<u8>) -> Result<StreamReceiver, RpcError> {
        // Allocate request ID
        let request_id = {
            let mut next_id = self.next_id.borrow_mut();
            let id = *next_id;
            *next_id += 1;
            id
        };

        // Create stream channel
        let (stream_tx, stream_rx) = mpsc::channel(16);

        // Register pending request
        self.pending
            .borrow_mut()
            .insert(request_id, PendingRequest::Stream { stream_tx });

        // Build request message
        let mut buf = Vec::new();
        buf.push(RPC_TYPE_REQUEST);
        encode_leb128_u64(request_id, &mut buf);
        encode_leb128_u64(method_index as u64, &mut buf);
        buf.extend_from_slice(&payload);

        // Send request via channel
        if self.tx_send.send(buf).await.is_err() {
            self.pending.borrow_mut().remove(&request_id);
            return Err(RpcError::new(0, "transport closed"));
        }

        Ok(StreamReceiver {
            request_id,
            stream_rx,
            tx_send: self.tx_send.clone(),
            pending: self.pending.clone(),
        })
    }
}

/// Stream receiver for streaming responses.
///
/// Cancels the stream on drop if not already closed.
pub struct StreamReceiver {
    request_id: u64,
    stream_rx: mpsc::Receiver<StreamEvent>,
    tx_send: mpsc::Sender<Vec<u8>>,
    pending: Rc<RefCell<HashMap<u64, PendingRequest>>>,
}

impl StreamReceiver {
    /// Receives the next item from the stream.
    pub async fn recv(&mut self) -> Result<Vec<u8>, RpcError> {
        match self.stream_rx.recv().await {
            Some(StreamEvent::Item(data)) => Ok(data),
            Some(StreamEvent::End) => Err(RpcError::stream_closed()),
            Some(StreamEvent::Error(e)) => Err(e),
            None => Err(RpcError::stream_closed()),
        }
    }
}

impl Drop for StreamReceiver {
    fn drop(&mut self) {
        // Remove from pending and send cancel if still active
        if self.pending.borrow_mut().remove(&self.request_id).is_some() {
            let mut buf = Vec::new();
            buf.push(RPC_TYPE_CANCEL);
            encode_leb128_u64(self.request_id, &mut buf);
            let _ = self.tx_send.try_send(buf);
        }
    }
}

// ============================================================================
// TCP Transport
// ============================================================================

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

/// TCP transport for RPC.
pub struct TcpTransport {
    read: RefCell<OwnedReadHalf>,
    write: RefCell<OwnedWriteHalf>,
}

impl TcpTransport {
    /// Creates a new TCP transport from a TCP stream.
    pub fn new(stream: tokio::net::TcpStream) -> Self {
        // Disable Nagle's algorithm for lower latency
        let _ = stream.set_nodelay(true);
        let (read, write) = stream.into_split();
        Self {
            read: RefCell::new(read),
            write: RefCell::new(write),
        }
    }
}

impl Transport for TcpTransport {
    async fn send(&self, data: Vec<u8>) -> Result<(), RpcError> {
        let mut write = self.write.borrow_mut();

        // Write length prefix as LEB128
        let mut len_buf = Vec::new();
        encode_leb128_u64(data.len() as u64, &mut len_buf);
        write
            .write_all(&len_buf)
            .await
            .map_err(|e| RpcError::new(0, e.to_string()))?;
        write
            .write_all(&data)
            .await
            .map_err(|e| RpcError::new(0, e.to_string()))?;
        write.flush().await.map_err(|e| RpcError::new(0, e.to_string()))?;
        Ok(())
    }

    async fn recv(&self) -> Result<Vec<u8>, RpcError> {
        let mut read = self.read.borrow_mut();

        // Read length prefix as LEB128
        let mut length: u64 = 0;
        let mut shift = 0;
        loop {
            let mut byte = [0u8; 1];
            read.read_exact(&mut byte)
                .await
                .map_err(|e| RpcError::new(0, e.to_string()))?;
            length |= ((byte[0] & 0x7F) as u64) << shift;
            if byte[0] & 0x80 == 0 {
                break;
            }
            shift += 7;
        }

        // Read data
        let mut data = vec![0u8; length as usize];
        read.read_exact(&mut data)
            .await
            .map_err(|e| RpcError::new(0, e.to_string()))?;
        Ok(data)
    }
}
