use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;
use turbine::Turbine;

#[derive(Parser)]
#[command(name = "turbine", about = "Multi-chain RPC proxy")]
struct Cli {
    /// Path to the TOML config file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Override the port from the config
    #[arg(short, long)]
    port: Option<u16>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .init();

    let turbine = Turbine::from_config(&cli.config).unwrap_or_else(|e| {
        eprintln!("Failed to load config: {}", e);
        std::process::exit(1);
    });

    let port = cli.port.unwrap_or(turbine.port());
    let host = turbine.host().to_string();
    let addr = format!("{}:{}", host, port);

    turbine.serve(&addr).await.unwrap_or_else(|e| {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    });
}
