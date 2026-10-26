use std::path::Path;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        eprintln!("Usage: sync_assistant_recovery <consistent-backup-db-directory> <audited-events.jsonl> <new-output-directory>\nPrepares a copy only; no network requests or profile installation.");
        std::process::exit(2);
    }
    match aipp_lib::prepare_assistant_recovery(Path::new(&args[0]), Path::new(&args[1]), Path::new(&args[2])) {
        Ok(report) => println!("{}", serde_json::json!({"prepared":true,"counts":report["counts"],
            "scanner_changed":report["scanner_changed"].as_array().map(Vec::len),
            "scanner_untracked":report["scanner_untracked"].as_array().map(Vec::len),
            "unresolved":report["unresolved"].as_array().map(Vec::len),"cursor_advanced":false})),
        Err(error) => { eprintln!("Assistant recovery preparation failed: {error}"); std::process::exit(1); }
    }
}
