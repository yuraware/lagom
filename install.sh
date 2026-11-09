#!/bin/sh
# Install lagom and add it to every coding agent found on this machine.
#
#   curl -fsSL https://raw.githubusercontent.com/yuraware/lagom/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/yuraware/lagom/main/install.sh | sh -s -- --version 0.1.0
#
# On macOS with Homebrew it runs `brew install yuraware/tap/lagom` (or
# `brew upgrade`). Everywhere else, and when Homebrew fails, it picks the
# release archive for this OS and CPU, downloads it with SHA256SUMS, stops if
# the hash does not match, and copies `lagom` into
# ${LAGOM_INSTALL_DIR:-$HOME/.local/bin} with an `lgm` alias next to it. It
# never uses sudo. Then it runs `lagom install --yes`, which adds the hooks to
# each agent it finds and prints one line per agent.
#
# Running it again is safe: it upgrades to the latest release, or says the
# installed one is already up to date.
#
# Environment:
#   LAGOM_INSTALL_DIR  folder for the binary (default: ~/.local/bin); skips Homebrew
#   LAGOM_VERSION      version to install, same as --version (default: latest release)
#   LAGOM_NO_AGENTS    set to 1 for the same effect as --no-agents
#   LAGOM_NO_BREW      set to 1 for the same effect as --no-brew
#   LAGOM_REPO_URL     where releases live (default: https://github.com/yuraware/lagom),
#                      for mirrors and tests
#
# Everything runs inside main(), so a download cut short never runs half a script.

set -eu

usage() {
    cat <<'EOF'
Install lagom and add it to the coding agents on this machine.

Usage: install.sh [--version X.Y.Z] [--no-agents] [--no-brew]

Options:
  --version X.Y.Z  install this version instead of the latest release
                   (uses the release archive, not Homebrew)
  --no-agents      install the binary only; add the hooks later with `lagom install`
  --no-brew        on macOS, use the release archive even when Homebrew is present
  -h, --help       show this help

Environment:
  LAGOM_INSTALL_DIR  install folder (default: ~/.local/bin); never needs sudo.
                     Setting it uses the release archive, not Homebrew
  LAGOM_VERSION      same as --version
  LAGOM_NO_AGENTS    1 = same as --no-agents
  LAGOM_NO_BREW      1 = same as --no-brew
  LAGOM_REPO_URL     release source (default: https://github.com/yuraware/lagom)

Run it again to upgrade; it says when lagom is already up to date.
EOF
}

say() {
    printf 'lagom-install: %s\n' "$*"
}

warn() {
    printf 'lagom-install: warning: %s\n' "$*" >&2
}

die() {
    printf 'lagom-install: error: %s\n' "$*" >&2
    exit 1
}

# Sets `target` to the Rust target triple of the release archive for this machine.
detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case $os in
        Darwin)
            os=apple-darwin
            # An x86_64 shell under Rosetta on Apple silicon still gets the native build.
            if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
                arch=arm64
            fi
            ;;
        Linux) os=unknown-linux-musl ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            die "install.sh does not support Windows yet; build from source with: cargo install lagom"
            ;;
        *) die "there is no prebuilt lagom for $os; build from source with: cargo install lagom" ;;
    esac
    case $arch in
        x86_64 | amd64) arch=x86_64 ;;
        arm64 | aarch64) arch=aarch64 ;;
        *) die "there is no prebuilt lagom for $(uname -s) on $arch; build from source with: cargo install lagom" ;;
    esac
    target=$arch-$os
}

# Sets `downloader` to curl or wget, and `https_only` when the source is HTTPS.
pick_downloader() {
    if command -v curl >/dev/null 2>&1; then
        downloader=curl
    elif command -v wget >/dev/null 2>&1; then
        downloader=wget
    else
        die "need curl or wget to download lagom"
    fi
    case $repo_url in
        https://*) https_only=1 ;;
        *) https_only= ;;
    esac
}

# fetch URL FILE: download URL to FILE, following redirects; fails on HTTP errors.
fetch() {
    if [ "$downloader" = curl ]; then
        if [ -n "$https_only" ]; then
            curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 -o "$2" "$1"
        else
            curl -fsSL --retry 3 -o "$2" "$1"
        fi
    else
        wget -q -O "$2" "$1"
    fi
}

