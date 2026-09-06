# Use kiyosi as the C++ library identity

The C++ library and public namespace will use `kiyosi`, targeting Windows x64 and Linux x64 first. CTest will run Catch2-based tests, while the production core remains standard-library-only; parity fixtures use a documented line-oriented CSV/TSV format. The initial release has no performance acceptance requirement; profiling can be added later if a concrete need appears.
