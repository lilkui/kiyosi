#pragma once

#include <algorithm>
#include <cmath>
#include <span>

namespace kiyosi::detail {

// The span includes the right-hand child beyond the nodes being overwritten.
template <bool american>
bool binomial_rollback(std::span<double> values, double discount, double probability,
                       double node_spot, double up_squared, double strike, double sign)
{
    for (std::size_t index = 0; index + 1 < values.size(); ++index) {
        const double continuation = discount * (probability * values[index + 1] + (1.0 - probability) * values[index]);
        if constexpr (american) {
            const double exercise = sign * (node_spot - strike);
            values[index] = std::max(continuation, exercise);
            node_spot *= up_squared;
        } else {
            values[index] = continuation;
        }
        if (!std::isfinite(values[index])) return false;
    }
    return true;
}

} // namespace kiyosi::detail
