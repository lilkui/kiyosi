#pragma once

#include <kiyosi/instruments/structured.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/engines/settings/finite_difference.hpp>

namespace kiyosi {

template <typename Option>
[[nodiscard]] KIYOSI_EXPORT result<PricingResult> price_finite_difference_structured(
    const Option&, const PricingContext&, FiniteDifferenceSettings);

template <typename Option>
class KIYOSI_EXPORT FiniteDifferenceStructuredEngine {
public:
    explicit FiniteDifferenceStructuredEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceStructuredEngine(int asset_steps, int time_steps,
                                     finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}
    [[nodiscard]] result<PricingResult> price(const Option& option, const PricingContext& context) const
    {
        return price_finite_difference_structured(option, context, settings_);
    }

private:
    FiniteDifferenceSettings settings_;
};
using FiniteDifferenceAccumulatorEngine = FiniteDifferenceStructuredEngine<Accumulator>;
using FiniteDifferencePhoenixEngine = FiniteDifferenceStructuredEngine<PhoenixOption>;
using FiniteDifferenceSnowballEngine = FiniteDifferenceStructuredEngine<SnowballOption>;
using FiniteDifferenceBinarySnowballEngine = FiniteDifferenceStructuredEngine<BinarySnowballOption>;
using FiniteDifferenceTernarySnowballEngine = FiniteDifferenceStructuredEngine<TernarySnowballOption>;
using FdAccumulatorEngine = FiniteDifferenceAccumulatorEngine;
using FdPhoenixEngine = FiniteDifferencePhoenixEngine;
using FdSnowballEngine = FiniteDifferenceSnowballEngine;
using FdBinarySnowballEngine = FiniteDifferenceBinarySnowballEngine;
using FdTernarySnowballEngine = FiniteDifferenceTernarySnowballEngine;
using FdAutocallableEngine = FiniteDifferenceBinarySnowballEngine;
using FdKiAutocallableEngine = FiniteDifferenceSnowballEngine;

} // namespace kiyosi
