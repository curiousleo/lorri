use crate::ops::error::{ok, OpResult};
use futures::prelude::*;
use slog_scope::{info, warn};
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{channel, Receiver, Sender};

#[derive(Copy, Clone, Debug)]
enum Fd {
    Stdout,
    Stderr,
}

#[derive(Debug)]
struct Log {
    name: String,
    fd: Fd,
    message: String,
}

#[derive(Debug)]
struct Service<'a> {
    name: String,
    path: &'a Path,
    args: &'a [&'a str],
}

pub fn main() -> OpResult {
    Runtime::new()?.block_on(main_async());
    ok()
}

async fn spawner<'a>(mut service_tx: Sender<Service<'a>>) {
    let mut id: u64 = 0;
    let duration = std::time::Duration::from_millis(1000);
    loop {
        tokio::time::delay_for(duration).await;
        let name = format!("echo {}", id);
        id += 1;
        service_tx
            .send(Service {
                name,
                path: &Path::new(
                    "/nix/store/fa4zygrvfq77gccqiyl9kixs05nfihk1-bash-interactive-4.4-p23/bin/bash",
                ),
                args: &["-c", "echo start; sleep 2; echo hi; sleep 2; echo bye"],
            })
            .await
            .unwrap();
    }
}

async fn main_async() {
    let (mut service_tx, service_rx) = channel(1000);

    // HACK
    service_tx
        .send(Service {
            name: "echo 0000".to_string(),
            path: &Path::new(
                "/nix/store/fa4zygrvfq77gccqiyl9kixs05nfihk1-bash-interactive-4.4-p23/bin/bash",
            ),
            args: &["-c", "echo start; sleep 2; echo hi; sleep 2; echo bye"],
        })
        .await
        .unwrap();
    // HACK

    let spwn_handle = tokio::spawn(spawner(service_tx));
    let svc_handle = tokio::spawn(start_services(service_rx));

    futures::future::join(spwn_handle, svc_handle).await;
}

async fn to_log<'a, L: Stream<Item = tokio::io::Result<String>> + std::marker::Unpin>(
    mut lines: L,
    name: String,
    fd: Fd,
) {
    while let Some(Ok(message)) = lines.next().await {
        match fd {
            Fd::Stdout => info!("{}", message; "name" => &name),
            Fd::Stderr => warn!("{}", message; "name" => &name),
        }
    }
}

async fn start_services<'a>(mut service_rx: Receiver<Service<'a>>) {
    while let Some(service) = service_rx.recv().await {
        let mut child = Command::new(&service.path)
            .args(service.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        tokio::spawn(to_log(
            BufReader::new(child.stdout().take().unwrap()).lines(),
            service.name.to_string(),
            Fd::Stdout,
        ));
        tokio::spawn(to_log(
            BufReader::new(child.stderr().take().unwrap()).lines(),
            service.name.to_string(),
            Fd::Stderr,
        ));
    }
}
