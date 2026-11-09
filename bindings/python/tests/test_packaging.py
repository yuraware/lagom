"""Checks for the binary-only `lagom-ai` wheel (REL-09).

The static checks read pyproject.toml, Cargo.toml and the workflow, so they run
anywhere with Python 3.11+. The installed-binary checks run when the wheel is
installed in the current environment, which CI signals with
LAGOM_WHEEL_INSTALLED=1.
"""

import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

tomllib = pytest.importorskip("tomllib")

HERE = Path(__file__).resolve().parent
PKG = HERE.parent
ROOT = PKG.parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "python.yml"


def load(path):
    with open(path, "rb") as f:
        return tomllib.load(f)


@pytest.fixture(scope="module")
def pyproject():
    return load(PKG / "pyproject.toml")


@pytest.fixture(scope="module")
def cargo(pyproject):
    return load(PKG / pyproject["tool"]["maturin"]["manifest-path"])


def test_project_name_and_backend(pyproject):
    assert pyproject["project"]["name"] == "lagom-ai"
    assert pyproject["build-system"]["build-backend"] == "maturin"
    assert pyproject["tool"]["maturin"]["bindings"] == "bin"


def test_version_comes_from_cargo(pyproject, cargo):
    # A static version here would drift from the crate; maturin reads Cargo.toml.
    assert "version" not in pyproject["project"]
    assert "version" in pyproject["project"]["dynamic"]
    assert cargo["package"]["name"] == "lagom"
    assert re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?", cargo["package"]["version"])


def test_manifest_is_the_repo_crate(pyproject):
    manifest = (PKG / pyproject["tool"]["maturin"]["manifest-path"]).resolve()
    assert manifest == ROOT / "Cargo.toml"


def test_both_binaries_are_built():
    # INST-08: the wheel ships `lgm` next to `lagom`. Cargo finds src/main.rs
    # and src/bin/lgm.rs on its own, and maturin packages every binary.
    assert (ROOT / "src" / "main.rs").is_file()
    assert (ROOT / "src" / "bin" / "lgm.rs").is_file()


def test_license_matches_crate(pyproject, cargo):
    assert pyproject["project"]["license"] == cargo["package"]["license"] == "Apache-2.0"
    for name in pyproject["project"]["license-files"]:
        path = PKG / name
        assert path.is_file(), name
        assert path.resolve() == (ROOT / "LICENSE").resolve()
        assert "Apache License" in path.read_text(encoding="utf-8")


def test_readme_warns_against_ephemeral_installs(pyproject):
    readme = (PKG / pyproject["project"]["readme"]).read_text(encoding="utf-8")
    assert "pipx install lagom-ai" in readme
    assert "uvx --from lagom-ai lagom --version" in readme
    # Hooks store the binary's path, so `lagom install` from uvx breaks later.
    assert "don't run `lagom install` from one" in readme


@pytest.fixture(scope="module")
def workflow():
    return WORKFLOW.read_text(encoding="utf-8")


def test_workflow_builds_every_wheel(workflow):
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
    ]:
        assert f"target: {target}" in workflow, target
    assert workflow.count("manylinux: musllinux_1_2") == 2
    assert "command: sdist" in workflow


def test_workflow_uses_trusted_publishing(workflow):
    assert "id-token: write" in workflow
    assert "environment:" in workflow
    # No stored PyPI token anywhere.
    assert "password:" not in workflow
    assert "PYPI_API_TOKEN" not in workflow
    assert "secrets." not in workflow


def test_workflow_publishes_only_exact_version_tags(workflow):
    assert "needs.version.outputs.publish == 'true'" in workflow
    assert '"$crate") publish=true ;;' in workflow


def test_workflow_actions_are_pinned_by_sha(workflow):
    uses = re.findall(r"uses:\s*(\S+)", workflow)
    assert uses
    for ref in uses:
        assert re.fullmatch(r"[\w.-]+/[\w.-]+@[0-9a-f]{40}", ref), ref


installed = pytest.mark.skipif(
    os.environ.get("LAGOM_WHEEL_INSTALLED") != "1",
    reason="set LAGOM_WHEEL_INSTALLED=1 after installing the wheel",
)


def scripts_dir():
    import sysconfig

    return Path(sysconfig.get_path("scripts"))


@installed
@pytest.mark.parametrize("name", ["lagom", "lgm"])
def test_installed_binary_reports_crate_version(name, cargo):
    exe = scripts_dir() / (name + (".exe" if sys.platform == "win32" else ""))
    assert exe.is_file(), exe
    out = subprocess.run([str(exe), "--version"], capture_output=True, text=True, check=True)
    assert out.stdout.strip() == f"lagom {cargo['package']['version']}"


@installed
def test_installed_binary_is_native_not_a_python_shim():
    # HK-01: hooks must call the real binary. maturin's bin bindings install
    # the executable itself, so the file must not start with a `#!` line.
    exe = scripts_dir() / ("lagom.exe" if sys.platform == "win32" else "lagom")
    assert not exe.read_bytes().startswith(b"#!")
