#pragma once

#include <algorithm>
#include <cmath>
#include <optional>
#include <ranges>
#include <span>
#include <vector>

#include <kiyosi/core/error.hpp>

namespace kiyosi::detail {

struct DiffusionParameters {
    double rate;
    double dividend;
    double volatility;
    double theta;
};

struct Boundaries {
    double lower;
    double upper;
};

/// One theta-weighted Black-Scholes time step solved with the Thomas algorithm.
/// Reuses its coefficient buffers so a full backward march allocates once.
class FiniteDifferenceStep {
public:
    explicit FiniteDifferenceStep(std::size_t size)
        : lower(size - 2), diagonal(size - 2), upper_diagonal(size - 2), rhs(size - 2) {}

    /// `constraint` pins a node to a fixed value, which barrier and autocallable engines use to
    /// overwrite knocked-out regions in place.
    template <typename Constraint>
    bool advance(
        const std::vector<double>& old, std::vector<double>& next, double dt, double rate,
        double dividend, double volatility, double theta, double lower_boundary, double upper_boundary,
        Constraint constraint)
    {
        next.front() = lower_boundary;
        next.back() = upper_boundary;
        const int asset_steps = static_cast<int>(old.size()) - 1;
        for (int index = 1; index < asset_steps; ++index) {
            const double i = static_cast<double>(index);
            const double a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i;
            const double b = -volatility * volatility * i * i - rate;
            const double c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            if (const auto fixed = constraint(index)) {
                lower[position] = upper_diagonal[position] = 0.0;
                diagonal[position] = 1.0;
                rhs[position] = *fixed;
                continue;
            }
            rhs[position] = old[static_cast<std::size_t>(index)] +
                            (1.0 - theta) * dt *
                                (a * old[position] + b * old[static_cast<std::size_t>(index)] +
                                 c * old[static_cast<std::size_t>(index + 1)]);
            if (index == 1) rhs[position] += theta * dt * a * next.front();
            if (index == asset_steps - 1) rhs[position] += theta * dt * c * next.back();
            lower[position] = -theta * dt * a;
            diagonal[position] = 1.0 - theta * dt * b;
            upper_diagonal[position] = -theta * dt * c;
        }
        if (theta == 0.0) {
            std::copy(rhs.begin(), rhs.end(), next.begin() + 1);
            return std::ranges::all_of(next, [](double value) { return std::isfinite(value); });
        }
        for (std::size_t index = 1; index < diagonal.size(); ++index) {
            if (!std::isfinite(diagonal[index - 1]) || diagonal[index - 1] == 0.0) return false;
            const double factor = lower[index] / diagonal[index - 1];
            diagonal[index] -= factor * upper_diagonal[index - 1];
            rhs[index] -= factor * rhs[index - 1];
        }
        if (diagonal.empty() || !std::isfinite(diagonal.back()) || diagonal.back() == 0.0) return false;
        rhs.back() /= diagonal.back();
        for (std::size_t index = diagonal.size() - 1; index-- > 0;)
            rhs[index] = (rhs[index] - upper_diagonal[index] * rhs[index + 1]) / diagonal[index];
        if (!std::ranges::all_of(rhs, [](double value) { return std::isfinite(value); })) return false;
        std::copy(rhs.begin(), rhs.end(), next.begin() + 1);
        return std::ranges::all_of(next, [](double value) { return std::isfinite(value); });
    }

    bool advance(const std::vector<double>& old, std::vector<double>& next, double dt, double rate,
                 double dividend, double volatility, double theta, double lower_boundary,
                 double upper_boundary)
    {
        return advance(old, next, dt, rate, dividend, volatility, theta, lower_boundary, upper_boundary,
                       [](int) -> std::optional<double> { return std::nullopt; });
    }

private:
    std::vector<double> lower, diagonal, upper_diagonal, rhs;
};

/// Stepper for layers that grow without bound at the top of the grid, such as autocallable
/// principal and accumulator accruals. The upper edge is extrapolated from the top two nodes and
/// carried forward by discounting the slope at the dividend yield and the intercept at the rate.
class LinearBoundaryStepper {
public:
    LinearBoundaryStepper(std::size_t size, double upper, double spacing, DiffusionParameters parameters)
        : step_(size), upper_(upper), spacing_(spacing), parameters_(parameters) {}

    bool advance(const std::vector<double>& old, std::vector<double>& next, double dt)
    {
        const double slope = (old.back() - old[old.size() - 2]) / spacing_;
        const double intercept = old.back() - slope * upper_;
        const double high = slope * upper_ * std::exp(-parameters_.dividend * dt) +
                            intercept * std::exp(-parameters_.rate * dt);
        return step_.advance(old, next, dt, parameters_.rate, parameters_.dividend,
                             parameters_.volatility, parameters_.theta,
                             old.front() * std::exp(-parameters_.rate * dt), high);
    }

private:
    FiniteDifferenceStep step_;
    double upper_;
    double spacing_;
    DiffusionParameters parameters_;
};

/// Marches one value layer from expiry back to valuation; `tau` is the time remaining to expiry.
template <typename BoundaryValues,
          typename Event = decltype([](std::vector<double>&, double, double) {}),
          typename Constraint = decltype([](int, double) { return std::optional<double>{}; })>
[[nodiscard]] inline result<void> march_backward(
    std::span<const double> grid, const DiffusionParameters& parameters, std::vector<double>& layer,
    BoundaryValues boundaries, Event event = {}, Constraint constraint = {})
{
    const double maturity = grid.back();
    std::vector<double> next(layer.size());
    FiniteDifferenceStep stepper(layer.size());
    for (std::size_t step = grid.size() - 1; step-- > 0;) {
        const double dt = grid[step + 1] - grid[step];
        const double tau = maturity - grid[step];
        const Boundaries edges = boundaries(tau);
        if (!stepper.advance(layer, next, dt, parameters.rate, parameters.dividend,
                             parameters.volatility, parameters.theta, edges.lower, edges.upper,
                             [&](int index) { return constraint(index, tau); }))
            return std::unexpected(Error{error_category::invalid_result,
                                         "finite-difference system is numerically unstable"});
        event(next, tau, grid[step]);
        layer.swap(next);
    }
    return {};
}

} // namespace kiyosi::detail
