#!/usr/bin/env python3
"""Print the Homebrew formula for a release, from its SHA256SUMS (REL-06).

    python3 -I scripts/homebrew_formula.py 0.2.0 SHA256SUMS > Formula/lagom.rb

The formula installs the prebuilt archive for the machine and checks its hash; only
`brew install --HEAD` builds from source. Standard library only.
"""
import re
import sys

REPO = "https://github.com/yuraware/lagom"
# (Homebrew block, CPU block, release target), in the order the formula lists them.
PLATFORMS = [
    ("on_macos", "on_arm", "aarch64-apple-darwin"),
    ("on_macos", "on_intel", "x86_64-apple-darwin"),
    ("on_linux", "on_arm", "aarch64-unknown-linux-musl"),
    ("on_linux", "on_intel", "x86_64-unknown-linux-musl"),
]


class FormulaError(Exception):
    pass


def parse_sums(text):
    """{archive name: sha256} from `sha256sum` output (`<hash>  <name>` or `<hash> *<name>`)."""
    sums = {}
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        m = re.fullmatch(r"([0-9a-f]{64}) [ *](\S+)", line.strip())
        if not m:
            raise FormulaError(f"SHA256SUMS:{n}: not '<sha256>  <file>': {line!r}")
        sums[m.group(2)] = m.group(1)
    return sums


def render(version, sums):
    # Final releases only: a pre-release must never become what `brew install` gets.
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise FormulaError(f"not a final release version: {version!r}")
    blocks = {}
    for os_block, cpu_block, target in PLATFORMS:
        name = f"lagom-{version}-{target}.tar.gz"
        if name not in sums:
            raise FormulaError(f"SHA256SUMS has no {name}")
        blocks.setdefault(os_block, []).append(
            f"    {cpu_block} do\n"
            f'      url "{REPO}/releases/download/v{version}/{name}"\n'
            f'      sha256 "{sums[name]}"\n'
            f"    end\n"
        )
    platforms = "\n".join(f"  {b} do\n{''.join(cpus)}  end\n" for b, cpus in blocks.items())
    return f'''# Written by scripts/homebrew_formula.py in yuraware/lagom at each release. Don't edit.
class Lagom < Formula
  desc "Fewer tokens, simpler answers for coding agents"
  homepage "{REPO}"
  version "{version}"
  license "Apache-2.0"

{platforms}
  head do
    url "{REPO}.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args # builds lagom and lgm
    else
      bin.install "lagom"
      bin.install_symlink "lagom" => "lgm"
    end
  end

  # Homebrew doesn't let a formula write to $HOME, so connecting the agents is a second step.
  def caveats
    <<~EOS
      Connect lagom to your coding agents (Claude Code, Codex, Cursor):
        lagom install
      Before `brew uninstall lagom`, remove its hooks and skills:
        lagom uninstall --all
    EOS
  end

  test do
    # A --HEAD build reports the version in Cargo.toml, not HEAD-<commit>.
    expected = version.head? ? /^lagom \\d+\\.\\d+\\.\\d+/ : "lagom #{{version}}"
    assert_match expected, shell_output("#{{bin}}/lagom --version")
    assert_match expected, shell_output("#{{bin}}/lgm --version")
  end
end
'''


def main(argv):
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    try:
        with open(argv[1], encoding="utf-8") as fh:
            sys.stdout.write(render(argv[0], parse_sums(fh.read())))
    except (OSError, FormulaError) as e:
        print(f"homebrew_formula: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
