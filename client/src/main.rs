#![warn(clippy::pedantic)]

use std::collections::HashSet;
use std::io::IsTerminal;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::net::SocketAddrV4;
use std::net::ToSocketAddrs;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use futures::StreamExt;
use indicatif::ProgressStyle;
use tokimak::TaskReactor;
use tokio::io::AsyncWriteExt;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::instrument;
use tracing::Span;
use tracing_glog::Glog;
use tracing_glog::GlogFields;
use tracing_glog::LocalTime;
use tracing_indicatif::filter::IndicatifFilter;
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// Dummy SSH client that uses as many resources as possible
#[derive(Debug, Parser)]
struct Cli {
    /// Number of concurrent tasks to run at once
    #[clap(short, long, default_value_t = 50)]
    concurrency: usize,

    /// Log level for stderr
    #[clap(short, long, default_value = "info", env = "RUST_LOG")]
    log_level: String,
}

impl Cli {
    fn init_tracing(&self) -> Result<()> {
        if std::io::stderr().is_terminal() {
            let indicatif_layer = IndicatifLayer::new()
                .with_progress_style(ProgressStyle::default_bar())
                .with_filter(IndicatifFilter::new(false));

            let stderr_writer = indicatif_layer.inner().get_stderr_writer();
            let stderr_filter = EnvFilter::builder().parse_lossy(&self.log_level);
            let stderr_layer = tracing_subscriber::fmt::layer()
                .with_writer(stderr_writer)
                .event_format(Glog::default().with_timer(LocalTime::default()))
                .fmt_fields(GlogFields::default())
                .with_filter(stderr_filter);

            let console_layer = console_subscriber::spawn();

            let subscriber = tracing_subscriber::registry()
                .with(indicatif_layer)
                .with(stderr_layer)
                .with(console_layer);
            tracing::subscriber::set_global_default(subscriber)?;
        } else {
            let stderr_filter = EnvFilter::builder().parse_lossy(&self.log_level);
            let stderr_layer = tracing_subscriber::fmt::layer()
                .event_format(Glog::default().with_timer(LocalTime::default()))
                .fmt_fields(GlogFields::default())
                .with_ansi(false)
                .with_filter(stderr_filter);

            let console_layer = console_subscriber::spawn();

            let subscriber = tracing_subscriber::registry()
                .with(stderr_layer)
                .with(console_layer);
            tracing::subscriber::set_global_default(subscriber)?;
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    cli.init_tracing()?;

    real_main(cli).await?;

    Ok(())
}

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn gen_targets(exclude_local_ports: HashSet<u16>) -> Vec<(SocketAddr, SocketAddr)> {
    let mut local_addrs: Vec<SocketAddr> = Vec::new();
    for local_port in 1..65535 {
        if exclude_local_ports.contains(&local_port) {
            continue;
        }
        local_addrs.push(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, local_port).into());
    }

    let mut remote_addrs: Vec<SocketAddr> = Vec::new();
    for remote_idx in 0..1 {
        let remote_addr = format!("server-{remote_idx}.server:20000")
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();
        for i in 0..10 {
            let remote_port = 20000 + i;
            remote_addrs.push(SocketAddr::new(remote_addr.ip(), remote_port));
        }
    }

    let mut addrs = Vec::new();
    for local in local_addrs {
        for remote in remote_addrs.iter() {
            addrs.push((local, remote.to_owned()));
        }
    }

    addrs
}

#[instrument(skip_all, fields(indicatif.pb_show))]
async fn real_main(cli: Cli) -> Result<()> {
    let metrics = tokio::runtime::Handle::current().metrics();
    let parallelism = std::thread::available_parallelism().unwrap().get();
    info!(tokio_workers = metrics.num_workers(), parallelism);

    let addrs = gen_targets(HashSet::from([6669]));
    let jobs = addrs.len();
    info!(jobs);
    Span::current().pb_set_length(jobs as u64);

    let stream = futures::stream::iter(addrs.into_iter().enumerate())
        .map(|(n, (local, remote))| do_work(n, local, remote));

    let mut tasks = TaskReactor::buffer_spawned(cli.concurrency, stream);

    while let Some(result) = tasks.next().await {
        Span::current().pb_inc(1);
        if let Err(error) = result {
            error!(?error, "task join error");
        }
    }

    Ok(())
}

/// Simulates some work that takes some time and can time out
async fn do_work(n: usize, local: SocketAddr, remote: SocketAddr) {
    let total_tasks = ACTIVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let metrics = tokio::runtime::Handle::current().metrics();
    let runtime_tasks = metrics.num_alive_tasks();

    let result = tokio::time::timeout(
        Duration::from_secs(15),
        connect_and_run(local, remote, "user", "password", "whoami"),
    )
    .await;

    if n % 10_000 == 0 {
        match result {
            Ok(Ok(output)) => info!(
                n,
                total_tasks,
                runtime_tasks,
                exit_code = output.exit_code,
                stdout = String::from_utf8(output.stdout).unwrap().trim(),
                stderr = String::from_utf8(output.stderr).unwrap().trim(),
                "success"
            ),
            Ok(Err(error)) => error!(n, total_tasks, runtime_tasks, ?error, "failed"),
            Err(_err) => error!(n, total_tasks, runtime_tasks, "timeout"),
        }
    } else {
        match result {
            Ok(Ok(output)) => debug!(
                n,
                total_tasks,
                runtime_tasks,
                exit_code = output.exit_code,
                stdout = String::from_utf8(output.stdout).unwrap().trim(),
                stderr = String::from_utf8(output.stderr).unwrap().trim(),
                "success"
            ),
            Ok(Err(error)) => debug!(n, total_tasks, runtime_tasks, ?error, "failed"),
            Err(_err) => debug!(n, total_tasks, runtime_tasks, "timeout"),
        }
    }
    ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}

// SSH ------------------------------------------------------------------------

#[derive(Debug)]
struct RunOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<u32>,
}

async fn connect_and_run(
    local: SocketAddr,
    remote: SocketAddr,
    user: &str,
    password: &str,
    command: &str,
) -> Result<RunOutput> {
    let config = russh::client::Config {
        inactivity_timeout: Some(Duration::from_secs(30)),
        ..Default::default()
    };
    let config = Arc::new(config);

    let socket = tokio::net::TcpSocket::new_v4()?;
    socket.set_reuseport(true)?;
    socket.bind(local)?;
    let stream = socket.connect(remote).await?;

    let mut session = russh::client::connect_stream(config, stream, SshClientHandler).await?;

    session.authenticate_password(user, password).await?;

    let mut channel = session.channel_open_session().await?;
    channel.exec(true, command).await?;

    let mut exit_code = None;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    loop {
        // There's an event available on the session channel
        let Some(msg) = channel.wait().await else {
            break;
        };
        match msg {
            // Copy stdout
            russh::ChannelMsg::Data { ref data } => {
                stdout.write_all(data).await?;
                stdout.flush().await?;
            }
            // Copy stderr
            russh::ChannelMsg::ExtendedData { ref data, ext: 1 } => {
                stderr.write_all(data).await?;
                stderr.flush().await?;
            }
            // The command has returned an exit code
            russh::ChannelMsg::ExitStatus { exit_status } => {
                exit_code = Some(exit_status);
                // cannot leave the loop immediately, there might still be
                // more data to receive
            }
            _ => {}
        }
    }

    Ok(RunOutput {
        stdout,
        stderr,
        exit_code,
    })
}

struct SshClientHandler;

#[async_trait]
impl russh::client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh_keys::key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}
