// Package volex provides runtime encoding/decoding support.
package volex

import (
	"context"
	"errors"
	"sync"
)

// ============================================================================
// RPC Transport
// ============================================================================

// Transport is the interface for sending and receiving binary messages.
// Implementations must be safe for concurrent use.
type Transport interface {
	// Send sends a binary message. It blocks until the message is sent.
	Send(ctx context.Context, data []byte) error
	// Recv receives a binary message. It blocks until a message is received.
	Recv(ctx context.Context) ([]byte, error)
}

// ============================================================================
// RPC Errors
// ============================================================================

var (
	ErrStreamClosed = errors.New("stream closed")
)

// RpcError represents an RPC error with a code and message.
type RpcError struct {
	Code    uint32
	Message string
}

func (e *RpcError) Error() string {
	return e.Message
}

// Error codes
const (
	ErrCodeUnknownMethod uint32 = 1
	ErrCodeDecodeError   uint32 = 2
	ErrCodeHandlerError  uint32 = 3
)

// ============================================================================
// RPC Message Types
// ============================================================================

// RPC message format:
// Server -> Client (0x00-0x7F):
// - Response:   [0x00] [callId: LEB128] [payload...]
// - StreamItem: [0x01] [callId: LEB128] [payload...]
// - StreamEnd:  [0x02] [callId: LEB128]
// - Error:      [0x03] [callId: LEB128] [errorCode: LEB128] [errorMessage: string]
// Client -> Server (0x80-0xFF):
// - Request:    [0x80] [callId: LEB128] [methodIndex: LEB128] [payload...]
// - Cancel:     [0x81] [callId: LEB128]

const (
	// Server -> Client message types (0x00-0x7F)
	rpcTypeResponse   byte = 0x00
	rpcTypeStreamItem byte = 0x01
	rpcTypeStreamEnd  byte = 0x02
	rpcTypeError      byte = 0x03

	// Client -> Server message types (0x80-0xFF)
	rpcTypeRequest byte = 0x80
	rpcTypeCancel  byte = 0x81
)

// ============================================================================
// Server Infrastructure
// ============================================================================

// MethodHandler is a function that handles a single RPC method call.
// For unary methods, it returns the response bytes directly.
// For streaming methods, it sends stream items through the provided channel.
type MethodHandler func(ctx context.Context, payload []byte, sender *ResponseSender) error

// ResponseSender is used by method handlers to send responses.
type ResponseSender struct {
	server    *ServerBase
	requestID uint64
}

// SendResponse sends a unary response.
func (s *ResponseSender) SendResponse(payload []byte) error {
	return s.server.sendResponse(s.requestID, payload)
}

// SendStreamItem sends a stream item.
func (s *ResponseSender) SendStreamItem(payload []byte) error {
	return s.server.sendStreamItem(s.requestID, payload)
}

// SendStreamEnd signals the end of a stream.
func (s *ResponseSender) SendStreamEnd() error {
	return s.server.sendStreamEnd(s.requestID)
}

// SendError sends an error response.
func (s *ResponseSender) SendError(code uint32, errMsg string) error {
	return s.server.sendError(s.requestID, code, errMsg)
}

// activeRequest tracks an in-flight server request.
type activeRequest struct {
	cancel context.CancelFunc
}

// ServerBase provides common server functionality.
type ServerBase struct {
	transport Transport
	handlers  map[uint32]MethodHandler
	mu        sync.Mutex
	active    map[uint64]*activeRequest
}

// NewServerBase creates a new ServerBase.
func NewServerBase(transport Transport) *ServerBase {
	return &ServerBase{
		transport: transport,
		handlers:  make(map[uint32]MethodHandler),
		active:    make(map[uint64]*activeRequest),
	}
}

// RegisterHandler registers a method handler.
func (s *ServerBase) RegisterHandler(methodIndex uint32, handler MethodHandler) {
	s.handlers[methodIndex] = handler
}

// Serve starts serving requests. It runs until the context is canceled or an error occurs.
func (s *ServerBase) Serve(ctx context.Context) error {
	for {
		select {
		case <-ctx.Done():
			s.cancelAllRequests()
			return ctx.Err()
		default:
		}

		data, err := s.transport.Recv(ctx)
		if err != nil {
			s.cancelAllRequests()
			return err
		}

		s.handleMessage(ctx, data)
	}
}

func (s *ServerBase) cancelAllRequests() {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, req := range s.active {
		req.cancel()
	}
}

