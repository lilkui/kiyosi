Type: task
Status: ready-for-agent

# Build and test foundation

## Goal

Turn the scaffold into a C++23 CMake project named `ito` with a standard-library-only production target, Catch2 tests, CTest discovery, and reproducible Windows x64/Linux x64 configurations.

## Depends on

None.

## Acceptance criteria

- CMake requires the agreed modern floor and enables C++23.
- A static library target is built by default and an optional shared target is available.
- The installed/exported target uses the `ito` identity.
- Catch2 is pinned and test-only through CMake `FetchContent`.
- CTest discovers and runs the test executable.
- Debug and release configurations work on Windows x64 and Linux x64.
- Production targets have no mandatory third-party dependency.

## Test

Configure, build, install, and run CTest on both supported platform configurations.

## Comments

This ticket establishes packaging and testing seams; it does not add pricing behavior.
