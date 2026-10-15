use kfx_core::{Config, server};

#[tokio::main]
async fn main() -> kfx_core::KfxResult<()> {
    let config = Config::default();
    println!("Starting Kinexfluence server on {}:{}", config.server.host, config.server.port);

    server::serve(config).await?;

    Ok(())
}
