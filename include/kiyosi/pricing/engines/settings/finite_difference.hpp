#pragma once



namespace kiyosi {

enum class finite_difference_scheme : unsigned char {
    explicit_euler,
    implicit_euler,
    crank_nicolson,
};

struct FiniteDifferenceSettings {
    int asset_steps = 200;
    int time_steps = 200;
    finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson;
    double upper_boundary = 0.0;
};

} // namespace kiyosi
