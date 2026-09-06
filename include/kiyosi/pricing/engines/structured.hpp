#pragma once

#include <algorithm>
#include <cstdint>
#include <optional>
#include <kiyosi/instruments/structured.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/engines/finite_difference.hpp>

namespace kiyosi {
struct StructuredMonteCarloSettings {
    int path_count = 20'000;
    std::optional<std::uint64_t> seed = 1;
};

template <typename Option>
class MonteCarloStructuredEngine {
public:
    explicit MonteCarloStructuredEngine(StructuredMonteCarloSettings settings = {}) : settings_(settings) {}
    explicit MonteCarloStructuredEngine(int path_count, bool = false, std::optional<std::uint64_t> seed = 1)
        : settings_{path_count, seed} {}
    [[nodiscard]] result<PricingResult> price(const Option&, const PricingContext&) const;
    StructuredMonteCarloSettings settings() const noexcept { return settings_; }

private:
    StructuredMonteCarloSettings settings_;
};

using MonteCarloAccumulatorEngine = MonteCarloStructuredEngine<Accumulator>;
using MonteCarloPhoenixEngine = MonteCarloStructuredEngine<PhoenixOption>;
using MonteCarloSnowballEngine = MonteCarloStructuredEngine<SnowballOption>;
using MonteCarloBinarySnowballEngine = MonteCarloStructuredEngine<BinarySnowballOption>;
using MonteCarloTernarySnowballEngine = MonteCarloStructuredEngine<TernarySnowballOption>;
using McAccumulatorEngine = MonteCarloAccumulatorEngine;
using McPhoenixEngine = MonteCarloPhoenixEngine;
using McSnowballEngine = MonteCarloSnowballEngine;
using McBinarySnowballEngine = MonteCarloBinarySnowballEngine;
using McTernarySnowballEngine = MonteCarloTernarySnowballEngine;

template <typename Option>
class FiniteDifferenceStructuredEngine {
public:
    explicit FiniteDifferenceStructuredEngine(FiniteDifferenceSettings settings = {}) : settings_(settings) {}
    FiniteDifferenceStructuredEngine(int asset_steps, int time_steps,
                                     finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}
    [[nodiscard]] result<PricingResult> price(const Option& option, const PricingContext& context) const
    {
        return MonteCarloStructuredEngine<Option>{{std::max(2'000, settings_.asset_steps * settings_.time_steps / 2), 1}}.price(option, context);
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
using McAutocallableEngine = MonteCarloBinarySnowballEngine;
using McKiAutocallableEngine = MonteCarloSnowballEngine;
using FdAutocallableEngine = FiniteDifferenceBinarySnowballEngine;
using FdKiAutocallableEngine = FiniteDifferenceSnowballEngine;
} // namespace kiyosi
