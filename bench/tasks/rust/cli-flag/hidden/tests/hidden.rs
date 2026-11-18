use std::process::Command;

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_linecount")).args(args).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn counts_lines_or_words() {
    let dir = std::env::temp_dir().join(format!("linecount-hidden-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("text.txt");
    std::fs::write(&file, "a b c\nd  e\n").unwrap();
    let file = file.to_str().unwrap();
    assert_eq!(run(&[file]), "2 lines\n");
    assert_eq!(run(&["--words", file]), "5 words\n");
    assert_eq!(run(&[file, "--words"]), "5 words\n");
}
