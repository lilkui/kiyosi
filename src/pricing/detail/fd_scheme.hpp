#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <optional>
#include <ranges>
#include <span>
#include <vector>

#include <kiyosi/core/error.hpp>

#include "fd_grid.hpp"

namespace kiyosi::detail {

struct DiffusionParameters {
    double rate;
    double dividend;
    double volatility;
    double theta;
    double sinh_spacing = 0.0;
    friend bool operator==(const DiffusionParameters&, const DiffusionParameters&) = default;
};

struct Boundaries {
    double lower;
    double upper;
};

inline std::array<double, 3> diffusion_coefficients(double node, double rate, double dividend, double volatility)
{
    const double drift = (rate - dividend) * node;
    // Switch to one-sided drift when the centered stencil would have a negative off-diagonal.
    const double diffusion = std::max(0.5 * volatility * volatility * node * node, 0.5 * std::abs(drift));
    return {diffusion - 0.5 * drift, -2.0 * diffusion - rate, diffusion + 0.5 * drift};
}

inline std::array<double, 3> diffusion_coefficients(double node, const DiffusionParameters& parameters)
{
    const double rate = parameters.rate;
    const double dividend = parameters.dividend;
    const double volatility = parameters.volatility;
    const double spacing = parameters.sinh_spacing;
    if (spacing == 0.0) return diffusion_coefficients(node, rate, dividend, volatility);
    const double tangent = std::tanh(node * spacing);
    const double drift = ((rate - dividend) - 0.5 * volatility * volatility * tangent * tangent) * tangent / spacing;
    const double scaled_volatility = volatility * tangent / spacing;
    const double diffusion = std::max(0.5 * scaled_volatility * scaled_volatility, 0.5 * std::abs(drift));
    return {diffusion - 0.5 * drift, -2.0 * diffusion - rate, diffusion + 0.5 * drift};
}

/// One theta-weighted Black-Scholes time step solved with the Thomas algorithm.
/// Reuses buffers and two factorizations for recurring time steps and constraint masks.
class FiniteDifferenceStep {
    struct Layer {
        std::span<const double> old;
        std::span<double> next;
        std::span<double> rhs;
        Boundaries boundaries;
    };

    struct Factorization {
        explicit Factorization(std::size_t size = 0)
            : lower(size), diagonal(size), upper_diagonal(size), fixed(size), multipliers(size), inverse(size) {}

        std::vector<double> lower, diagonal, upper_diagonal;
        std::vector<bool> fixed;
        std::vector<double> multipliers, inverse;
        double dt{};
        std::optional<DiffusionParameters> parameters;
    };

public:
    /// Requires size >= 4 from validated grid settings; every layer must have this size.
    explicit FiniteDifferenceStep(std::size_t size)
        : rhs_(size - 2) {}

    /// `constraint` pins a node to a fixed value, which barrier and autocallable engines use to
    /// overwrite knocked-out regions in place.
    template <typename Constraint = decltype([](int) { return std::optional<double>{}; })>
    bool advance(const std::vector<double>& old, std::vector<double>& next, double dt,
                 const DiffusionParameters& parameters, double lower_boundary,
                 double upper_boundary, Constraint constraint = {})
    {
        return advance_layers(
            std::array{Layer{old, next, rhs_, {lower_boundary, upper_boundary}}}, dt,
            parameters, constraint);
    }

    /// Advances two unconstrained layers that share the same finite-difference matrix.
    bool advance_pair(
        const std::vector<double>& first_old, std::vector<double>& first_next,
        const std::vector<double>& second_old, std::vector<double>& second_next, double dt,
        const DiffusionParameters& parameters,
        Boundaries first_boundaries, Boundaries second_boundaries)
    {
        paired_rhs_.resize(rhs_.size());
        return advance_layers(
            std::array{Layer{first_old, first_next, rhs_, first_boundaries},
                       Layer{second_old, second_next, paired_rhs_, second_boundaries}},
            dt, parameters, [](int) -> std::optional<double> { return std::nullopt; });
    }

private:
    template <std::size_t count, typename Constraint>
    bool advance_layers(const std::array<Layer, count>& layers, double dt,
                        const DiffusionParameters& parameters, Constraint constraint)
    {
        const double theta = parameters.theta;
        // Calendar grids repeatedly alternate weekday and weekend interval widths.
        if (theta != 0.0 && (factors_[active_].dt != dt || factors_[active_].parameters != parameters))
            active_ = 1 - active_;
        auto& factors = factors_[active_];
        if (theta != 0.0 && factors.diagonal.empty()) factors = Factorization{rhs_.size()};
        bool reuse = factors.dt == dt && factors.parameters == parameters;
        if (theta != 0.0) factors.parameters.reset();
        for (const auto& layer : layers) {
            layer.next.front() = layer.boundaries.lower;
            layer.next.back() = layer.boundaries.upper;
        }
        const int asset_step_count = static_cast<int>(layers.front().old.size()) - 1;
        if (cached_parameters_ != parameters) {
            coefficients_.resize(rhs_.size());
            for (int index = 1; index < asset_step_count; ++index)
                coefficients_[static_cast<std::size_t>(index - 1)] = diffusion_coefficients(index, parameters);
            cached_parameters_ = parameters;
        }
        std::size_t first = rhs_.size(), last = 0;
        for (int index = 1; index < asset_step_count; ++index) {
            const auto position = static_cast<std::size_t>(index - 1);
            const auto [a, b, c] = coefficients_[position];
            const auto fixed = constraint(index);
            if (theta != 0.0) {
                reuse = reuse && factors.fixed[position] == fixed.has_value();
                factors.fixed[position] = fixed.has_value();
            }
            if (fixed) {
                for (const auto& layer : layers)
                    layer.rhs[position] = *fixed;
                continue;
            }
            first = std::min(first, position);
            last = position + 1;
            for (const auto& layer : layers) {
                // Implicit Euler has no explicit term; evaluating it can overflow before multiplication by zero.
                layer.rhs[position] = theta == 1.0 ? layer.old[static_cast<std::size_t>(index)] : layer.old[static_cast<std::size_t>(index)] + (1.0 - theta) * dt * (a * layer.old[position] + b * layer.old[static_cast<std::size_t>(index)] + c * layer.old[static_cast<std::size_t>(index) + 1]);
                if (index == 1) layer.rhs[position] += theta * dt * a * layer.next.front();
                if (index == asset_step_count - 1) layer.rhs[position] += theta * dt * c * layer.next.back();
            }
        }
        // A changed constraint near the upper edge also invalidates earlier matrix rows.
        if (theta != 0.0 && !reuse)
            for (std::size_t position = 0; position < rhs_.size(); ++position) {
                if (factors.fixed[position]) {
                    factors.lower[position] = factors.upper_diagonal[position] = 0.0;
                    factors.diagonal[position] = 1.0;
                } else {
                    const auto [a, b, c] = coefficients_[position];
                    factors.lower[position] = -theta * dt * a;
                    factors.diagonal[position] = 1.0 - theta * dt * b;
                    factors.upper_diagonal[position] = -theta * dt * c;
                }
            }
        if (theta != 0.0 && !solve(layers, factors, reuse, first, last)) return false;
        for (const auto& layer : layers)
            std::ranges::copy(layer.rhs, layer.next.begin() + 1);
        const bool valid = std::ranges::all_of(layers, [theta](const auto& layer) {
            if (theta == 0.0)
                return std::ranges::all_of(layer.next, [](double value) { return std::isfinite(value); });
            return std::isfinite(layer.next.front()) && std::isfinite(layer.next.back());
        });
        if (valid && theta != 0.0) {
            factors.dt = dt;
            factors.parameters = parameters;
        }
        return valid;
    }

