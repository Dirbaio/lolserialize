package main

import (
	"context"
	"errors"
	"fmt"
	"net"
	"os"
	"time"

	"rpc_test/gen"

	volex "github.com/volex/runtime"
)

// TestServiceImpl implements the TestService interface
type TestServiceImpl struct {
	lastStatus string
}

func (s *TestServiceImpl) Echo(ctx context.Context, req gen.EchoRequest) (gen.EchoResponse, error) {
	return gen.EchoResponse{Text: req.Text}, nil
}

func (s *TestServiceImpl) Subscribe(ctx context.Context, req gen.StreamRequest, stream gen.TestServiceSubscribeStream) error {
	for i := uint32(0); i < req.Count; i++ {
		err := stream.Send(gen.StreamItem{
			Seq:  i,
			Data: fmt.Sprintf("item-%d", i),
		})
		if err != nil {
			return err
		}
	}
	return nil
}

func (s *TestServiceImpl) MaybeFail(ctx context.Context, req gen.FailRequest) (gen.FailResponse, error) {
	if req.ShouldFail {
		return gen.FailResponse{}, errors.New(req.ErrorMessage)
	}
	return gen.FailResponse{Success: true}, nil
}

func (s *TestServiceImpl) StreamThenFail(ctx context.Context, req gen.StreamRequest, stream gen.TestServiceStreamThenFailStream) error {
	// Send some items, then fail
	for i := uint32(0); i < req.Count; i++ {
		err := stream.Send(gen.StreamItem{
			Seq:  i,
			Data: fmt.Sprintf("item-%d", i),
		})
		if err != nil {
			return err
		}
	}
	return errors.New("stream error after items")
}

func (s *TestServiceImpl) Add(ctx context.Context, req uint32) (uint32, error) {
	return req + 10, nil
}

func (s *TestServiceImpl) GetStrings(ctx context.Context, req uint32, stream gen.TestServiceGetStringsStream) error {
	for i := uint32(0); i < req; i++ {
		err := stream.Send(fmt.Sprintf("string-%d", i))
		if err != nil {
			return err
		}
	}
	return nil
}

func (s *TestServiceImpl) SlowUnary(ctx context.Context, req gen.SlowRequest) (gen.SlowResponse, error) {
	s.lastStatus = "slow_unary: started"
	// Wait for the specified delay, but respect context cancellation
	select {
	case <-time.After(time.Duration(req.DelayMs) * time.Millisecond):
		s.lastStatus = "slow_unary: completed"
		return gen.SlowResponse{Completed: true}, nil
	case <-ctx.Done():
		s.lastStatus = "slow_unary: canceled"
		return gen.SlowResponse{}, ctx.Err()
	}
}

func (s *TestServiceImpl) SlowStream(ctx context.Context, req gen.SlowRequest, stream gen.TestServiceSlowStreamStream) error {
	s.lastStatus = "slow_stream: started"
	// Send items with delay between each, respecting cancellation
	for i := uint32(0); ; i++ {
		select {
		case <-time.After(time.Duration(req.DelayMs) * time.Millisecond):
			err := stream.Send(gen.StreamItem{
				Seq:  i,
				Data: fmt.Sprintf("slow-item-%d", i),
			})
			if err != nil {
				return err
			}
		case <-ctx.Done():
			s.lastStatus = "slow_stream: canceled"
			return ctx.Err()
		}
	}
}

func (s *TestServiceImpl) GetStatus(ctx context.Context, req gen.Empty) (string, error) {
	return s.lastStatus, nil
}

func main() {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		fmt.Fprintf(os.Stderr, "failed to listen: %v\n", err)
		os.Exit(1)
	}

	// Print the address for the client to connect to
	fmt.Println(listener.Addr().String())

	ctx := context.Background()

	for {
		conn, err := listener.Accept()
		if err != nil {
			return
		}
		go func(conn net.Conn) {
			defer conn.Close()
			transport := volex.NewTCPTransport(conn)
			impl := &TestServiceImpl{}
			server := gen.NewTestServiceServer(transport, impl)
			server.Serve(ctx)
		}(conn)
	}
}
