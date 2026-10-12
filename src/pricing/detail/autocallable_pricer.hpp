#pragma once

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi::detail {

// Keeps request-local validation available to scalar Greek stencils.
template <typename Price>
struct AutocallablePricer {
    Price evaluate;

    template <typename Note>
    Result<double> price(const Note& note, const PricingContext& context) const
    {
        return evaluate(note, context);
    }
};

} // namespace kiyosi::detail
