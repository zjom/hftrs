use anyhow::Result;
use app::Config;
use clap::Parser;
use std::fs::File;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    let config = Config::parse();
    let envfilter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if let Some(ref p) = config.logfile {
        if let Ok(logfile) = File::open(p) {
            tracing_subscriber::fmt()
                .with_env_filter(envfilter)
                .with_writer(logfile)
                .init();
        }
    } else {
        tracing_subscriber::fmt().with_env_filter(envfilter).init();
    }

    app::run(config)
}
