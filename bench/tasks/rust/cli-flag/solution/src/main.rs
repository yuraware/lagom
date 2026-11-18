use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words = args.iter().any(|a| a == "--words");
    let Some(path) = args.iter().find(|a| *a != "--words") else {
        eprintln!("usage: linecount [--words] FILE");
        return ExitCode::from(2);
    };
    match std::fs::read_to_string(path) {
        Ok(text) if words => {
            println!("{} words", text.split_whitespace().count());
            ExitCode::SUCCESS
        }
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
