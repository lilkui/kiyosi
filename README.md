# kiyosi

Kiyosi is a C++23 quantitative finance library for pricing vanilla and exotic
derivatives. It provides market data types, option instruments, calendars,
and analytic, binomial, finite-difference, integral, and Monte Carlo pricing
engines.

The Python bindings have their own guide at
[`python/README.md`](python/README.md).

## Requirements

- CMake 3.28 or newer
- A C++23 compiler
- Ninja

On Windows, activate a Visual Studio Developer PowerShell before configuring
the project so that `cl.exe` is available.

## Build and test

Use the preset for your platform and configuration:

```bash
cmake --preset windows-release
cmake --build --preset windows-release
ctest --preset windows-release
```

On Linux, replace `windows-release` with `linux-release`.

Debug and sanitizer presets are also available in `CMakePresets.json`.

## Install and use from CMake

```bash
cmake --install out/build/windows-release --prefix out/install/kiyosi
```

An installed consumer can link the exported CMake target:

```cmake
find_package(kiyosi CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE kiyosi::kiyosi)
```

Include the public API with:

```cpp
#include <kiyosi/kiyosi.hpp>
```

## License

kiyosi is licensed under the MIT License. See [LICENSE.txt](LICENSE.txt).
