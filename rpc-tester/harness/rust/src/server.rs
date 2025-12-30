//! Rust RPC test harness server.

mod generated;

use std::cell::RefCell;
use std::time::Duration;

use generated::*;
use tokio::net::TcpListener;
use volex::rpc::{RpcError, StreamSender, TcpTransport};

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

/// Test service implementation.
struct TestServiceImpl {
    last_status: RefCell<String>,
}

impl TestServiceImpl {
    fn new() -> Self {
        Self {
            last_status: RefCell::new(String::new()),
        }
    }
}

impl TestService for TestServiceImpl {
    async fn echo(&self, req: EchoRequest) -> Result<EchoResponse, RpcError> {
        Ok(EchoResponse { text: req.text })
    }

    async fn subscribe(&self, req: StreamRequest, stream: StreamSender<StreamItem>) {
        for i in 0..req.count {
            if stream
                .send(StreamItem {
                    seq: i,
                    data: format!("item-{}", i),
                })
                .await
                .is_err()
            {
                return;
            }
        }
    }

    async fn maybe_fail(&self, req: FailRequest) -> Result<FailResponse, RpcError> {
        if req.should_fail {
            Err(RpcError::new(volex::rpc::ERR_CODE_HANDLER_ERROR, &req.error_message))
        } else {
            Ok(FailResponse { success: true })
        }
    }

    async fn stream_then_fail(&self, req: StreamRequest, stream: StreamSender<StreamItem>) {
        for i in 0..req.count {
            if stream
                .send(StreamItem {
                    seq: i,
                    data: format!("item-{}", i),
                })
                .await
                .is_err()
            {
                return;
            }
        }
        stream
            .error(volex::rpc::ERR_CODE_HANDLER_ERROR, "stream error after items")
            .await;
    }

    async fn add(&self, req: u32) -> Result<u32, RpcError> {
        Ok(req + 10)
    }

    async fn get_strings(&self, req: u32, stream: StreamSender<String>) {
        for i in 0..req {
            if stream.send(format!("string-{}", i)).await.is_err() {
                return;
            }
        }
    }

    async fn slow_unary(&self, req: SlowRequest) -> Result<SlowResponse, RpcError> {
        let mut guard = OnDrop::new(|| {
            *self.last_status.borrow_mut() = "slow_unary: canceled".to_string();
        });

        *self.last_status.borrow_mut() = "slow_unary: started".to_string();

        tokio::time::sleep(Duration::from_millis(req.delay_ms as u64)).await;

        guard.defuse();
        *self.last_status.borrow_mut() = "slow_unary: completed".to_string();
        Ok(SlowResponse { completed: true })
    }

    async fn slow_stream(&self, req: SlowRequest, stream: StreamSender<StreamItem>) {
        let _guard = OnDrop::new(|| {
            *self.last_status.borrow_mut() = "slow_stream: canceled".to_string();
        });

        *self.last_status.borrow_mut() = "slow_stream: started".to_string();

        let mut i = 0u32;
        loop {
            tokio::time::sleep(Duration::from_millis(req.delay_ms as u64)).await;
            if stream
                .send(StreamItem {
                    seq: i,
                    data: format!("slow-item-{}", i),
                })
                .await
                .is_err()
            {
                return;
            }
            i += 1;
        }
    }

    async fn get_status(&self, _req: Empty) -> Result<String, RpcError> {
        Ok(self.last_status.borrow().clone())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let addr = listener.local_addr()?;

            // Print the address for the client to connect to
            println!("{}", addr);

            loop {
                let (stream, _) = listener.accept().await?;
                let transport = TcpTransport::new(stream);
                let impl_ = TestServiceImpl::new();
                let server = TestServiceServer::new(transport, impl_);

                tokio::task::spawn_local(async move {
                    if let Err(e) = server.serve().await {
                        eprintln!("Server error: {}", e);
                    }
                });
            }
        })
        .await
}
