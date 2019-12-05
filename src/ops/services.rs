//! Control development services.

use crate::ops::error::{ok, ExitError, OpResult};
use futures::prelude::*;
use slog_scope::{info, warn};
use std::path::{Path, PathBuf};
use std::fs::File;
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

#[derive(Debug, Deserialize)]
struct Service {
    name: String,
    path: PathBuf,
    args: Vec<String>,
}

/// See the documentation for lorri::cli::Command::Services.
pub fn main(config: &Path) -> OpResult {
    let services: Vec<Service> = match serde_json::from_reader(std::io::BufReader::new(File::open(config)?)) {
        Ok(services) => services,
        Err(e) => Err(ExitError::temporary(format!("{}", e)))?,
    };
    Runtime::new()?.block_on(main_async(services));
    ok()
}

async fn main_async(services: Vec<Service>) {
    let (mut service_tx, service_rx) = channel(1000);
    for service in services {
        service_tx.send(service).await.unwrap();
    }

    tokio::spawn(start_services(service_rx)).await.unwrap()
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

async fn start_services(mut service_rx: Receiver<Service>) {
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

// TESTING ONLY
async fn spawner(mut service_tx: Sender<Service>) {
    let mut id: u64 = 0;
    let duration = std::time::Duration::from_millis(1000);
    loop {
        tokio::time::delay_for(duration).await;
        let name = format!("echo {}", id);
        id += 1;
        service_tx
            .send(Service {
                name,
                path: PathBuf::from(
                    "/nix/store/fa4zygrvfq77gccqiyl9kixs05nfihk1-bash-interactive-4.4-p23/bin/bash",
                ),
                args: vec!["-c".to_string(), "echo start; sleep 2; echo hi; sleep 2; echo bye >&2".to_string()],
            })
            .await
            .unwrap();
    }
}