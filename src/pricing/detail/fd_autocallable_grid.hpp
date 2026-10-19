#pragma once

#include <algorithm>
#include <cmath>
#include <numeric>
#include <vector>

#include "fd_grid.hpp"

namespace kiyosi::detail {

/// Sinh half-intervals share a physical concentration scale at every contractual level.
inline SpatialGrid concentrate_autocallable_grid(SpatialGrid grid, std::vector<double> levels, double scale)
{
    std::erase_if(levels, [&](double level) { return level <= 0.0 || level >= grid.upper; });
    std::ranges::sort(levels);
    const SpatialGrid domain{grid.upper, 0.0, 0, scale};
    const double minimum_gap = scale * domain.coordinate_of(grid.upper) / grid.asset_step_count;
    // Sub-resolution anchors create ill-conditioned cells; event thresholds stay unchanged.
    std::erase_if(levels, [minimum_gap, upper = grid.upper, previous = 0.0](double level) mutable {
        if (level - previous < minimum_gap || upper - level < minimum_gap) return true;
        previous = level;
        return false;
    });
    if (levels.empty()) return grid;

    struct Segment {
        double lower;
        double upper;
        double width;
        bool from_left;
        int steps = 1;
        double remainder = 0.0;
    };
    std::vector<Segment> segments;
    segments.reserve(2 * levels.size());
    const auto add = [&](double lower, double upper, bool from_left) {
        if (lower < upper) {
            const SpatialGrid transform{upper - lower, 0.0, 0, scale};
            segments.push_back({lower, upper, transform.coordinate_of(upper - lower), from_left});
        }
    };
    add(0.0, levels.front(), false);
    for (std::size_t index = 1; index < levels.size(); ++index) {
        const double middle = std::midpoint(levels[index - 1], levels[index]);
        add(levels[index - 1], middle, true);
        add(middle, levels[index], false);
    }
    add(levels.back(), grid.upper, true);
    if (std::cmp_greater(segments.size(), grid.asset_step_count)) return grid;
    const int available = grid.asset_step_count - static_cast<int>(segments.size());
    double total = 0.0;
    for (const auto& segment : segments)
        total += segment.width;
    int allocated = 0;
    for (auto& segment : segments) {
        const double count = available * (segment.width / total);
        const auto integral = static_cast<int>(count);
        segment.steps += integral;
        segment.remainder = count - integral;
        allocated += integral;
    }
    for (int index = allocated; index < available; ++index) {
        auto found = std::ranges::max_element(segments, {}, &Segment::remainder);
        ++found->steps;
        found->remainder = -1.0;
    }
    double coordinate_spacing = total / grid.asset_step_count;
    for (const auto& segment : segments)
        if (segment.steps > 1)
            coordinate_spacing = std::min(coordinate_spacing, segment.width / (segment.steps - 0.5));
    grid.nodes.reserve(grid.size());
    grid.nodes.push_back(0.0);
    for (const auto& segment : segments) {
        const SpatialGrid transform{segment.upper - segment.lower, 0.0, 0, scale};
        for (int step = 1; step <= segment.steps; ++step) {
            // Match spacing on both sides of each anchor; absorb remainders away from it.
            const double coordinate = coordinate_spacing * (segment.from_left ? step : segment.steps - step);
            const double value = step == segment.steps ? segment.upper
                                 : segment.from_left   ? segment.lower + transform.asset_of_coordinate(coordinate)
                                                       : segment.upper - transform.asset_of_coordinate(coordinate);
            if (value > grid.nodes.back()) grid.nodes.push_back(value);
        }
    }
    grid.asset_step_count = static_cast<int>(grid.nodes.size()) - 1;
    grid.spacing = 1.0;
    grid.sinh_scale = 0.0;
    return grid;
}

} // namespace kiyosi::detail
