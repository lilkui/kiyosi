# Use a value-oriented C++ boundary

The public C++ library will expose concrete immutable instrument values, explicit pricing algorithms, `std::expected` results for validation and numerical failure, standard day-based `std::chrono` dates, `double` as the public scalar, and compiled CMake targets. Parity will be tested with language-neutral fixtures rather than by preserving the C# inheritance hierarchy or test structure.
