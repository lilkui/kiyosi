# Use kiyosi as the C++ library identity

The C++ library and public namespace use `kiyosi`. CTest runs Catch2-based tests, while the production core remains standard-library-only; parity fixtures use a documented line-oriented CSV/TSV format. Performance profiling is deferred until a concrete need appears.
