//! A node's routers served on an ephemeral local port, and JSON-RPC calls.

use arkiv_node::signed::{rpc, runtime::Shared};
use serde_json::{Value, json};

/// A router served until the value is dropped.
pub struct Server {
    pub url: String,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn serve(router: axum::Router) -> Server {
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", socket.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(socket, router).await.unwrap() });
    Server { url, task }
}

/// The response of the runtime's JSON-RPC handler to one request.
pub async fn call(rt: &Shared, method: &str, params: Value) -> Value {
    rpc::handle_request(
        rt,
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
    )
    .await
}
