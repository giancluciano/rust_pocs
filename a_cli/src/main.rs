use clap::Parser;
use std::io::{self, Write};

#[derive(Parser)]
struct Cli {
    pattern: String,
    path: std::path::PathBuf,
}

fn main() {

    let args = Cli::parse();
    let stdout = io::stdout();
    let mut handle = io::BufWriter::new(stdout);

    let _ = writeln!(handle, "pattern: {:?}, path: {:?}", args.pattern, args.path);
    let content = std::fs::read_to_string(&args.path).expect("could not read file");

    let pb = indicatif::ProgressBar::new(100);
    for i in 0..100 {
        pb.println(format!("[+] finished #{}", i));
        pb.inc(1);
    }
    pb.finish_with_message("done");
    for line in content.lines() {
        if line.contains(&args.pattern) {
            let _ = writeln!(handle, "{}", line);
        }
    }
}
