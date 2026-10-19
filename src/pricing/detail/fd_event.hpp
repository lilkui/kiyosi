#pragma once

#include <algorithm>
#include <array>
#include <cstddef>
#include <numeric>
#include <numbers>
#include <span>

#include "fd_grid.hpp"

namespace kiyosi::detail {

/// Project only cells containing a break; smooth continuation must not be averaged every day.
template <typename Value>
std::array<double, 2> project_finite_difference_event(
    const SpatialGrid& space, std::span<const double> breaks, std::size_t index, const Value& value_at)
{
    const double center = space.coordinate_at(index);
    if (index == 0 || index + 1 == space.size()) return value_at(center);
    const double lower = std::midpoint(space.coordinate_at(index - 1), center);
    const double upper = std::midpoint(center, space.coordinate_at(index + 1));
    const auto first = std::ranges::upper_bound(breaks, lower);
    if (first == breaks.end() || *first >= upper) return value_at(center);

    std::array<double, 2> integral{};
    auto next_break = first;
    double left = lower;
    while (left < upper) {
        double right = upper;
        if (left < center) right = center;
        if (next_break != breaks.end() && *next_break < right) right = *next_break;
        // Two-point Gauss integration also handles linear-in-S payoffs in sinh coordinates.
        constexpr double gauss = std::numbers::inv_sqrt3;
        const double middle = std::midpoint(left, right);
        const double radius = 0.5 * (right - left);
        const auto low = value_at(middle - gauss * radius);
        const auto high = value_at(middle + gauss * radius);
        for (std::size_t layer = 0; layer < integral.size(); ++layer)
            integral[layer] += radius / (upper - lower) * (low[layer] + high[layer]);
        left = right;
        while (next_break != breaks.end() && *next_break <= left)
            ++next_break;
    }
    return integral;
}

} // namespace kiyosi::detail
