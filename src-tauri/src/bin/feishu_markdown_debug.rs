use std::env;
use std::fs;
use std::io::{self, Read};

fn read_markdown_input(path: Option<&str>) -> Result<String, String> {
    match path {
        Some(path) => fs::read_to_string(path)
            .map_err(|error| format!("failed to read markdown file {}: {}", path, error)),
        None => {
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|error| format!("failed to read markdown from stdin: {}", error))?;
            Ok(buffer)
        }
    }
}

fn main() {
    let mut args = env::args().skip(1);
    let mut blocks_mode = false;
    let mut payload_mode = false;
    let mut input_path: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--blocks" => blocks_mode = true,
            "--payload" => payload_mode = true,
            "--help" | "-h" => {
                eprintln!(
                    "Usage: cargo run --manifest-path src-tauri\\Cargo.toml --bin feishu_markdown_debug -- [--blocks|--payload] [markdown-file]\n\
                     If no markdown file is provided, markdown is read from stdin."
                );
                return;
            }
            _ if input_path.is_none() => input_path = Some(arg),
            _ => {
                eprintln!("unexpected argument: {}", arg);
                std::process::exit(2);
            }
        }
    }

    let markdown = match read_markdown_input(input_path.as_deref()) {
        Ok(markdown) => markdown,
        Err(error) => {
            eprintln!("{}", error);
            std::process::exit(1);
        }
    };

    let output = if blocks_mode {
        Ok(aipp_lib::debug_describe_feishu_markdown_blocks(&markdown))
    } else if payload_mode {
        aipp_lib::debug_build_feishu_interactive_payload(&markdown)
    } else {
        aipp_lib::debug_build_feishu_markdown_card(&markdown)
    };

    match output {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(json) => println!("{}", json),
            Err(error) => {
                eprintln!("failed to render debug JSON: {}", error);
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("{}", error);
            std::process::exit(1);
        }
    }
}
