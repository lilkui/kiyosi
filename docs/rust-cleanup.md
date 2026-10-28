# Rust delivery after legacy removal

Ticket: [38 — remove legacy and verify artifacts](../.scratch/rust-rewrite/issues/38-remove-legacy-and-verify-artifacts.md).
Starting commit: `39460e91265268832c4959ad537c7467b7bd5ea2`.
Verification started on 2026-10-28 and continued on 2026-10-29 (Asia/Shanghai).

Ticket 37's [completed cutover acceptance](rust-acceptance.md) was checked before
removing the legacy implementation. The CPU support matrix and numerical budgets
from that record remain applicable; this change does not alter pricing code.

## Final engineering tree

The Rust workspace contains the Python-independent `kiyosi-core` crate and its
separate PyO3 adapter. Python delivery uses maturin and the existing `kiyosi` name,
with version `0.13.0` supplied by Cargo. There is one CPU implementation and no
public C++ or C ABI, CUDA bridge, CUDA runtime, or oneTBB dependency.

Removed the C++ sources and public headers, CUDA kernels, nanobind package, CMake
build/install/export configuration, C++ tests and benchmark programs, their
comparison driver, obsolete stub generator, Doxygen instructions and dedicated
format/lint settings. Remaining Python benchmark helpers use the public Python
API; their performance gates are outside acceptance.

Independent QuantLib modules and JSON fixtures now live in
`rust/python/tests/quantlib_reference`; scalar price differences and shared
structured/calendar fixtures are in the same test tree. The Rust-backed suites
retain their numerical assertions and no longer import the legacy test tree.
Packaging and cibuildwheel copy these resources with the tests. Current READMEs,
test/API/calendar guidance and agent instructions describe the Rust CPU delivery.
ADRs and the earlier acceptance record retain migration history.

The Rust Sobol table retains all **397,013** direction-number entries from the
deleted Joe–Kuo table. All independent reference modules and JSON fixtures match
the old files after normalizing checkout line endings. Calendar data remains in
`rust/core/src/sse_holidays.rs`. The core and wheel retain their full applicable
PCG, Sobol, calendar and dependency licenses; wheel notices match the root and
binding copies. The deleted oneTBB license belonged to the removed distribution.

## Verification

Windows AMD64 and Ubuntu 26.04.1 x86-64 under WSL2 use Rust 1.99.1,
maturin 1.15.0 and QuantLib 1.43. The cleaned sdist supplies the builds and a
separate test tree containing exactly cibuildwheel's configured `test-sources`.
Each wheel is installed into a fresh environment with its required test extra;
the full Rust-backed suite runs against that installation, outside the checkout.

| CPython | Windows AMD64 | Ubuntu / WSL2 x86-64 |
| --- | --- | --- |
| 3.11.17 | Build/install/import; 444 passed | Build/install/import; 444 passed |
| 3.12.14 | Build/install/import; 444 passed | Build/install/import; 444 passed |
| 3.13.15 | Build/install/import; 444 passed | Build/install/import; 444 passed |
| 3.14.7 | Build/install/import; 444 passed | Build/install/import; 444 passed |
| 3.15.1 | Build/install/import; 444 passed | Build/install/import; 444 passed |

Windows and Linux each pass **389 Rust tests**, including **74 doctests**,
core check, warnings-denied rustdoc, Cargo package verification and the separate
Rust consumer. Core checks use an invalid `PYO3_PYTHON`. Both platforms pass
warnings-denied workspace Clippy. Linux also passes the explicit core build check
on Rust 1.85.0; this is not a full MSRV test run.

Rustfmt, Ruff lint and formatting, Python `ty`, and the live calendar generator's
`--check` pass. The core archive contains its full notices and both numerical and
calendar data. Reference JSON and modules retain their original numerical budgets.
Every ordinary wheel passes typed-payload, version, full license and native
dependency checks. `delvewheel show` and `auditwheel show` find only Python/system
libraries, with no CUDA, oneTBB or C++ runtime dependency. QuantLib remains a
test-only requirement. The sdist contains no C++ headers/sources, CUDA kernels,
legacy Python package or legacy test tree.

The main build/install/test commands, repeated for each interpreter and platform,
are:

```sh
uv build --sdist --out-dir target/ticket38/dist
uv build --wheel --python 3.15 --out-dir wheels path/to/kiyosi-0.13.0.tar.gz
uv venv --python 3.15 path/to/fresh-environment
uv pip install --python PATH_TO_FRESH_PYTHON 'PATH_TO_WHEEL[test]'
PATH_TO_FRESH_PYTHON -m unittest discover -s rust/python/tests -p "test_*.py"
```

The discovery command runs in the copied test-source tree. QuantLib is required,
and no numerical reference tests are skipped. Its settings remain isolated in
each test process. Early 3.11/3.12 builds were refreshed after correcting the
documentation path in package metadata; their final artifacts receive the same
full suite. No pricing or binding implementation changed during verification.
The final sdist also includes `tests/README.md`, which the bundled README links
to, and uses `docs/**/*.md` to include the documentation files rather than matching
only directories. Its Rust/Python runtime, data and test files are compared with the verified
source trees; the last archive changes concern documentation and its inclusion.
CPython 3.15 wheel builds from that final archive, reinstalls and installed-package
checks pass again on both platforms.

Local verification logs are under `target/ticket38` on Windows and
`~/kiyosi-ticket38` on Linux, with `-final311` and `-final312` directories for the
refreshed wheels. Consumable local artifacts are collected under
`target/ticket38/artifacts`: ten ordinary wheels, one sdist, the standalone Cargo
archive and their SHA-256 inventory. These are local builds, not published releases.

The checkout defaults to the core. Maturin intentionally removes workspace
`default-members` in its Python sdist, so core commands from an extracted sdist
must select `-p kiyosi-core` (or its manifest). The first unqualified extracted
sdist check attempted PyO3 and rejected the deliberately invalid Python path;
explicit core selection verifies the intended independent boundary.

## Review and environment limits

Independent Standards and Spec reviews against the starting commit report
**0 unresolved findings each**.

Local Linux wheels have ordinary `linux_x86_64` tags and audit as
`manylinux_2_35_x86_64`; Windows wheels use `win_amd64`. This does not verify
the release workflow's manylinux 2.28 image or GitHub-hosted CI. WSL2 is the Linux
environment tested, not a separate bare-metal Linux machine. Windows MSRV and
full MSRV suites are untested. No other architectures, ABI3 or free-threaded
interpreters are claimed. Legacy C++ and legacy Python suites were not run.

No performance baseline, timing threshold, release, crate publication, push or
change to the external documentation website is part of this work.