# Prints the version of the latest release, read from the redirect of
# <repo>/releases/latest to <repo>/releases/tag/v<version>. This avoids the
# GitHub API and its rate limit.
latest_version() {
    url=$repo_url/releases/latest
    if [ "$downloader" = curl ]; then
        if [ -n "$https_only" ]; then
            final=$(curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 -o /dev/null -w '%{url_effective}' "$url") || final=
        else
            final=$(curl -fsSL --retry 3 -o /dev/null -w '%{url_effective}' "$url") || final=
        fi
    else
        final=$(wget -S --spider "$url" 2>&1 | sed -n 's/^ *[Ll]ocation: *\([^ ]*\).*/\1/p' | tr -d '\r' | tail -n 1) || final=
    fi
    case $final in
        */releases/tag/*) ;;
        *) die "could not find the latest release at $repo_url/releases; pass --version X.Y.Z" ;;
    esac
    tag=${final##*/releases/tag/}
    tag=${tag%%[?#]*}
    printf '%s\n' "${tag#v}"
}

# sha256 FILE: prints the lowercase hex SHA-256 of FILE.
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sum=$(sha256sum "$1")
    elif command -v shasum >/dev/null 2>&1; then
        sum=$(shasum -a 256 "$1")
    else
        return 1
    fi
    printf '%s\n' "${sum%% *}" | tr 'A-F' 'a-f'
}


# Succeeds when $1 is set to anything other than empty or 0.
is_on() {
    case ${1:-} in
        '' | 0) return 1 ;;
        *) return 0 ;;
    esac
}