func (s *ServerBase) handleMessage(ctx context.Context, data []byte) {
	buf := data

	// Decode message type
	if len(buf) == 0 {
		return // Invalid message, ignore
	}
	msgType := buf[0]
	buf = buf[1:]

	// Decode call ID
	callID, err := DecodeLEB128(&buf)
	if err != nil {
		return // Invalid message, ignore
	}

	switch msgType {
	case rpcTypeRequest:
		go s.handleRequest(ctx, callID, buf)
	case rpcTypeCancel:
		s.handleCancel(callID)
	default:
		// Unknown message type, ignore
	}
}

func (s *ServerBase) handleRequest(ctx context.Context, callID uint64, buf []byte) {
	// Create cancellable context for this request
	reqCtx, cancel := context.WithCancel(ctx)
	defer cancel()

	// Register the active request
	s.mu.Lock()
	s.active[callID] = &activeRequest{cancel: cancel}
	s.mu.Unlock()

	// Clean up when done
	defer func() {
		s.mu.Lock()
		delete(s.active, callID)
		s.mu.Unlock()
	}()

	// Decode method index
	methodIndex, err := DecodeLEB128(&buf)
	if err != nil {
		s.sendError(callID, ErrCodeDecodeError, "failed to decode method index")
		return
	}

	handler, ok := s.handlers[uint32(methodIndex)]
	if !ok {
		s.sendError(callID, ErrCodeUnknownMethod, "unknown method")
		return
	}

	sender := &ResponseSender{server: s, requestID: callID}
	if err := handler(reqCtx, buf, sender); err != nil {
		s.sendError(callID, ErrCodeHandlerError, err.Error())
	}
}

func (s *ServerBase) handleCancel(callID uint64) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if req, ok := s.active[callID]; ok {
		req.cancel()
	}
}

func (s *ServerBase) sendResponse(requestID uint64, payload []byte) error {
	var buf []byte
	buf = append(buf, rpcTypeResponse)
	EncodeLEB128(requestID, &buf)
	buf = append(buf, payload...)
	return s.transport.Send(context.Background(), buf)
}

func (s *ServerBase) sendStreamItem(requestID uint64, payload []byte) error {
	var buf []byte
	buf = append(buf, rpcTypeStreamItem)
	EncodeLEB128(requestID, &buf)
	buf = append(buf, payload...)
	return s.transport.Send(context.Background(), buf)
}

func (s *ServerBase) sendStreamEnd(requestID uint64) error {
	var buf []byte
	buf = append(buf, rpcTypeStreamEnd)
	EncodeLEB128(requestID, &buf)
	return s.transport.Send(context.Background(), buf)
}

func (s *ServerBase) sendError(requestID uint64, code uint32, errMsg string) error {
	var buf []byte
	buf = append(buf, rpcTypeError)
	EncodeLEB128(requestID, &buf)
	EncodeLEB128(uint64(code), &buf)
	EncodeString(errMsg, &buf)
	return s.transport.Send(context.Background(), buf)
}

// ============================================================================
// Client Infrastructure
// ============================================================================

// streamResult represents either a stream item or an error/end signal.
type streamResult struct {
	data []byte // nil if this is an error or stream end
	err  error  // nil for data items, ErrStreamClosed for end, or the actual error
}

// pendingRequest tracks an in-flight request.
type pendingRequest struct {
	respChan   chan []byte       // For unary responses
	streamChan chan streamResult // For streaming responses (items, errors, and end)
	errChan    chan error        // For unary errors
	isStream   bool
}

// ClientBase provides common client functionality.
type ClientBase struct {
	transport   Transport
	mu          sync.Mutex
	nextID      uint64
	pending     map[uint64]*pendingRequest
	recvRunning bool
	recvErr     error
	recvErrMu   sync.RWMutex
}

// NewClientBase creates a new ClientBase.
func NewClientBase(transport Transport) *ClientBase {
	return &ClientBase{
		transport: transport,
		nextID:    1,
		pending:   make(map[uint64]*pendingRequest),
	}
}

// Run runs the client's receive loop until the context is canceled or the transport fails.
// Must be called before making any RPC calls. This method blocks until an error occurs.
func (c *ClientBase) Run(ctx context.Context) error {
	c.mu.Lock()
	if c.recvRunning {
		c.mu.Unlock()
		return errors.New("client already running")
	}
	c.recvRunning = true
	c.mu.Unlock()

	return c.receiveLoop(ctx)
}

