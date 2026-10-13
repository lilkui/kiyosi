#pragma once

#include <kiyosi/pricing/detail/autocallable_program.hpp>

#include "monte_carlo_discount.hpp"

namespace kiyosi::detail {

struct SimulationStep {
    double drift{};
    double diffusion{};
    MonteCarloDiscount discount{};
};

struct AutocallableStep {
    SimulationStep simulation{};
    AutocallableEvent event{};
    double alternate_coupon{};
};

} // namespace kiyosi::detail
