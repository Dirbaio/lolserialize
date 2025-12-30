package main

import (
	"context"
	"fmt"
	"net"
	"os"
	"sync"
	"testing"
	"time"

	"rpc_test/gen"

	volex "github.com/volex/runtime"
)

func TestRPC(t *testing.T) {
	addr := os.Getenv("SERVER_ADDR")
	if addr == "" {
		t.Fatal("SERVER_ADDR environment variable not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	conn, err := net.Dial("tcp", addr)
	if err != nil {
		t.Fatalf("failed to connect: %v", err)
	}
	defer conn.Close()

	transport := volex.NewTCPTransport(conn)
	client := gen.NewTestServiceClient(transport)
	client.Start(ctx)

	// Test echo
	t.Run("echo simple", func(t *testing.T) {
		resp, err := client.Echo(ctx, gen.EchoRequest{Text: "hello"})
		if err != nil {
			t.Fatalf("Echo failed: %v", err)
		}
		if resp.Text != "hello" {
			t.Errorf("expected 'hello', got '%s'", resp.Text)
		}
	})

	t.Run("echo empty", func(t *testing.T) {
		resp, err := client.Echo(ctx, gen.EchoRequest{Text: ""})
		if err != nil {
			t.Fatalf("Echo failed: %v", err)
		}
		if resp.Text != "" {
			t.Errorf("expected '', got '%s'", resp.Text)
		}
	})

	t.Run("echo unicode", func(t *testing.T) {
		resp, err := client.Echo(ctx, gen.EchoRequest{Text: "Hello 世界 🌍"})
		if err != nil {
			t.Fatalf("Echo failed: %v", err)
		}
		if resp.Text != "Hello 世界 🌍" {
			t.Errorf("expected 'Hello 世界 🌍', got '%s'", resp.Text)
		}
	})

	// Test streaming
	t.Run("stream zero items", func(t *testing.T) {
		stream, err := client.Subscribe(ctx, gen.StreamRequest{Count: 0})
		if err != nil {
			t.Fatalf("Subscribe failed: %v", err)
		}
		_, err = stream.Recv()
		if err != volex.ErrStreamClosed {
			t.Errorf("expected ErrStreamClosed, got %v", err)
		}
	})

	t.Run("stream one item", func(t *testing.T) {
		stream, err := client.Subscribe(ctx, gen.StreamRequest{Count: 1})
		if err != nil {
			t.Fatalf("Subscribe failed: %v", err)
		}

		item, err := stream.Recv()
		if err != nil {
			t.Fatalf("Recv failed: %v", err)
		}
		if item.Seq != 0 || item.Data != "item-0" {
			t.Errorf("expected {0, item-0}, got {%d, %s}", item.Seq, item.Data)
		}

		_, err = stream.Recv()
		if err != volex.ErrStreamClosed {
			t.Errorf("expected ErrStreamClosed, got %v", err)
		}
	})

	t.Run("stream multiple items", func(t *testing.T) {
		stream, err := client.Subscribe(ctx, gen.StreamRequest{Count: 5})
		if err != nil {
			t.Fatalf("Subscribe failed: %v", err)
		}

		for i := uint32(0); i < 5; i++ {
			item, err := stream.Recv()
			if err != nil {
				t.Fatalf("Recv failed: %v", err)
			}
			expectedData := fmt.Sprintf("item-%d", i)
			if item.Seq != i || item.Data != expectedData {
				t.Errorf("expected {%d, %s}, got {%d, %s}", i, expectedData, item.Seq, item.Data)
			}
		}

		_, err = stream.Recv()
		if err != volex.ErrStreamClosed {
			t.Errorf("expected ErrStreamClosed, got %v", err)
		}
	})

	// Test error handling
	t.Run("error unary success", func(t *testing.T) {
		resp, err := client.MaybeFail(ctx, gen.FailRequest{ShouldFail: false})
		if err != nil {
			t.Fatalf("MaybeFail failed unexpectedly: %v", err)
		}
		if !resp.Success {
			t.Errorf("expected success=true")
		}
	})

	t.Run("error unary failure", func(t *testing.T) {
		_, err := client.MaybeFail(ctx, gen.FailRequest{ShouldFail: true, ErrorMessage: "test error"})
		if err == nil {
			t.Fatalf("MaybeFail should have failed")
		}
		rpcErr, ok := err.(*volex.RpcError)
		if !ok {
			t.Fatalf("expected *RpcError, got %T: %v", err, err)
		}
		if rpcErr.Code != volex.ErrCodeHandlerError {
			t.Errorf("expected error code %d, got %d", volex.ErrCodeHandlerError, rpcErr.Code)
		}
		if rpcErr.Message != "test error" {
			t.Errorf("expected message 'test error', got '%s'", rpcErr.Message)
		}
	})

	t.Run("error stream after items", func(t *testing.T) {
		stream, err := client.StreamThenFail(ctx, gen.StreamRequest{Count: 3})
		if err != nil {
			t.Fatalf("StreamThenFail failed: %v", err)
		}

		// Should receive 3 items first
		for i := uint32(0); i < 3; i++ {
			item, err := stream.Recv()
			if err != nil {
				t.Fatalf("Recv failed on item %d: %v", i, err)
			}
			expectedData := fmt.Sprintf("item-%d", i)
			if item.Seq != i || item.Data != expectedData {
				t.Errorf("expected {%d, %s}, got {%d, %s}", i, expectedData, item.Seq, item.Data)
			}
		}

		// Then should get an error
		_, err = stream.Recv()
		if err == nil {
			t.Fatalf("expected error after stream items")
		}
		rpcErr, ok := err.(*volex.RpcError)
		if !ok {
			t.Fatalf("expected *RpcError, got %T: %v", err, err)
		}
		if rpcErr.Code != volex.ErrCodeHandlerError {
			t.Errorf("expected error code %d, got %d", volex.ErrCodeHandlerError, rpcErr.Code)
		}
	})

	// Test multiple simultaneous streams
	t.Run("multiple simultaneous streams", func(t *testing.T) {
		const numStreams = 5
		var wg sync.WaitGroup
		errChan := make(chan error, numStreams)

		for s := 0; s < numStreams; s++ {
			wg.Add(1)
			go func(streamIdx int) {
				defer wg.Done()
				count := uint32(streamIdx + 1) // Each stream gets different count

				stream, err := client.Subscribe(ctx, gen.StreamRequest{Count: count})
				if err != nil {
					errChan <- fmt.Errorf("stream %d: Subscribe failed: %v", streamIdx, err)
					return
				}

				for i := uint32(0); i < count; i++ {
					item, err := stream.Recv()
					if err != nil {
						errChan <- fmt.Errorf("stream %d: Recv failed on item %d: %v", streamIdx, i, err)
						return
					}
					expectedData := fmt.Sprintf("item-%d", i)
					if item.Seq != i || item.Data != expectedData {
						errChan <- fmt.Errorf("stream %d: expected {%d, %s}, got {%d, %s}", streamIdx, i, expectedData, item.Seq, item.Data)
						return
					}
				}

				_, err = stream.Recv()
				if err != volex.ErrStreamClosed {
					errChan <- fmt.Errorf("stream %d: expected ErrStreamClosed, got %v", streamIdx, err)
					return
				}
			}(s)
		}

		wg.Wait()
		close(errChan)

		for err := range errChan {
			t.Error(err)
		}
	})

	// Test non-message types
	t.Run("non-message add", func(t *testing.T) {
		resp, err := client.Add(ctx, uint32(5))
		if err != nil {
			t.Fatalf("Add failed: %v", err)
		}
		if resp != 15 {
			t.Errorf("expected 15, got %d", resp)
		}
	})

	t.Run("non-message add zero", func(t *testing.T) {
		resp, err := client.Add(ctx, uint32(0))
		if err != nil {
			t.Fatalf("Add failed: %v", err)
		}
		if resp != 10 {
			t.Errorf("expected 10, got %d", resp)
		}
	})

	t.Run("non-message stream strings", func(t *testing.T) {
		stream, err := client.GetStrings(ctx, uint32(3))
		if err != nil {
			t.Fatalf("GetStrings failed: %v", err)
		}

		for i := 0; i < 3; i++ {
			s, err := stream.Recv()
			if err != nil {
				t.Fatalf("Recv failed: %v", err)
			}
			expected := fmt.Sprintf("string-%d", i)
			if s != expected {
				t.Errorf("expected '%s', got '%s'", expected, s)
			}
		}

		_, err = stream.Recv()
		if err != volex.ErrStreamClosed {
			t.Errorf("expected ErrStreamClosed, got %v", err)
		}
	})

	// Test cancellation
	t.Run("cancel unary request", func(t *testing.T) {
		// Create a context that we'll cancel quickly
		cancelCtx, cancelFunc := context.WithCancel(ctx)

		// Start the slow request in a goroutine
		done := make(chan struct{})
		var callErr error
		go func() {
			defer close(done)
			// Request a 5 second delay - we'll cancel before it completes
			_, callErr = client.SlowUnary(cancelCtx, gen.SlowRequest{DelayMs: 5000})
		}()

		// Wait a bit for the server to start processing
		time.Sleep(50 * time.Millisecond)

		// Verify server has started but not yet canceled
		status, err := client.GetStatus(ctx, gen.Empty{})
		if err != nil {
			t.Fatalf("GetStatus failed: %v", err)
		}
		if status != "slow_unary: started" {
			t.Errorf("expected status 'slow_unary: started', got '%s'", status)
		}

		// Now cancel
		cancelFunc()

		// Wait for the call to complete
		select {
		case <-done:
			// Expected
		case <-time.After(500 * time.Millisecond):
			t.Fatal("call did not complete after cancellation")
		}

		// Should have gotten a context canceled error
		if callErr == nil {
			t.Fatal("expected error after cancellation")
		}
		if callErr != context.Canceled {
			t.Errorf("expected context.Canceled, got %v", callErr)
		}

		// Give the server a moment to process the cancellation
		time.Sleep(50 * time.Millisecond)

		// Verify server saw the cancellation
		status, err = client.GetStatus(ctx, gen.Empty{})
		if err != nil {
			t.Fatalf("GetStatus failed: %v", err)
		}
		if status != "slow_unary: canceled" {
			t.Errorf("expected status 'slow_unary: canceled', got '%s'", status)
		}
	})

	t.Run("cancel stream request", func(t *testing.T) {
		// Create a context that we'll cancel after receiving some items
		cancelCtx, cancelFunc := context.WithCancel(ctx)

		// Start a slow stream (items every 100ms)
		stream, err := client.SlowStream(cancelCtx, gen.SlowRequest{DelayMs: 100})
		if err != nil {
			t.Fatalf("SlowStream failed: %v", err)
		}

		// Receive a couple items
		for i := 0; i < 2; i++ {
			item, err := stream.Recv()
			if err != nil {
				t.Fatalf("Recv failed on item %d: %v", i, err)
			}
			if item.Seq != uint32(i) {
				t.Errorf("expected seq %d, got %d", i, item.Seq)
			}
		}

		// Verify server has started but not yet canceled
		status, err := client.GetStatus(ctx, gen.Empty{})
		if err != nil {
			t.Fatalf("GetStatus failed: %v", err)
		}
		if status != "slow_stream: started" {
			t.Errorf("expected status 'slow_stream: started', got '%s'", status)
		}

		// Now cancel and verify we get an error
		cancelFunc()

		// Next recv should fail with context canceled
		_, err = stream.Recv()
		if err == nil {
			t.Fatal("expected error after cancellation")
		}
		if err != context.Canceled {
			t.Errorf("expected context.Canceled, got %v", err)
		}

		// Give the server a moment to process the cancellation
		time.Sleep(50 * time.Millisecond)

		// Verify server saw the cancellation
		status, err = client.GetStatus(ctx, gen.Empty{})
		if err != nil {
			t.Fatalf("GetStatus failed: %v", err)
		}
		if status != "slow_stream: canceled" {
			t.Errorf("expected status 'slow_stream: canceled', got '%s'", status)
		}
	})
}
