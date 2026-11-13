//! Tests for `install.sh` against a fake release served from 127.0.0.1.
//!
//! Each test builds release archives in a temp folder, serves them with a tiny
//! HTTP server that mimics GitHub's `releases/latest` redirect, and runs the
//! script with a PATH that holds only the tools it needs plus fake `uname`,
//! `sysctl` and (for the Homebrew tests) `brew` commands. The packed `lagom`
//! is a script that logs `lagom install` calls to `$HOME/agent-calls`.
//! Nothing touches the network or the real HOME.
#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh");

/// Tools the script calls, linked into the test PATH when they exist.
/// The downloader and the hash tools are added per test.
const TOOLS: &[&str] = &[
    "awk", "cat", "chmod", "cp", "gzip", "ln", "mkdir", "mktemp", "mv", "readlink", "rm", "sed",
    "tail", "tar", "tr",
];

/// A stand-in for the `lagom` binary. `--version` prints the version;
/// `install` logs its arguments to `$HOME/agent-calls` and fails when
/// `$HOME/fail-agents` exists, the way a real install fails with no agents.
fn fake_lagom(version: &str) -> String {
    format!(
        "#!/bin/sh\n\
         case \"$1\" in\n\
         --version) echo \"lagom {version}\" ;;\n\
         install) echo \"$*\" >> \"$HOME/agent-calls\"\n\
           if [ -e \"$HOME/fail-agents\" ]; then echo 'no agents found' >&2; exit 1; fi\n\
           echo 'claude: installed' ;;\n\
         *) exit 2 ;;\n\
         esac\n"
    )
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

fn write_exe(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("lagom-install-sh-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["bin", "home", "tmp", "release"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        Sandbox { root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Fills `bin/` with the base tools, the given extra tools, and a fake
    /// `uname` and `sysctl` that report `os`, `arch` and `translated`.
    fn tools(&self, extra: &[&str], os: &str, arch: &str, translated: bool) {
        let bin = self.path("bin");
        for name in TOOLS.iter().chain(extra) {
            let real = which(name).unwrap_or_else(|| panic!("{name} is needed to run this test"));
            symlink(real, bin.join(name)).unwrap();
        }
        write_exe(
            &bin.join("uname"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in -s) echo {os} ;; -m) echo {arch} ;; *) exit 1 ;; esac\n"
            ),
        );
        let sysctl = if translated { "echo 1" } else { "exit 1" };
        write_exe(&bin.join("sysctl"), &format!("#!/bin/sh\n{sysctl}\n"));
    }

    /// Adds `releases/download/v<version>/` with an archive per target and a
    /// `SHA256SUMS` file. The packed `lagom` is a script that prints its version.
    fn release(&self, version: &str, targets: &[&str]) {
        let dir = self.path(&format!("release/download/v{version}"));
        fs::create_dir_all(&dir).unwrap();
        let mut sums = String::new();
        for target in targets {
            let name = format!("lagom-{version}-{target}");
            let stage = dir.join(&name);
            fs::create_dir_all(&stage).unwrap();
            write_exe(&stage.join("lagom"), &fake_lagom(version));
            let status = Command::new("tar")
                .arg("-czf")
                .arg(format!("{name}.tar.gz"))
                .arg(&name)
                .current_dir(&dir)
                .env("COPYFILE_DISABLE", "1")
                .status()
                .unwrap();
            assert!(status.success());
            fs::remove_dir_all(&stage).unwrap();
            let archive = format!("{name}.tar.gz");
            sums.push_str(&format!("{}  {archive}\n", sha256_hex(&dir.join(&archive))));
        }
        fs::write(dir.join("SHA256SUMS"), sums).unwrap();
    }

    fn run(&self, base_url: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg(SCRIPT)
            .args(args)
            .env_clear()
            .env("PATH", self.path("bin"))
            .env("HOME", self.path("home"))
            .env("TMPDIR", self.path("tmp"))
            .env("LAGOM_REPO_URL", base_url)
            .current_dir(&self.root)
            .stdin(Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    fn tmp_is_empty(&self) -> bool {
        fs::read_dir(self.path("tmp")).unwrap().next().is_none()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sha256_hex(file: &Path) -> String {
    let out = if let Some(tool) = which("sha256sum") {
        Command::new(tool).arg(file).output().unwrap()
    } else {
        Command::new("shasum")
            .args(["-a", "256"])
            .arg(file)
            .output()
            .unwrap()
    };
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()[..64].to_string()
}

/// The hash tool this machine has (the script needs one of them).
fn hash_tool() -> &'static str {
    if which("sha256sum").is_some() {
        "sha256sum"
    } else {
        "shasum"
    }
}

/// Serves `root/release` the way GitHub serves releases:
/// `/releases/latest` redirects to `/releases/tag/v<latest>`, and
/// `/releases/download/...` serves files. Returns the repo base URL.
fn serve(root: &Path, latest: Option<&str>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/yuraware/lagom", listener.local_addr().unwrap());
    let files = root.join("release");
    let latest = latest.map(str::to_string);
    let base_for_thread = base.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if line == "\r\n" || line == "\n" => break,
                    Ok(_) => {}
                }
            }
            let mut parts = request.split_whitespace();
            let method = parts.next().unwrap_or("");
            let path = parts.next().unwrap_or("");
            let head = method == "HEAD";
            let rel = path.strip_prefix("/yuraware/lagom").unwrap_or("");
            let (status, extra, body): (&str, String, Vec<u8>) = if rel == "/releases/latest" {
                match &latest {
                    Some(v) => (
                        "302 Found",
                        format!("Location: {base_for_thread}/releases/tag/v{v}\r\n"),
                        Vec::new(),
                    ),
                    None => (
                        "302 Found",
                        format!("Location: {base_for_thread}/releases\r\n"),
                        Vec::new(),
                    ),
                }
            } else if rel.starts_with("/releases/tag/") || rel == "/releases" {
                ("200 OK", String::new(), b"ok".to_vec())
            } else if let Some(file) = rel.strip_prefix("/releases/download/") {
                match (!file.contains("..")).then(|| fs::read(files.join("download").join(file))) {
                    Some(Ok(bytes)) => ("200 OK", String::new(), bytes),
                    _ => ("404 Not Found", String::new(), b"not found".to_vec()),
                }
            } else {
                ("404 Not Found", String::new(), b"not found".to_vec())
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            if !head {
                let _ = stream.write_all(&body);
            }
            let _ = stream.flush();
        }
    });
    base
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn assert_ok(o: &Output) {
    assert!(
        o.status.success(),
        "install.sh failed\nstdout:\n{}\nstderr:\n{}",
        stdout(o),
        stderr(o)
    );
}

