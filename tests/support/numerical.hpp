#pragma once

#include "pricing/detail/numerical_greeks.hpp"

namespace kiyosi::test {

// Keep scalar-price comparisons independent of an engine's native Greeks.
template <typename Engine, typename Option>
Result<PricingResult> price_differences(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    if constexpr (kiyosi::detail::is_builtin_monte_carlo_engine<Engine, Option>)
        return engine.price_with_greeks(option, context, true, settings);
    else
        return kiyosi::detail::price_with_greeks(engine, option, context, true, settings);
}

} // namespace kiyosi::test
