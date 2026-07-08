extern crate cultivator_lib as cultivator;

use cultivator::preview::{open_text_preview_impl, read_text_preview_lines_impl};

#[tokio::main]
async fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "..\\README.md".to_string());
    eprintln!("Previewing {path}");

    let preview = open_text_preview_impl(path.clone()).await?;
    let lines = read_text_preview_lines_impl(path, 0, 40).await?;

    println!("{preview:#?}");
    println!("{lines:#?}");

    Ok(())
}
