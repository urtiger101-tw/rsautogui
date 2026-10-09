use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
    next: u64,
}
impl Client {
    pub fn start(path: &str) -> Self {
        let mut child = Command::new(path)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (send, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let value: Value =
                    serde_json::from_str(&line).expect("MCP stdout must contain only JSON");
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            output,
            next: 0,
        };
        let result=client.request("initialize",json!({"protocolVersion":"2025-11-25","clientInfo":{"name":"native-fixture","version":"1"},"capabilities":{}}));
        assert_eq!(result["protocolVersion"], "2025-11-25");
        client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        assert_eq!(
            client.request("tools/list", json!({}))["tools"]
                .as_array()
                .unwrap()
                .len(),
            12
        );
        client
    }
    pub fn send(&mut self, value: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{value}").unwrap();
        input.flush().unwrap();
    }
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params}));
        let value = self
            .output
            .recv_timeout(Duration::from_secs(15))
            .expect("MCP response within deadline");
        assert_eq!(value["id"], self.next);
        assert!(value.get("error").is_none(), "{value}");
        value["result"].clone()
    }
    pub fn raw_tool(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":args}))
    }
    pub fn cancelled_tool(&mut self, name: &str, args: Value) -> Value {
        // Late/unknown cancellations must not fill a permanent cancellation set.
        for id in 10_000..10_100 {
            self.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}));
        }
        self.next += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.next,"method":"tools/call","params":{"name":name,"arguments":args}}));
        std::thread::sleep(Duration::from_millis(100));
        self.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":self.next}}));
        let value = self
            .output
            .recv_timeout(Duration::from_secs(5))
            .expect("cooperative cancellation within deadline");
        assert_eq!(value["id"], self.next);
        value["result"].clone()
    }
    pub fn tool(&mut self, name: &str, args: Value) -> Value {
        let value = self.raw_tool(name, args);
        assert_eq!(value["isError"], false, "{value}");
        serde_json::from_str(value["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        drop(self.input.take());
        let start = std::time::Instant::now();
        while self.child.try_wait().ok().flatten().is_none()
            && start.elapsed() < Duration::from_secs(7)
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
