#!/bin/sh
# Install the prebuilt lagom binary from a GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/yuraware/lagom/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/yuraware/lagom/main/install.sh | sh -s -- --version 0.1.0
#
# It picks the archive for this OS and CPU, downloads it with SHA256SUMS,
# stops if the hash does not match, and copies `lagom` into
# ${LAGOM_INSTALL_DIR:-$HOME/.local/bin}. It never uses sudo and changes
# nothing else: run `lagom install` afterwards to add the agent hooks.
#
# Environment:
#   LAGOM_INSTALL_DIR  folder for the binary (default: ~/.local/bin)
#   LAGOM_VERSION      version to install, same as --version (default: latest release)
#   LAGOM_REPO_URL     where releases live (default: https://github.com/yuraware/lagom),
#                      for mirrors and tests
#
# Everything runs inside main(), so a download cut short never runs half a script.

set -eu

usage() {
    cat <<'EOF'
Install the lagom binary from a GitHub release.

Usage: install.sh [--version X.Y.Z]

Options:
  --version X.Y.Z  install this version instead of the latest release
  -h, --help       show this help

Environment:
  LAGOM_INSTALL_DIR  install folder (default: ~/.local/bin); never needs sudo
  LAGOM_VERSION      same as --version
  LAGOM_REPO_URL     release source (default: https://github.com/yuraware/lagom)
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

main() {
    version=${LAGOM_VERSION:-}
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
            -h | --help)
                usage
                exit 0
                ;;
            *) die "unknown option: $1 (see --help)" ;;
        esac
    done

    repo_url=${LAGOM_REPO_URL:-https://github.com/yuraware/lagom}
    repo_url=${repo_url%/}

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

    detect_target
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
    if ! { cp "$bin" "$staged" && chmod 755 "$staged" && mv -f "$staged" "$install_dir/lagom"; }; then
        rm -f "$staged"
        die "could not write $install_dir/lagom"
    fi

    dest=$install_dir/lagom
    if ! out=$("$dest" --version 2>&1); then
        die "installed $dest but it does not run: $out"
    fi
    say "installed $out to $dest"

    case ":${PATH:-}:" in
        *":$install_dir:"* | *":$install_dir/:"*)
            found=$(command -v lagom 2>/dev/null || true)
            if [ -n "$found" ] && [ "$found" != "$dest" ]; then
                warn "$found comes before $dest on PATH, so 'lagom' runs that one"
            fi
            run=lagom
            ;;
        *)
            warn "$install_dir is not on PATH. Add this line to your shell profile (~/.profile, ~/.bashrc or ~/.zshrc):"
            printf "\n    export PATH=\"%s:\$PATH\"\n\n" "$install_dir" >&2
            run=$dest
            ;;
    esac
    say "next, add the hooks to your agents with: $run install"
}

main "$@"
