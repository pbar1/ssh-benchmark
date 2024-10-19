#![warn(clippy::pedantic)]

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use russh::keys::key;
use russh::server;
use russh::server::Msg;
use russh::server::Server as _;
use russh::server::Session;
use russh::Channel;
use russh::ChannelId;
use russh::CryptoVec;
use tracing::error;
use tracing_glog::Glog;
use tracing_glog::GlogFields;
use tracing_glog::LocalTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

/// Dummy SSH server that uses as few resources as possible
#[derive(Debug, Parser)]
struct Cli {
    /// Port for SSH server to bind to.
    #[clap(short, long, default_value_t = 2222)]
    port: u16,

    /// Log level for stderr
    #[clap(short, long, default_value = "error", env = "RUST_LOG")]
    log_level: String,
}

impl Cli {
    fn init_tracing(&self) -> Result<()> {
        let stderr_filter = EnvFilter::builder().parse_lossy(&self.log_level);
        let stderr_layer = tracing_subscriber::fmt::layer()
            .event_format(Glog::default().with_timer(LocalTime::default()))
            .fmt_fields(GlogFields::default())
            .with_filter(stderr_filter);

        let subscriber = tracing_subscriber::registry().with(stderr_layer);
        tracing::subscriber::set_global_default(subscriber)?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    cli.init_tracing()?;

    let config = russh::server::Config {
        inactivity_timeout: Some(std::time::Duration::from_secs(60)),
        auth_rejection_time: std::time::Duration::from_secs(3),
        auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
        keys: vec![
            russh_keys::key::KeyPair::generate_ed25519().context("unable to generate keypair")?
        ],
        ..Default::default()
    };
    let config = Arc::new(config);
    let mut sh = Server;
    sh.run_on_address(config, ("0.0.0.0", cli.port)).await?;

    Ok(())
}

// SSH ------------------------------------------------------------------------

#[derive(Clone)]
struct Server;

impl server::Server for Server {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Self
    }
    fn handle_session_error(&mut self, error: <Self::Handler as russh::server::Handler>::Error) {
        error!(?error, "session error");
    }
}

#[async_trait]
impl server::Handler for Server {
    type Error = russh::Error;

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        _public_key: &key::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        Ok(server::Auth::Accept)
    }

    async fn auth_password(
        &mut self,
        _user: &str,
        _password: &str,
    ) -> Result<server::Auth, Self::Error> {
        Ok(server::Auth::Accept)
    }

    /// Simply return the requested command on stdout, and exit code 0.
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.data(channel, CryptoVec::from_slice(data));
        session.exit_status_request(channel, 0);
        session.eof(channel);
        session.close(channel);
        Ok(())
    }
}
