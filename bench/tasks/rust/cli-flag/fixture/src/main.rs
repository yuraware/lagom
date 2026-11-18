use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: linecount FILE");
        return ExitCode::from(2);
    };
    match std::fs::read_to_string(path) {
        Ok(text) => {
            println!("{} lines", text.lines().count());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("linecount: {path}: {e}");
            ExitCode::from(1)
        }
    }
}
