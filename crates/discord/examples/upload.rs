use std::{env, error::Error, path::Path};

use rustycord_discord::Discord;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let mut args = env::args().skip(1);
    let token = args
        .next()
        .ok_or("usage: upload <token> <channel_id> <file>")?;
    let channel_id = args.next().ok_or("missing channel id")?;
    let file = args.next().ok_or("missing file path")?;

    let (client, me) = Discord::login(token).await?;
    println!("logged in as {} ({})", me.display_name(), me.id);

    let message = client.send_file(&channel_id, "", Path::new(&file)).await?;
    println!(
        "sent message {} with {} attachment(s)",
        message.id,
        message.attachments.len()
    );

    for attachment in &message.attachments {
        let bytes = client.download(&attachment.url).await?;
        println!(
            "downloaded {} ({} bytes, expected {})",
            attachment.filename,
            bytes.len(),
            attachment.size
        );
    }

    client.delete(&channel_id, &message.id).await?;
    println!("cleaned up test message");
    Ok(())
}