func (c *ClientBase) receiveLoop(ctx context.Context) error {
	for {
		select {
		case <-ctx.Done():
			c.setRecvError(ctx.Err())
			return ctx.Err()
		default:
		}

		data, err := c.transport.Recv(ctx)
		if err != nil {
			c.setRecvError(err)
			return err
		}

		c.handleResponse(data)
	}
}

func (c *ClientBase) setRecvError(err error) {
	c.recvErrMu.Lock()
	c.recvErr = err
	c.recvErrMu.Unlock()

	// Notify all pending requests
	c.mu.Lock()
	for _, req := range c.pending {
		if req.isStream {
			select {
			case req.streamChan <- streamResult{err: err}:
			default:
			}
		} else {
			select {
			case req.errChan <- err:
			default:
			}
		}
	}
	c.mu.Unlock()
}

func (c *ClientBase) handleResponse(data []byte) {
	buf := data

	// Decode message type
	if len(buf) == 0 {
		return // Invalid message, ignore
	}
	msgType := buf[0]
	buf = buf[1:]

	// Decode request ID
	requestID, err := DecodeLEB128(&buf)
	if err != nil {
		return // Invalid message, ignore
	}

	c.mu.Lock()
	req, ok := c.pending[requestID]
	c.mu.Unlock()

	if !ok {
		return // Unknown request ID, ignore
	}

	switch msgType {
	case rpcTypeResponse:
		// Make a copy of the payload
		payload := make([]byte, len(buf))
		copy(payload, buf)
		select {
		case req.respChan <- payload:
		default:
		}

	case rpcTypeStreamItem:
		// Make a copy of the payload
		payload := make([]byte, len(buf))
		copy(payload, buf)
		select {
		case req.streamChan <- streamResult{data: payload}:
		default:
		}

	case rpcTypeStreamEnd:
		select {
		case req.streamChan <- streamResult{err: ErrStreamClosed}:
		default:
		}
		c.mu.Lock()
		delete(c.pending, requestID)
		c.mu.Unlock()

	case rpcTypeError:
		errCode, _ := DecodeLEB128(&buf)
		errMsg, _ := DecodeString(&buf)
		rpcErr := &RpcError{Code: uint32(errCode), Message: errMsg}
		if req.isStream {
			select {
			case req.streamChan <- streamResult{err: rpcErr}:
			default:
			}
		} else {
			select {
			case req.errChan <- rpcErr:
			default:
			}
		}
		c.mu.Lock()
		delete(c.pending, requestID)
		c.mu.Unlock()
	}
}

// CallUnary makes a unary RPC call.
func (c *ClientBase) CallUnary(ctx context.Context, methodIndex uint32, payload []byte) ([]byte, error) {
	// Check for receive error
	c.recvErrMu.RLock()
	if c.recvErr != nil {
		err := c.recvErr
		c.recvErrMu.RUnlock()
		return nil, err
	}
	c.recvErrMu.RUnlock()

	// Allocate request ID
	c.mu.Lock()
	requestID := c.nextID
	c.nextID++

	req := &pendingRequest{
		respChan: make(chan []byte, 1),
		errChan:  make(chan error, 1),
		isStream: false,
	}
	c.pending[requestID] = req
	c.mu.Unlock()

	// Build request message
	var buf []byte
	buf = append(buf, rpcTypeRequest)
	EncodeLEB128(requestID, &buf)
	EncodeLEB128(uint64(methodIndex), &buf)
	buf = append(buf, payload...)

	// Send request
	if err := c.transport.Send(ctx, buf); err != nil {
		c.mu.Lock()
		delete(c.pending, requestID)
		c.mu.Unlock()
		return nil, err
	}

	// Wait for response
	select {
	case <-ctx.Done():
		// Send cancel message
		c.sendCancel(requestID)
		c.mu.Lock()
		delete(c.pending, requestID)
		c.mu.Unlock()
		return nil, ctx.Err()
	case resp := <-req.respChan:
		c.mu.Lock()
		delete(c.pending, requestID)
		c.mu.Unlock()
		return resp, nil
	case err := <-req.errChan:
		return nil, err
	}
}

// StreamReceiver is used to receive streaming responses.
type StreamReceiver struct {
	client     *ClientBase
	requestID  uint64
	streamChan <-chan streamResult
	ctx        context.Context
}

