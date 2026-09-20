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
        : lower_(size - 2), diagonal_(size - 2), upper_diagonal_(size - 2), rhs_(size - 2) {}

    /// `constraint` pins a node to a fixed value, which barrier and autocallable engines use to
    /// overwrite knocked-out regions in place.
    template <typename Constraint>
    bool advance(
        const std::vector<double>& old, std::vector<double>& next, double dt, double rate,
        double dividend, double volatility, double theta, double lower_boundary, double asset_upper_boundary,
        Constraint constraint)
    {
        next.front() = lower_boundary;
        next.back() = asset_upper_boundary;
        const int asset_step_count = static_cast<int>(old.size()) - 1;
        for (int index = 1; index < asset_step_count; ++index) {
            const double i = static_cast<double>(index);
            const double a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i;
            const double b = -volatility * volatility * i * i - rate;
            const double c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            if (const auto fixed = constraint(index)) {
                lower_[position] = upper_diagonal_[position] = 0.0;
                diagonal_[position] = 1.0;
                rhs_[position] = *fixed;
                continue;
            }
            rhs_[position] = old[static_cast<std::size_t>(index)] +
                             (1.0 - theta) * dt *
                                 (a * old[position] + b * old[static_cast<std::size_t>(index)] +
                                  c * old[static_cast<std::size_t>(index + 1)]);
            if (index == 1) rhs_[position] += theta * dt * a * next.front();
            if (index == asset_step_count - 1) rhs_[position] += theta * dt * c * next.back();
            lower_[position] = -theta * dt * a;
            diagonal_[position] = 1.0 - theta * dt * b;
            upper_diagonal_[position] = -theta * dt * c;
        }
        if (theta == 0.0) {
            std::copy(rhs_.begin(), rhs_.end(), next.begin() + 1);
            return std::ranges::all_of(next, [](double value) { return std::isfinite(value); });
        }
        for (std::size_t index = 1; index < diagonal_.size(); ++index) {
            if (!std::isfinite(diagonal_[index - 1]) || diagonal_[index - 1] == 0.0) return false;
            const double factor = lower_[index] / diagonal_[index - 1];
            diagonal_[index] -= factor * upper_diagonal_[index - 1];
            rhs_[index] -= factor * rhs_[index - 1];
        }
        if (diagonal_.empty() || !std::isfinite(diagonal_.back()) || diagonal_.back() == 0.0) return false;
        rhs_.back() /= diagonal_.back();
        for (std::size_t index = diagonal_.size() - 1; index-- > 0;)
            rhs_[index] = (rhs_[index] - upper_diagonal_[index] * rhs_[index + 1]) / diagonal_[index];
        if (!std::ranges::all_of(rhs_, [](double value) { return std::isfinite(value); })) return false;
        std::copy(rhs_.begin(), rhs_.end(), next.begin() + 1);
        return std::isfinite(next.front()) && std::isfinite(next.back());
    }

    bool advance(const std::vector<double>& old, std::vector<double>& next, double dt, double rate,
                 double dividend, double volatility, double theta, double lower_boundary,
                 double asset_upper_boundary)
    {
        return advance(old, next, dt, rate, dividend, volatility, theta, lower_boundary, asset_upper_boundary,
                       [](int) -> std::optional<double> { return std::nullopt; });
    }

    /// Advances two unconstrained layers that share the same finite-difference matrix.
    bool advance_pair(
        const std::vector<double>& first_old, std::vector<double>& first_next,
        const std::vector<double>& second_old, std::vector<double>& second_next, double dt,
        const DiffusionParameters& parameters,
        Boundaries first_boundaries, Boundaries second_boundaries)
    {
        const auto [rate, dividend, volatility, theta] = parameters;
        first_next.front() = first_boundaries.lower;
        first_next.back() = first_boundaries.upper;
        second_next.front() = second_boundaries.lower;
        second_next.back() = second_boundaries.upper;
        paired_rhs_.resize(rhs_.size());
        const int asset_step_count = static_cast<int>(first_old.size()) - 1;
        for (int index = 1; index < asset_step_count; ++index) {
            const double i = static_cast<double>(index);
            const double a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i;
            const double b = -volatility * volatility * i * i - rate;
            const double c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            rhs_[position] = first_old[static_cast<std::size_t>(index)] +
                             (1.0 - theta) * dt *
                                 (a * first_old[position] +
                                  b * first_old[static_cast<std::size_t>(index)] +
                                  c * first_old[static_cast<std::size_t>(index + 1)]);
            paired_rhs_[position] = second_old[static_cast<std::size_t>(index)] +
                                    (1.0 - theta) * dt *
                                        (a * second_old[position] +
                                         b * second_old[static_cast<std::size_t>(index)] +
                                         c * second_old[static_cast<std::size_t>(index + 1)]);
            if (index == 1) {
                rhs_[position] += theta * dt * a * first_next.front();
                paired_rhs_[position] += theta * dt * a * second_next.front();
            }
            if (index == asset_step_count - 1) {
                rhs_[position] += theta * dt * c * first_next.back();
                paired_rhs_[position] += theta * dt * c * second_next.back();
            }
            lower_[position] = -theta * dt * a;
            diagonal_[position] = 1.0 - theta * dt * b;
            upper_diagonal_[position] = -theta * dt * c;
        }
        if (theta == 0.0) {
            std::copy(rhs_.begin(), rhs_.end(), first_next.begin() + 1);
            std::copy(paired_rhs_.begin(), paired_rhs_.end(), second_next.begin() + 1);
            return std::ranges::all_of(first_next, [](double value) { return std::isfinite(value); }) &&
                   std::ranges::all_of(second_next,
                                       [](double value) { return std::isfinite(value); });
        }
        for (std::size_t index = 1; index < diagonal_.size(); ++index) {
            if (!std::isfinite(diagonal_[index - 1]) || diagonal_[index - 1] == 0.0) return false;
            const double factor = lower_[index] / diagonal_[index - 1];
            diagonal_[index] -= factor * upper_diagonal_[index - 1];
            rhs_[index] -= factor * rhs_[index - 1];
            paired_rhs_[index] -= factor * paired_rhs_[index - 1];
        }
        if (diagonal_.empty() || !std::isfinite(diagonal_.back()) || diagonal_.back() == 0.0)
            return false;
        rhs_.back() /= diagonal_.back();
        paired_rhs_.back() /= diagonal_.back();
        for (std::size_t index = diagonal_.size() - 1; index-- > 0;) {
            rhs_[index] = (rhs_[index] - upper_diagonal_[index] * rhs_[index + 1]) / diagonal_[index];
            paired_rhs_[index] =
                (paired_rhs_[index] - upper_diagonal_[index] * paired_rhs_[index + 1]) /
                diagonal_[index];
        }
        if (!std::ranges::all_of(rhs_, [](double value) { return std::isfinite(value); }) ||
            !std::ranges::all_of(paired_rhs_, [](double value) { return std::isfinite(value); }))
            return false;
        std::copy(rhs_.begin(), rhs_.end(), first_next.begin() + 1);
        std::copy(paired_rhs_.begin(), paired_rhs_.end(), second_next.begin() + 1);
        return std::isfinite(first_next.front()) && std::isfinite(first_next.back()) &&
               std::isfinite(second_next.front()) && std::isfinite(second_next.back());
    }