# Sets `install_dir` to an absolute folder without a trailing slash.
resolve_install_dir() {
    if [ -n "${LAGOM_INSTALL_DIR:-}" ]; then
        install_dir=$LAGOM_INSTALL_DIR
    elif [ -n "${HOME:-}" ]; then
        install_dir=$HOME/.local/bin
    else
        die "HOME is not set; set LAGOM_INSTALL_DIR to the folder for the lagom binary"
    fi
    case $install_dir in
        \~ | \~/*) install_dir=${HOME:?HOME is not set}${install_dir#\~} ;;
    esac
    case $install_dir in
        /*) ;;
        *) install_dir=$(pwd)/$install_dir ;;
    esac
    install_dir=${install_dir%/}
    [ -n "$install_dir" ] || install_dir=/
}

# Installs or upgrades lagom with Homebrew and sets `dest`. Returns 1 when
# Homebrew could not install it, so the caller can use the archive instead.
# The formula adds the `lgm` alias itself.
brew_install() {
    formula=yuraware/tap/lagom
    if brew list --versions "$formula" >/dev/null 2>&1; then
        say "upgrading with Homebrew: brew upgrade $formula"
        # brew prints "already installed" when there is nothing newer.
        brew upgrade "$formula" || warn "brew upgrade $formula failed; keeping the installed version"
    else
        say "installing with Homebrew: brew install $formula"
        brew install "$formula" || return 1
    fi
    prefix=$(brew --prefix 2>/dev/null) || return 1
    dest=$prefix/bin/lagom
    [ -x "$dest" ] || return 1
    out=$("$dest" --version 2>&1) || return 1
    say "$out is installed with Homebrew at $dest"
}

# Installs the verified release archive into `install_dir` and sets `dest`.
install_archive() {
    resolve_install_dir
    pick_downloader

    if [ -z "$version" ]; then
        version=$(latest_version)
    fi
    version=${version#v}
    # The version goes into URLs and paths, so allow only what a SemVer tag can hold.
    case $version in
        [0-9]*) ;;
        *) die "not a version: '$version' (expected something like 0.1.0)" ;;
    esac
    case $version in
        *[!0-9A-Za-z.+-]*) die "not a version: '$version' (expected something like 0.1.0)" ;;
    esac

    dest=$install_dir/lagom
    if [ -f "$dest" ] && [ -x "$dest" ] && [ "$("$dest" --version 2>/dev/null || true)" = "lagom $version" ]; then
        say "lagom $version is already up to date in $install_dir"
        return 0
    fi

    name=lagom-$version-$target
    archive=$name.tar.gz
    base=$repo_url/releases/download/v$version

    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t lagom-install)
    [ -n "$tmp" ] && [ -d "$tmp" ] || die "could not create a temporary folder"
    trap 'rm -rf "$tmp"' EXIT
    trap 'rm -rf "$tmp"; exit 1' HUP INT TERM

    say "downloading lagom $version for $target"
    fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" ||
        die "could not download $base/SHA256SUMS; check that v$version is a lagom release"
    fetch "$base/$archive" "$tmp/$archive" ||
        die "could not download $base/$archive"

    expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print tolower($1); exit }' "$tmp/SHA256SUMS")
    [ -n "$expected" ] || die "SHA256SUMS has no entry for $archive"
    actual=$(sha256 "$tmp/$archive") ||
        die "need sha256sum or shasum to check the download; nothing was installed"
    if [ "$actual" != "$expected" ]; then
        rm -f "$tmp/$archive"
        die "checksum mismatch for $archive (expected $expected, got $actual); deleted the download and installed nothing"
    fi

    tar -xzf "$tmp/$archive" -C "$tmp" || die "could not unpack $archive"
    bin=$tmp/$name/lagom
    [ -f "$bin" ] || die "$archive has no $name/lagom"

    mkdir -p "$install_dir" 2>/dev/null ||
        die "could not create $install_dir; set LAGOM_INSTALL_DIR to a folder you own (this script never uses sudo)"
    [ -w "$install_dir" ] ||
        die "$install_dir is not writable; set LAGOM_INSTALL_DIR to a folder you own (this script never uses sudo)"
    # Copy next to the target, then rename, so a running lagom is replaced in one step.
    staged=$install_dir/.lagom.install.$$
    if ! { cp "$bin" "$staged" && chmod 755 "$staged" && mv -f "$staged" "$dest"; }; then
        rm -f "$staged"
        die "could not write $dest"
    fi

    if ! out=$("$dest" --version 2>&1); then
        die "installed $dest but it does not run: $out"
    fi
    say "installed $out to $dest"
}

# Adds `lgm` as a symlink to lagom in `install_dir`, unless some other `lgm`
# exists (for example the Pulsar CLI of the same name). Never replaces one.
link_lgm() {
    alias_path=$install_dir/lgm
    if [ -L "$alias_path" ] && [ "$(readlink "$alias_path" 2>/dev/null || true)" = lagom ]; then
        return 0
    fi
    if [ -e "$alias_path" ] || [ -L "$alias_path" ]; then
        say "note: $alias_path is not lagom, so the lgm alias was not added; use lagom instead"
        return 0
    fi
    other=$(command -v lgm 2>/dev/null || true)
    if [ -n "$other" ]; then
        say "note: $other is another lgm command, so the lgm alias was not added; use lagom instead"
        return 0
    fi
    if ln -s lagom "$alias_path" 2>/dev/null; then
        say "added the lgm alias: $alias_path"
    else
        warn "could not add the lgm alias at $alias_path; use lagom instead"
    fi
}

# Warns when the folder of `dest` is not on PATH or another lagom comes first,
# and sets `run` to the command the user should type.
check_path() {
    bindir=${dest%/*}
    case ":${PATH:-}:" in
        *":$bindir:"* | *":$bindir/:"*)
            found=$(command -v lagom 2>/dev/null || true)
            if [ -n "$found" ] && [ "$found" != "$dest" ]; then
                warn "$found comes before $dest on PATH, so 'lagom' runs that one"
                run=$dest
            else
                run=lagom
            fi
            ;;
        *)
            warn "$bindir is not on PATH. Add this line to your shell profile (~/.profile, ~/.bashrc or ~/.zshrc):"
            printf "\n    export PATH=\"%s:\$PATH\"\n\n" "$bindir" >&2
            run=$dest
            ;;
    esac
}

# Runs `lagom install --yes`, which adds the hooks to every agent found and
# prints one line per agent. A failure here leaves the binary installed and
# tells the user how to finish by hand.
add_agents() {
    if [ -n "$no_agents" ]; then
        say "next, add the hooks to your agents with: $run install"
        return 0
    fi
    say "adding lagom to your agents: $run install --yes"
    if "$dest" install --yes </dev/null; then
        say "done; see what is installed with: $run status"
    else
        warn "lagom is installed, but adding it to your agents did not finish."
        warn "run '$run install' to see why, or name the agents, for example: $run install claude"
    fi
}

main() {
    version=${LAGOM_VERSION:-}
    no_agents=
    no_brew=
    if is_on "${LAGOM_NO_AGENTS:-}"; then no_agents=1; fi
    if is_on "${LAGOM_NO_BREW:-}"; then no_brew=1; fi
    while [ $# -gt 0 ]; do
        case $1 in
            --version)
                [ $# -ge 2 ] && [ -n "$2" ] || die "--version needs a value, for example --version 0.1.0"
                version=$2
                shift 2
                ;;
            --version=*)
                version=${1#--version=}
                [ -n "$version" ] || die "--version needs a value, for example --version 0.1.0"
                shift
                ;;
            --no-agents)
                no_agents=1
                shift
                ;;
            --no-brew)
                no_brew=1
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            *) die "unknown option: $1 (see --help)" ;;
        esac
    done

    repo_url=${LAGOM_REPO_URL:-https://github.com/yuraware/lagom}
    repo_url=${repo_url%/}

    detect_target

    # Homebrew only for the plain "latest, default place" install on macOS:
    # a pinned version or a chosen folder needs the archive.
    use_brew=
    case $target in
        *-apple-darwin)
            if [ -z "$no_brew" ] && [ -z "$version" ] && [ -z "${LAGOM_INSTALL_DIR:-}" ] &&
                command -v brew >/dev/null 2>&1; then
                use_brew=1
            fi
            ;;
    esac

    via_archive=1
    if [ -n "$use_brew" ]; then
        if brew_install; then
            via_archive=
        else
            warn "Homebrew could not install yuraware/tap/lagom; using the release archive instead"
        fi
    fi
    if [ -n "$via_archive" ]; then
        install_archive
        link_lgm
    fi

    check_path
    add_agents
}

main "$@"