    // Factor the matrix once and validate every solved layer before callers copy any interiors.
    template <std::size_t count>
    bool solve(const std::array<Layer, count>& layers, Factorization& factors, bool reuse, std::size_t first, std::size_t last)
    {
        // Fixed edge rows are identity equations; only their adjacent values enter the live solve.
        if (first < last && first != 0)
            for (const auto& layer : layers)
                layer.rhs[first] -= factors.lower[first] * layer.rhs[first - 1];
        for (std::size_t index = first + 1; index < last; ++index) {
            if (!std::isfinite(factors.diagonal[index - 1]) || factors.diagonal[index - 1] == 0.0) return false;
            const double factor = reuse ? factors.multipliers[index] : factors.lower[index] / factors.diagonal[index - 1];
            if (!reuse) {
                factors.multipliers[index] = factor;
                factors.diagonal[index] -= factor * factors.upper_diagonal[index - 1];
            }
            for (const auto& layer : layers)
                layer.rhs[index] -= factor * layer.rhs[index - 1];
        }
        if (first < last) {
            if (!std::isfinite(factors.diagonal[last - 1]) || factors.diagonal[last - 1] == 0.0) return false;
            if (!reuse) {
                for (std::size_t index = first; index < last; ++index)
                    factors.inverse[index] = 1.0 / factors.diagonal[index];
            }
            for (const auto& layer : layers) {
                const double value = layer.rhs[last - 1] - (last < factors.diagonal.size() ? factors.upper_diagonal[last - 1] * layer.rhs[last] : 0.0);
                layer.rhs[last - 1] = std::isfinite(factors.inverse[last - 1]) ? value * factors.inverse[last - 1]
                                                                               : value / factors.diagonal[last - 1];
            }
            for (std::size_t index = last - 1; index-- > first;)
                for (const auto& layer : layers) {
                    const double value = layer.rhs[index] - factors.upper_diagonal[index] * layer.rhs[index + 1];
                    // A finite pivot can have an overflowing reciprocal.
                    layer.rhs[index] = std::isfinite(factors.inverse[index]) ? value * factors.inverse[index] : value / factors.diagonal[index];
                }
        }
        return std::ranges::all_of(layers, [](const auto& layer) {
            return std::ranges::all_of(layer.rhs, [](double value) { return std::isfinite(value); });
        });
    }

    std::vector<double> rhs_;
    std::vector<double> paired_rhs_;
    std::array<Factorization, 2> factors_;
    std::size_t active_{};
    std::optional<DiffusionParameters> cached_parameters_;
    std::vector<std::array<double, 3>> coefficients_;
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
        return step_.advance(old, next, dt, parameters_, edges.lower, edges.upper);
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
    const FiniteDifferenceTimeGrid& grid, const DiffusionParameters& parameters, std::vector<double>& layer,
    BoundaryValues boundaries, Event event = {}, Constraint constraint = {})
{
    const double maturity = grid.times.back();
    std::vector<double> next(layer.size());
    FiniteDifferenceStep stepper(layer.size());
    for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
        const double dt = grid.time_steps[step];
        const double tau = maturity - grid.times[step];
        const Boundaries edges = boundaries(tau);
        if (!stepper.advance(layer, next, dt, parameters, edges.lower, edges.upper,
                             [&](int index) { return constraint(index, tau); }))
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "finite-difference system is numerically unstable"});
        event(next, tau, grid.times[step]);
        layer.swap(next);
    }
    return {};
}

} // namespace kiyosi::detail