private:
    std::vector<double> lower_, diagonal_, upper_diagonal_, rhs_;
    std::vector<double> paired_rhs_;
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
        const Boundaries edges = boundary_values(old, dt);
        return step_.advance(old, next, dt, parameters_.rate, parameters_.dividend,
                             parameters_.volatility, parameters_.theta, edges.lower, edges.upper);
    }

    bool advance_pair(
        const std::vector<double>& first_old, std::vector<double>& first_next,
        const std::vector<double>& second_old, std::vector<double>& second_next, double dt)
    {
        return step_.advance_pair(
            first_old, first_next, second_old, second_next, dt, parameters_,
            boundary_values(first_old, dt), boundary_values(second_old, dt));
    }

private:
    Boundaries boundary_values(const std::vector<double>& old, double dt) const
    {
        const double slope = (old.back() - old[old.size() - 2]) / spacing_;
        const double intercept = old.back() - slope * upper_;
        return {old.front() * std::exp(-parameters_.rate * dt),
                slope * upper_ * std::exp(-parameters_.dividend * dt) +
                    intercept * std::exp(-parameters_.rate * dt)};
    }

    FiniteDifferenceStep step_;
    double upper_;
    double spacing_;
    DiffusionParameters parameters_;
};

/// Marches one value layer from expiry_date back to valuation; `tau` is the time remaining to expiry_date.
template <typename BoundaryValues,
          typename Event = decltype([](std::vector<double>&, double, double) {}),
          typename Constraint = decltype([](int, double) { return std::optional<double>{}; })>
[[nodiscard]] inline Result<void> march_backward(
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
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "finite-difference system is numerically unstable"});
        event(next, tau, grid[step]);
        layer.swap(next);
    }
    return {};
}

} // namespace kiyosi::detail