// Recv receives the next stream item. Returns nil, ErrStreamClosed when the stream ends.
func (s *StreamReceiver) Recv() ([]byte, error) {
	select {
	case <-s.ctx.Done():
		s.Cancel()
		return nil, s.ctx.Err()
	case res := <-s.streamChan:
		if res.err != nil {
			return nil, res.err
		}
		return res.data, nil
	}
}

// Cancel cancels the stream.
func (s *StreamReceiver) Cancel() {
	s.client.sendCancel(s.requestID)
	s.client.mu.Lock()
	delete(s.client.pending, s.requestID)
	s.client.mu.Unlock()
}

// CallStream makes a streaming RPC call.
func (c *ClientBase) CallStream(ctx context.Context, methodIndex uint32, payload []byte) (*StreamReceiver, error) {
	// Check for receive error
	c.recvErrMu.RLock()
	if c.recvErr != nil {
		err := c.recvErr
		c.recvErrMu.RUnlock()
		return nil, err
	}
	c.recvErrMu.RUnlock()

	// Allocate request ID
	c.mu.Lock()
	requestID := c.nextID
	c.nextID++

	streamChan := make(chan streamResult, 16) // Buffer some stream items
	req := &pendingRequest{
		streamChan: streamChan,
		isStream:   true,
	}
	c.pending[requestID] = req
	c.mu.Unlock()

	// Build request message
	var buf []byte
	buf = append(buf, rpcTypeRequest)
	EncodeLEB128(requestID, &buf)
	EncodeLEB128(uint64(methodIndex), &buf)
	buf = append(buf, payload...)

	// Send request
	if err := c.transport.Send(ctx, buf); err != nil {
		c.mu.Lock()
		delete(c.pending, requestID)
		c.mu.Unlock()
		return nil, err
	}

	return &StreamReceiver{
		client:     c,
		requestID:  requestID,
		streamChan: streamChan,
		ctx:        ctx,
	}, nil
}

func (c *ClientBase) sendCancel(requestID uint64) {
	var buf []byte
	buf = append(buf, rpcTypeCancel)
	EncodeLEB128(requestID, &buf)
	// Best effort, ignore errors
	_ = c.transport.Send(context.Background(), buf)
}

// ============================================================================
// TCP Transport
// ============================================================================

// TCPTransport implements Transport over a TCP-like connection (net.Conn).
// It uses LEB128 length-prefix framing as per the RPC transport spec.
type TCPTransport struct {
	conn interface {
		Read(b []byte) (n int, err error)
		Write(b []byte) (n int, err error)
	}
	readMu  sync.Mutex
	writeMu sync.Mutex
}

// NewTCPTransport creates a new TCPTransport from a net.Conn.
func NewTCPTransport(conn interface {
	Read(b []byte) (n int, err error)
	Write(b []byte) (n int, err error)
}) *TCPTransport {
	return &TCPTransport{conn: conn}
}

// Send sends a binary message with LEB128 length-prefix framing.
func (t *TCPTransport) Send(ctx context.Context, data []byte) error {
	t.writeMu.Lock()
	defer t.writeMu.Unlock()

	// Write length prefix as LEB128
	var lenBuf []byte
	EncodeLEB128(uint64(len(data)), &lenBuf)
	if _, err := t.conn.Write(lenBuf); err != nil {
		return err
	}
	_, err := t.conn.Write(data)
	return err
}

// Recv receives a binary message with LEB128 length-prefix framing.
func (t *TCPTransport) Recv(ctx context.Context) ([]byte, error) {
	t.readMu.Lock()
	defer t.readMu.Unlock()

	// Read length prefix as LEB128
	length, err := t.readLEB128()
	if err != nil {
		return nil, err
	}

	// Read data
	data := make([]byte, length)
	if _, err := t.readFull(data); err != nil {
		return nil, err
	}
	return data, nil
}

func (t *TCPTransport) readLEB128() (uint64, error) {
	var result uint64
	var shift uint
	for {
		var b [1]byte
		if _, err := t.conn.Read(b[:]); err != nil {
			return 0, err
		}
		result |= uint64(b[0]&0x7F) << shift
		if b[0]&0x80 == 0 {
			break
		}
		shift += 7
	}
	return result, nil
}

func (t *TCPTransport) readFull(buf []byte) (int, error) {
	total := 0
	for total < len(buf) {
		n, err := t.conn.Read(buf[total:])
		if err != nil {
			return total + n, err
		}
		total += n
	}
	return total, nil
}
