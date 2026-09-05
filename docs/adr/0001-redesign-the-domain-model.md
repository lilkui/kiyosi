# Redesign the domain model instead of porting the source

The C++ library will redesign DerivaSharp's domain model and public contract rather than translate the C# source line by line. Numerical parity with established reference results remains a correctness constraint, while the new model is free to improve boundaries and ergonomics.
