#pragma once

namespace kiyosi::detail {

struct FiniteDifferenceSubsteps {
    int count;
    double theta;
};

/// Replaces each initial Crank-Nicolson interval with two implicit Euler half-steps.
/// The caller restarts after a discontinuity; instances own their scheduling state.
class RannacherDamping {
public:
    /// Requires a nonnegative interval count, chosen by the calling engine.
    explicit RannacherDamping(int intervals) noexcept
        : intervals_(intervals), remaining_(intervals) {}

    void restart() noexcept { remaining_ = intervals_; }

    /// Consumes one time-grid interval, regardless of its width or number of value layers.
    /// theta is the base weight of the caller's validated time-marching scheme.
    [[nodiscard]] FiniteDifferenceSubsteps next(double theta) noexcept
    {
        const bool damp = theta == 0.5 && remaining_ > 0;
        if (remaining_ > 0) --remaining_;
        return {damp ? 2 : 1, damp ? 1.0 : theta};
    }

private:
    int intervals_;
    int remaining_;
};

} // namespace kiyosi::detail