fn installed_version(bin: &Path) -> String {
    let out = Command::new(bin).arg("--version").output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn installs_latest_release_with_curl_into_install_dir() {
    let sb = Sandbox::new("latest-curl");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    sb.release(
        "9.9.9",
        &["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"],
    );
    let url = serve(&sb.root, Some("9.9.9"));
    let dir = sb.path("opt/bin");
    let o = sb.run(&url, &[], &[("LAGOM_INSTALL_DIR", dir.to_str().unwrap())]);
    assert_ok(&o);
    let bin = dir.join("lagom");
    assert_eq!(installed_version(&bin), "lagom 9.9.9");
    assert_eq!(
        fs::metadata(&bin).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let out = stdout(&o);
    assert!(
        out.contains("lagom 9.9.9 for x86_64-unknown-linux-musl"),
        "{out}"
    );
    assert!(
        out.contains(&format!("{} install --yes", bin.display())),
        "{out}"
    );
    let err = stderr(&o);
    assert!(err.contains("is not on PATH"), "{err}");
    assert!(
        err.contains(&format!("export PATH=\"{}:$PATH\"", dir.display())),
        "{err}"
    );
    assert!(
        sb.tmp_is_empty(),
        "the temporary download folder was left behind"
    );
    // Only the binary and its `lgm` alias land in the install folder.
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["lagom", "lgm"]);
}

#[test]
fn installs_pinned_version_with_wget_on_apple_silicon() {
    if which("wget").is_none() {
        eprintln!("skipped: wget is not installed");
        return;
    }
    let sb = Sandbox::new("pinned-wget");
    sb.tools(&["wget", hash_tool()], "Darwin", "arm64", false);
    // Only the Apple silicon archive exists, so any other pick fails the download.
    sb.release("1.2.3-rc.1", &["aarch64-apple-darwin"]);
    let url = serve(&sb.root, Some("9.9.9"));
    let o = sb.run(&url, &["--version", "v1.2.3-rc.1"], &[]);
    assert_ok(&o);
    let bin = sb.path("home/.local/bin/lagom");
    assert_eq!(installed_version(&bin), "lagom 1.2.3-rc.1");
}

#[test]
fn wget_finds_the_latest_release_from_the_redirect() {
    if which("wget").is_none() {
        eprintln!("skipped: wget is not installed");
        return;
    }
    let sb = Sandbox::new("latest-wget");
    sb.tools(&["wget", hash_tool()], "Linux", "aarch64", false);
    sb.release("2.0.0", &["aarch64-unknown-linux-musl"]);
    let url = serve(&sb.root, Some("2.0.0"));
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert_eq!(
        installed_version(&sb.path("home/.local/bin/lagom")),
        "lagom 2.0.0"
    );
}

#[test]
fn rosetta_shell_gets_the_apple_silicon_build() {
    let sb = Sandbox::new("rosetta");
    sb.tools(&["curl", hash_tool()], "Darwin", "x86_64", true);
    sb.release("1.0.0", &["aarch64-apple-darwin"]);
    let url = serve(&sb.root, None);
    let o = sb.run(&url, &[], &[("LAGOM_VERSION", "1.0.0")]);
    assert_ok(&o);
    assert!(stdout(&o).contains("aarch64-apple-darwin"));
}

#[test]
fn intel_mac_gets_the_intel_build() {
    let sb = Sandbox::new("intel-mac");
    sb.tools(&["curl", hash_tool()], "Darwin", "x86_64", false);
    sb.release("1.0.0", &["x86_64-apple-darwin"]);
    let url = serve(&sb.root, None);
    assert_ok(&sb.run(&url, &["--version=1.0.0"], &[]));
}

#[test]
fn checksum_mismatch_installs_nothing_and_deletes_the_download() {
    let sb = Sandbox::new("mismatch");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    sb.release("1.0.0", &["x86_64-unknown-linux-musl"]);
    let sums = sb.path("release/download/v1.0.0/SHA256SUMS");
    let bad = format!(
        "{}  lagom-1.0.0-x86_64-unknown-linux-musl.tar.gz\n",
        "0".repeat(64)
    );
    fs::write(&sums, bad).unwrap();
    let url = serve(&sb.root, Some("1.0.0"));
    let o = sb.run(&url, &[], &[]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("checksum mismatch"), "{}", stderr(&o));
    assert!(!sb.path("home/.local/bin").exists());
    assert!(sb.tmp_is_empty());
}

#[test]
fn archive_missing_from_sha256sums_is_refused() {
    let sb = Sandbox::new("no-entry");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    sb.release("1.0.0", &["x86_64-unknown-linux-musl"]);
    fs::write(sb.path("release/download/v1.0.0/SHA256SUMS"), "").unwrap();
    let url = serve(&sb.root, None);
    let o = sb.run(&url, &["--version", "1.0.0"], &[]);
    assert!(!o.status.success());
    assert!(
        stderr(&o).contains("SHA256SUMS has no entry"),
        "{}",
        stderr(&o)
    );
    assert!(!sb.path("home/.local/bin").exists());
}

#[test]
fn without_a_hash_tool_nothing_is_installed() {
    let sb = Sandbox::new("no-hash");
    sb.tools(&["curl"], "Linux", "x86_64", false);
    sb.release("1.0.0", &["x86_64-unknown-linux-musl"]);
    let url = serve(&sb.root, None);
    let o = sb.run(&url, &["--version", "1.0.0"], &[]);
    assert!(!o.status.success());
    assert!(
        stderr(&o).contains("need sha256sum or shasum"),
        "{}",
        stderr(&o)
    );
    assert!(!sb.path("home/.local/bin").exists());
}

#[test]
fn missing_release_reports_the_version() {
    let sb = Sandbox::new("missing");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    let url = serve(&sb.root, None);
    let o = sb.run(&url, &["--version", "0.0.1"], &[]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("v0.0.1"), "{}", stderr(&o));
}

#[test]
fn no_release_yet_asks_for_a_version() {
    let sb = Sandbox::new("no-latest");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    let url = serve(&sb.root, None);
    let o = sb.run(&url, &[], &[]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("pass --version"), "{}", stderr(&o));
}

#[test]
fn unsupported_systems_point_to_cargo_install() {
    for (os, arch) in [
        ("FreeBSD", "amd64"),
        ("Linux", "armv7l"),
        ("MINGW64_NT-10.0", "x86_64"),
    ] {
        let sb = Sandbox::new(&format!("unsupported-{arch}"));
        sb.tools(&["curl", hash_tool()], os, arch, false);
        let url = serve(&sb.root, Some("1.0.0"));
        let o = sb.run(&url, &[], &[]);
        assert!(!o.status.success(), "{os} {arch}");
        assert!(stderr(&o).contains("cargo install lagom"), "{}", stderr(&o));
        assert!(!sb.path("home/.local/bin").exists());
    }
}

#[test]
fn rejects_versions_that_are_not_versions() {
    let sb = Sandbox::new("bad-version");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    let url = serve(&sb.root, None);
    for bad in ["../../evil", "1.0.0/../x", "latest", "1.0 0", "1.0.0?x"] {
        let o = sb.run(&url, &["--version", bad], &[]);
        assert!(!o.status.success(), "accepted {bad:?}");
        assert!(
            stderr(&o).contains("not a version"),
            "{bad:?}: {}",
            stderr(&o)
        );
    }
    for args in [&["--version"][..], &["--version", ""], &["--version="]] {
        let o = sb.run(&url, args, &[]);
        assert!(!o.status.success(), "accepted {args:?}");
        assert!(stderr(&o).contains("needs a value"), "{}", stderr(&o));
    }
    let o = sb.run(&url, &["--bogus"], &[]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("unknown option"));
}

#[test]
fn replaces_an_existing_binary_and_stays_quiet_when_on_path() {
    let sb = Sandbox::new("upgrade");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    sb.release("1.0.0", &["x86_64-unknown-linux-musl"]);
    sb.release("1.1.0", &["x86_64-unknown-linux-musl"]);
    let url = serve(&sb.root, Some("1.1.0"));
    let dir = sb.path("home/.local/bin");
    let path = format!("{}:{}", dir.display(), sb.path("bin").display());
    assert_ok(&sb.run(&url, &["--version", "1.0.0"], &[]));
    assert_eq!(installed_version(&dir.join("lagom")), "lagom 1.0.0");
    let o = sb.run(&url, &[], &[("PATH", &path)]);
    assert_ok(&o);
    assert_eq!(installed_version(&dir.join("lagom")), "lagom 1.1.0");
    assert!(!stderr(&o).contains("not on PATH"), "{}", stderr(&o));
    assert!(
        stdout(&o).contains("agents: lagom install --yes"),
        "{}",
        stdout(&o)
    );
}

#[test]
fn warns_when_another_lagom_comes_first_on_path() {
    let sb = Sandbox::new("shadowed");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    sb.release("1.0.0", &["x86_64-unknown-linux-musl"]);
    let url = serve(&sb.root, None);
    write_exe(&sb.path("bin/lagom"), "#!/bin/sh\necho old\n");
    let dir = sb.path("home/.local/bin");
    let path = format!("{}:{}", sb.path("bin").display(), dir.display());
    let o = sb.run(&url, &["--version", "1.0.0"], &[("PATH", &path)]);
    assert_ok(&o);
    assert!(stderr(&o).contains("comes before"), "{}", stderr(&o));
}

#[test]
fn expands_a_quoted_tilde_in_install_dir() {
    let sb = Sandbox::new("tilde");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    sb.release("1.0.0", &["x86_64-unknown-linux-musl"]);
    let url = serve(&sb.root, None);
    let o = sb.run(
        &url,
        &["--version", "1.0.0"],
        &[("LAGOM_INSTALL_DIR", "~/tools/")],
    );
    assert_ok(&o);
    assert!(sb.path("home/tools/lagom").is_file());
}

#[test]
fn help_prints_usage_without_downloading() {
    let sb = Sandbox::new("help");
    sb.tools(&[], "Linux", "x86_64", false);
    let o = sb.run("http://127.0.0.1:9/unused", &["--help"], &[]);
    assert_ok(&o);
    assert!(stdout(&o).contains("Usage: install.sh"));
}

#[test]
fn passes_shellcheck() {
    let Some(shellcheck) = which("shellcheck") else {
        // GitHub's Ubuntu runner ships shellcheck, so on Linux CI a missing one is an
        // error, not a quiet skip (REL-19). Elsewhere it is optional.
        assert!(
            !(cfg!(target_os = "linux") && std::env::var_os("CI").is_some()),
            "shellcheck is missing on Linux CI"
        );
        eprintln!("skipped: shellcheck is not installed");
        return;
    };
    let o = Command::new(shellcheck)
        .args(["--shell=sh", SCRIPT])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", stdout(&o));
}

// INST-02: agents, the `lgm` alias, upgrades and Homebrew.

fn agent_calls(sb: &Sandbox) -> String {
    fs::read_to_string(sb.path("home/agent-calls")).unwrap_or_default()
}

/// A Linux sandbox with release 1.0.0 published as the latest one.
fn linux_with_release(name: &str) -> (Sandbox, String) {
    let sb = Sandbox::new(name);
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    sb.release("1.0.0", &["x86_64-unknown-linux-musl"]);
    let url = serve(&sb.root, Some("1.0.0"));
    (sb, url)
}

#[test]
fn adds_lagom_to_the_agents_after_installing() {
    let (sb, url) = linux_with_release("agents");
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert_eq!(agent_calls(&sb), "install --yes\n");
    let out = stdout(&o);
    assert!(out.contains("claude: installed"), "{out}");
    assert!(out.contains("see what is installed with:"), "{out}");
}

#[test]
fn no_agents_installs_only_the_binary() {
    let (sb, url) = linux_with_release("no-agents");
    let o = sb.run(&url, &["--no-agents"], &[]);
    assert_ok(&o);
    assert!(sb.path("home/.local/bin/lagom").is_file());
    assert_eq!(agent_calls(&sb), "");
    assert!(
        stdout(&o).contains("next, add the hooks to your agents with:"),
        "{}",
        stdout(&o)
    );

    let o = sb.run(&url, &[], &[("LAGOM_NO_AGENTS", "1")]);
    assert_ok(&o);
    assert_eq!(agent_calls(&sb), "");

    // 0 means "not set", so the agents are added.
    let o = sb.run(&url, &[], &[("LAGOM_NO_AGENTS", "0")]);
    assert_ok(&o);
    assert_eq!(agent_calls(&sb), "install --yes\n");
}

#[test]
fn a_failed_agent_step_keeps_the_binary_and_says_how_to_finish() {
    let (sb, url) = linux_with_release("agents-fail");
    fs::write(sb.path("home/fail-agents"), "").unwrap();
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert_eq!(
        installed_version(&sb.path("home/.local/bin/lagom")),
        "lagom 1.0.0"
    );
    let err = stderr(&o);
    assert!(err.contains("no agents found"), "{err}");
    assert!(err.contains("did not finish"), "{err}");
    assert!(err.contains("install claude"), "{err}");
}

#[test]
fn a_second_run_is_already_up_to_date_and_downloads_nothing() {
    let (sb, url) = linux_with_release("up-to-date");
    assert_ok(&sb.run(&url, &[], &[]));
    // With the archive gone, a second download would fail.
    fs::remove_file(
        sb.path("release/download/v1.0.0/lagom-1.0.0-x86_64-unknown-linux-musl.tar.gz"),
    )
    .unwrap();
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    let out = stdout(&o);
    assert!(out.contains("lagom 1.0.0 is already up to date"), "{out}");
    assert!(!out.contains("downloading"), "{out}");
    // The agents are checked again; `lagom install` reports them as up to date.
    assert_eq!(agent_calls(&sb), "install --yes\ninstall --yes\n");
}

#[test]
fn a_second_run_upgrades_to_a_newer_release() {
    let sb = Sandbox::new("upgrade-latest");
    sb.tools(&["curl", hash_tool()], "Linux", "x86_64", false);
    sb.release("1.0.0", &["x86_64-unknown-linux-musl"]);
    sb.release("1.1.0", &["x86_64-unknown-linux-musl"]);
    let url = serve(&sb.root, Some("1.1.0"));
    assert_ok(&sb.run(&url, &["--version", "1.0.0"], &[]));
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert!(
        stdout(&o).contains("downloading lagom 1.1.0"),
        "{}",
        stdout(&o)
    );
    assert_eq!(
        installed_version(&sb.path("home/.local/bin/lgm")),
        "lagom 1.1.0"
    );
}

#[test]
fn lgm_is_a_symlink_to_lagom_and_survives_a_rerun() {
    let (sb, url) = linux_with_release("lgm");
    assert_ok(&sb.run(&url, &[], &[]));
    let lgm = sb.path("home/.local/bin/lgm");
    assert_eq!(fs::read_link(&lgm).unwrap(), Path::new("lagom"));
    assert_eq!(installed_version(&lgm), "lagom 1.0.0");
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert!(!stdout(&o).contains("note:"), "{}", stdout(&o));
    assert_eq!(fs::read_link(&lgm).unwrap(), Path::new("lagom"));
}

#[test]
fn an_existing_lgm_file_is_left_alone() {
    let (sb, url) = linux_with_release("lgm-file");
    let dir = sb.path("home/.local/bin");
    fs::create_dir_all(&dir).unwrap();
    write_exe(&dir.join("lgm"), "#!/bin/sh\necho pulsar\n");
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert_eq!(
        fs::read_to_string(dir.join("lgm")).unwrap(),
        "#!/bin/sh\necho pulsar\n"
    );
    assert!(
        stdout(&o).contains("lgm alias was not added"),
        "{}",
        stdout(&o)
    );
}

#[test]
fn another_lgm_on_path_means_no_alias() {
    let (sb, url) = linux_with_release("lgm-path");
    write_exe(&sb.path("bin/lgm"), "#!/bin/sh\necho pulsar\n");
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert!(!sb.path("home/.local/bin/lgm").exists());
    let out = stdout(&o);
    assert!(out.contains("another lgm command"), "{out}");
}

/// Adds a fake `brew` to the sandbox. `brew info --json=v2` reports the
/// formula at `version` (or fails when `$HOME/brew-no-tap` exists),
/// `brew install|upgrade` copies a fake lagom into `$HOME/brew/bin` (or fails
/// when `$HOME/brew-fails` exists), `brew list` succeeds once it is there, and
/// every call is logged to `$HOME/brew-calls`.
fn fake_brew(sb: &Sandbox, version: &str) {
    let src = sb.path("brew-src");
    fs::create_dir_all(&src).unwrap();
    write_exe(&src.join("lagom"), &fake_lagom(version));
    let script = format!(
        "#!/bin/sh\n\
         echo \"$*\" >> \"$HOME/brew-calls\"\n\
         prefix=\"$HOME/brew\"\n\
         case \"$1\" in\n\
         --prefix) echo \"$prefix\" ;;\n\
         list) [ -x \"$prefix/bin/lagom\" ] ;;\n\
         info)\n\
           if [ -e \"$HOME/brew-no-tap\" ]; then echo 'Error: No available formula' >&2; exit 1; fi\n\
           printf '{{\\n  \"formulae\": [{{\\n    \"name\": \"lagom\",\\n    \"versions\": {{\\n      \"stable\": \"{version}\",\\n      \"head\": null\\n    }}\\n  }}]\\n}}\\n' ;;\n\
         install|upgrade)\n\
           if [ -e \"$HOME/brew-fails\" ]; then echo 'Error: no tap' >&2; exit 1; fi\n\
           mkdir -p \"$prefix/bin\" && cp '{}' \"$prefix/bin/lagom\" ;;\n\
         *) exit 1 ;;\n\
         esac\n",
        src.join("lagom").display()
    );
    write_exe(&sb.path("bin/brew"), &script);
}

fn brew_calls(sb: &Sandbox) -> String {
    fs::read_to_string(sb.path("home/brew-calls")).unwrap_or_default()
}

#[test]
fn macos_with_homebrew_installs_and_then_upgrades_through_the_tap() {
    let sb = Sandbox::new("brew");
    sb.tools(&["curl", hash_tool()], "Darwin", "arm64", false);
    fake_brew(&sb, "1.0.0");
    // The tap is at the latest release, so Homebrew installs it (no archive is served).
    let url = serve(&sb.root, Some("1.0.0"));
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert!(brew_calls(&sb).contains("install yuraware/tap/lagom"));
    assert!(
        stdout(&o).contains("is installed with Homebrew"),
        "{}",
        stdout(&o)
    );
    assert!(!sb.path("home/.local/bin").exists());
    assert_eq!(agent_calls(&sb), "install --yes\n");

    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert!(brew_calls(&sb).contains("upgrade yuraware/tap/lagom"));
    assert!(!sb.path("home/.local/bin").exists());
}

#[test]
fn homebrew_behind_the_latest_release_or_without_the_tap_is_skipped() {
    for (name, brew_version, no_tap, says) in [
        (
            "brew-behind",
            "0.9.0",
            false,
            "is 0.9.0, not the latest release 1.0.0",
        ),
        (
            "brew-no-tap",
            "1.0.0",
            true,
            "has no yuraware/tap/lagom formula",
        ),
    ] {
        let sb = Sandbox::new(name);
        sb.tools(&["curl", hash_tool()], "Darwin", "arm64", false);
        fake_brew(&sb, brew_version);
        if no_tap {
            fs::write(sb.path("home/brew-no-tap"), "").unwrap();
        }
        sb.release("1.0.0", &["aarch64-apple-darwin"]);
        let url = serve(&sb.root, Some("1.0.0"));
        let o = sb.run(&url, &[], &[]);
        assert_ok(&o);
        assert!(stdout(&o).contains(says), "{name}: {}", stdout(&o));
        let calls = brew_calls(&sb);
        assert!(
            !calls.contains("install") && !calls.contains("upgrade"),
            "{name}: {calls}"
        );
        assert_eq!(
            installed_version(&sb.path("home/.local/bin/lagom")),
            "lagom 1.0.0",
            "{name}"
        );
    }
}

#[test]
fn homebrew_failure_falls_back_to_the_verified_archive() {
    let sb = Sandbox::new("brew-fails");
    sb.tools(&["curl", hash_tool()], "Darwin", "arm64", false);
    fake_brew(&sb, "1.0.0");
    fs::write(sb.path("home/brew-fails"), "").unwrap();
    sb.release("1.0.0", &["aarch64-apple-darwin"]);
    let url = serve(&sb.root, Some("1.0.0"));
    let o = sb.run(&url, &[], &[]);
    assert_ok(&o);
    assert!(
        stderr(&o).contains("using the release archive instead"),
        "{}",
        stderr(&o)
    );
    assert_eq!(
        installed_version(&sb.path("home/.local/bin/lagom")),
        "lagom 1.0.0"
    );
}

#[test]
fn pinned_version_install_dir_and_no_brew_skip_homebrew() {
    type Case<'a> = (&'a str, &'a [&'a str], &'a [(&'a str, &'a str)]);
    let cases: [Case; 4] = [
        ("brew-pinned", &["--version", "1.0.0"], &[]),
        ("brew-dir", &[], &[("LAGOM_INSTALL_DIR", "~/tools")]),
        ("brew-flag", &["--no-brew"], &[]),
        ("brew-env", &[], &[("LAGOM_NO_BREW", "1")]),
    ];
    for (name, args, env) in cases {
        let sb = Sandbox::new(name);
        sb.tools(&["curl", hash_tool()], "Darwin", "arm64", false);
        fake_brew(&sb, "9.9.9");
        sb.release("1.0.0", &["aarch64-apple-darwin"]);
        let url = serve(&sb.root, Some("1.0.0"));
        let o = sb.run(&url, args, env);
        assert_ok(&o);
        assert_eq!(brew_calls(&sb), "", "{name}");
        assert!(stdout(&o).contains("downloading lagom 1.0.0"), "{name}");
    }
}

#[test]
fn linux_never_uses_homebrew() {
    let (sb, url) = linux_with_release("brew-linux");
    fake_brew(&sb, "9.9.9");
    assert_ok(&sb.run(&url, &[], &[]));
    assert_eq!(brew_calls(&sb), "");
    assert_eq!(
        installed_version(&sb.path("home/.local/bin/lagom")),
        "lagom 1.0.0"
    );
}
